use crate::{
    consensus::ConsensusProtocol, error::AxiomError, incentive::IncentiveMechanism,
    state_machine::StateMachine, Action, Message,
};
use rand::{seq::SliceRandom, Rng};
use std::sync::Arc;
use tokio::sync::{
    mpsc::{self, error::TrySendError, Receiver, Sender},
    RwLock,
};
use tokio::time::{timeout, Duration};

/// A node in the Axiom network, integrating state machine, incentives, and consensus.
pub struct Node<SM: StateMachine, IM: IncentiveMechanism, CP: ConsensusProtocol> {
    /// Unique node identifier.
    id: usize,

    /// Current state.
    state: SM::State,

    /// State machine for transitions.
    state_machine: SM,

    /// Incentive mechanism for rewards.
    incentive_mechanism: IM,

    /// Consensus protocol for proposing and checking.
    consensus_protocol: CP,

    /// Accumulated reward.
    reward: IM::Reward,

    /// Sender for broadcasting messages.
    sender: Sender<Action>,

    /// Receiver for incoming messages.
    receiver: Receiver<Action>,
}

impl<SM, IM, CP> Node<SM, IM, CP>
where
    SM: StateMachine<State = f64, Input = f64>,
    IM: IncentiveMechanism<Reward = f64>,
    CP: ConsensusProtocol<State = f64>,
{
    /// Creates a new node with random initial state.
    ///
    /// # Arguments
    /// * `id` - Node identifier.
    /// * `state_machine` - State machine instance.
    /// * `incentive_mechanism` - Incentive mechanism instance.
    /// * `consensus_protocol` - Consensus protocol instance.
    ///
    /// # Returns
    /// A tuple of the node and its sender for external communication.
    pub fn new(
        id: usize,
        state_machine: SM,
        incentive_mechanism: IM,
        consensus_protocol: CP,
    ) -> (Self, Sender<Action>) {
        let (sender, receiver) = mpsc::channel(100);
        let state = rand::thread_rng().gen_range(0.0..1.0); // Random initial state
        (
            Self {
                id,
                state,
                state_machine,
                incentive_mechanism,
                consensus_protocol,
                reward: 0.0,
                sender: sender.clone(),
                receiver,
            },
            sender,
        )
    }

    /// Gets the current state of the node.
    pub fn get_state(&self) -> SM::State {
        self.state.clone()
    }

    /// Gets the current reward of the node.
    pub fn get_reward(&self) -> IM::Reward {
        self.reward.clone()
    }

    /// Processes a single action and returns generated actions.
    async fn process_action(
        &mut self,
        action: Action,
        network: &Arc<Network<SM, IM, CP>>,
    ) -> Result<Vec<Action>, AxiomError> {
        match action {
            Action::SendMessage(_msg) => {
                // Process peer state
                let is_partitioned = network.is_partitioned(self.id).await;
                let peer_states = network.get_peer_states(self.id).await;
                let target_state = self.consensus_protocol.propose(&peer_states);

                // Transition state with w = alpha (normal) or p (partitioned)
                let input = if is_partitioned {
                    target_state
                } else {
                    network.normal_weight
                };

                let (new_state, actions) =
                    self.state_machine
                        .transition(&self.state, input, is_partitioned)?;

                // Update state and reward
                self.state = new_state;
                let reward_delta = self
                    .incentive_mechanism
                    .calculate_reward::<SM>(&self.state, &peer_states)?;
                self.reward += reward_delta;

                Ok(actions)
            }
            Action::UpdateState => {
                // Local update without broadcasting
                let peer_states = network.get_peer_states(self.id).await;
                let reward_delta = self
                    .incentive_mechanism
                    .calculate_reward::<SM>(&self.state, &peer_states)?;
                self.reward += reward_delta;
                Ok(vec![])
            }
        }
    }

    /// Runs the node's main loop, processing messages and updating state.
    async fn run(
        &mut self,
        network: Arc<Network<SM, IM, CP>>,
        shutdown_rx: &mut tokio::sync::oneshot::Receiver<()>,
    ) -> Result<(), AxiomError> {
        loop {
            tokio::select! {
                action = self.receiver.recv() => {
                    match action {
                        Some(action) => {
                            let actions = self.process_action(action, &network).await?;

                            // Broadcast actions
                            for action in actions {
                                if let Err(_) = self.sender.send(action).await {
                                    return Err(AxiomError::NetworkSend(
                                        "Failed to send action".to_string()
                                    ));
                                }
                            }
                        }
                        None => {
                            // Channel closed
                            break;
                        }
                    }
                }
                _ = &mut *shutdown_rx => {
                    break;
                }
            }
        }
        Ok(())
    }
}

/// Network managing a collection of nodes and simulating partitions.
pub struct Network<SM: StateMachine, IM: IncentiveMechanism, CP: ConsensusProtocol> {
    /// List of nodes with RwLock for better concurrent access.
    nodes: Vec<Arc<RwLock<Node<SM, IM, CP>>>>,

    /// Senders for each node.
    senders: Vec<Sender<Action>>,

    /// Current partition configuration (node ID -> group).
    partitions: Arc<RwLock<Vec<Vec<usize>>>>,

    /// Weight α for non-partitioned updates.
    pub normal_weight: f64,

    /// Maximum simulation steps.
    max_steps: usize,

    /// Consensus threshold for early termination.
    consensus_threshold: f64,
}

impl<SM, IM, CP> Network<SM, IM, CP>
where
    SM: StateMachine<State = f64, Input = f64> + Clone + Send + Sync + 'static,
    IM: IncentiveMechanism<Reward = f64> + Clone + Send + Sync + 'static,
    CP: ConsensusProtocol<State = f64> + Clone + Send + Sync + 'static,
{
    /// Creates a new network with the specified configuration.
    ///
    /// # Arguments
    /// * `num_nodes` - Number of nodes.
    /// * `state_machine` - State machine instance.
    /// * `incentive_mechanism` - Incentive mechanism instance.
    /// * `consensus_protocol` - Consensus protocol instance.
    /// * `normal_weight` - Weight α for non-partitioned updates (0 ≤ α ≤ 1).
    /// * `max_steps` - Maximum simulation steps.
    /// * `consensus_threshold` - Threshold for consensus detection.
    pub fn new(
        num_nodes: usize,
        state_machine: SM,
        incentive_mechanism: IM,
        consensus_protocol: CP,
        normal_weight: f64,
        max_steps: usize,
        consensus_threshold: f64,
    ) -> Self {
        let mut nodes = Vec::new();
        let mut senders = Vec::new();

        for id in 0..num_nodes {
            let (node, sender) = Node::new(
                id,
                state_machine.clone(),
                incentive_mechanism.clone(),
                consensus_protocol.clone(),
            );
            nodes.push(Arc::new(RwLock::new(node)));
            senders.push(sender);
        }

        let partitions = Arc::new(RwLock::new(vec![(0..num_nodes).collect()])); // Initially fully connected

        Self {
            nodes,
            senders,
            partitions,
            normal_weight,
            max_steps,
            consensus_threshold,
        }
    }

    /// Checks if a node is in a partition (not fully connected).
    pub async fn is_partitioned(&self, node_id: usize) -> bool {
        let partitions = self.partitions.read().await;
        let group = partitions.iter().find(|g| g.contains(&node_id)).unwrap();
        group.len() < self.nodes.len()
    }

    /// Gets the states of peers in the same partition group.
    pub async fn get_peer_states(&self, node_id: usize) -> Vec<f64> {
        let partitions = self.partitions.read().await;
        let group = partitions.iter().find(|g| g.contains(&node_id)).unwrap();

        let mut states = Vec::new();
        for &id in group.iter().filter(|&&id| id != node_id) {
            if let Some(node) = self.nodes.get(id) {
                let node_guard = node.read().await;
                states.push(node_guard.get_state());
            }
        }
        states
    }

    /// Gets all node states for consensus checking.
    pub async fn get_all_states(&self) -> Vec<f64> {
        let mut states = Vec::new();
        for node in &self.nodes {
            let node_guard = node.read().await;
            states.push(node_guard.get_state());
        }
        states
    }

    /// Gets all node rewards for analysis.
    pub async fn get_all_rewards(&self) -> Vec<f64> {
        let mut rewards = Vec::new();
        for node in &self.nodes {
            let node_guard = node.read().await;
            rewards.push(node_guard.get_reward());
        }
        rewards
    }

    /// Updates partition configuration.
    async fn update_partitions(&self, step: usize) {
        let mut partitions = self.partitions.write().await;
        *partitions = if step % 10 == 0 {
            // Split into two random groups
            let mut indices: Vec<usize> = (0..self.nodes.len()).collect();
            indices.shuffle(&mut rand::thread_rng());
            let split = self.nodes.len() / 2;
            vec![indices[..split].to_vec(), indices[split..].to_vec()]
        } else {
            // Fully connected
            vec![(0..self.nodes.len()).collect()]
        };
    }

    /// Checks if consensus has been reached.
    async fn check_consensus(&self) -> bool {
        let states = self.get_all_states().await;
        if states.is_empty() {
            return false;
        }

        // Check if the first node's consensus protocol agrees
        let node = self.nodes[0].read().await;
        node.consensus_protocol.is_consensus(&states)
    }

    /// Runs a single simulation step.
    async fn simulate_step(&self, step: usize) -> Result<(), AxiomError> {
        // Update partitions every 5 steps
        if step % 5 == 0 {
            self.update_partitions(step).await;
        }

        // Send random actions to some nodes to simulate activity
        let num_active_nodes = (self.nodes.len() / 2).max(1);
        let mut indices: Vec<usize> = (0..self.nodes.len()).collect();
        indices.shuffle(&mut rand::thread_rng());

        for &node_id in indices.iter().take(num_active_nodes) {
            if let Some(sender) = self.senders.get(node_id) {
                let state = match self.nodes.get(node_id) {
                    Some(node) => node.read().await.get_state(),
                    None => 0.0,
                };
                let action = if rand::thread_rng().gen_bool(0.7) {
                    Action::SendMessage(Message {
                        sender_id: node_id,
                        state,
                    })
                } else {
                    Action::UpdateState
                };

                if let Err(e) = sender.try_send(action) {
                    match e {
                        TrySendError::Full(_) => {
                            // Channel full, skip this action
                            continue;
                        }
                        TrySendError::Closed(_) => {
                            return Err(AxiomError::NetworkSend("Channel closed".to_string()));
                        }
                    }
                }
            }
        }

        // Give nodes time to process
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok(())
    }

    /// Simulates the network, running nodes and updating partitions.
    pub async fn simulate(&self) -> Result<(), AxiomError> {
        let (_shutdown_tx, _shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let mut node_handles = Vec::new();

        // Start all nodes
        for node in &self.nodes {
            let network = Arc::new(self.clone());
            let node_clone = Arc::clone(node);
            let (node_shutdown_tx, mut node_shutdown_rx) = tokio::sync::oneshot::channel::<()>();

            let handle = tokio::spawn(async move {
                let mut node = node_clone.write().await;
                node.run(network, &mut node_shutdown_rx).await
            });

            node_handles.push((handle, node_shutdown_tx));
        }

        // Run simulation steps
        for step in 0..self.max_steps {
            self.simulate_step(step).await?;

            // Check for consensus
            if self.check_consensus().await {
                let states = self.get_all_states().await;
                println!("Consensus reached at step {}: {:?}", step, states);
                break;
            }

            // Optional: Add progress reporting
            if step % 100 == 0 {
                let states = self.get_all_states().await;
                println!("Step {}: States = {:?}", step, states);
            }
        }

        // Shutdown all nodes
        for (handle, shutdown_tx) in node_handles {
            let _ = shutdown_tx.send(());
            match timeout(Duration::from_secs(5), handle).await {
                Ok(Ok(Ok(()))) => {} // Normal shutdown
                Ok(Ok(Err(e))) => return Err(e),
                Ok(Err(_)) => return Err(AxiomError::NetworkSend("Task panicked".to_string())),
                Err(_) => return Err(AxiomError::Timeout(5)),
            }
        }

        if !self.check_consensus().await {
            Err(AxiomError::Timeout(self.max_steps))
        } else {
            Ok(())
        }
    }
}

// Improved Clone implementation
impl<SM, IM, CP> Clone for Network<SM, IM, CP>
where
    SM: StateMachine + Clone,
    IM: IncentiveMechanism + Clone,
    CP: ConsensusProtocol + Clone,
{
    fn clone(&self) -> Self {
        Self {
            nodes: self.nodes.clone(),
            senders: self.senders.clone(),
            partitions: Arc::clone(&self.partitions),
            normal_weight: self.normal_weight,
            max_steps: self.max_steps,
            consensus_threshold: self.consensus_threshold,
        }
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_network_creation() {
        // Mock implementations would go here
        // This is a placeholder to show testing structure
    }

    #[tokio::test]
    async fn test_partition_detection() {
        // Test partition logic
    }

    #[tokio::test]
    async fn test_consensus_detection() {
        // Test consensus detection
    }
}
