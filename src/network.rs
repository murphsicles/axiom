use crate::{
    consensus::ConsensusProtocol,
    error::AxiomError,
    incentive::IncentiveMechanism,
    state_machine::StateMachine,
    Action,
};
use rand::{seq::SliceRandom, Rng};
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc::{self, Receiver, Sender, error::TryRecvError};
use tokio::time::{Duration, timeout};

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
                let is_partitioned = network.is_partitioned(self.id);
                let peer_states = network.get_peer_states(self.id);
                let target_state = self.consensus_protocol.propose(&peer_states);

                // Transition state with w = alpha (normal) or p (partitioned)
                let input = if is_partitioned {
                    target_state
                } else {
                    network.normal_weight
                };

                let (new_state, actions) = self.state_machine.transition(
                    &self.state,
                    input,
                    is_partitioned,
                )?;

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
                let peer_states = network.get_peer_states(self.id);
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
                _ = shutdown_rx => {
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
    pub fn is_partitioned(&self, node_id: usize) -> bool {
        let partitions = self.partitions.read().unwrap();
        let group = partitions.iter().find(|g| g.contains(&node_id)).unwrap();
        group.len() < self.nodes.len()
    }

    /// Gets the states of peers in the same partition group.
    pub fn get_peer_states(&self, node_id: usize) -> Vec<f64> {
        let partitions = self.partitions.read().unwrap();
        let group = partitions.iter().find(|g| g.contains(&node_id)).unwrap();

        group
            .iter()
            .filter(|&&id| id != node_id)
            .filter_map(|&id| self.nodes.get(id).and_then(|node| node.read().ok()).map(|node| node.get_state()))
            .collect()
    }

    /// Gets all node states for consensus checking.
    pub fn get_all_states(&self) -> Vec<f64> {
        self.nodes
            .iter()
            .filter_map(|node| node.read().ok().map(|n| n.get_state()))
            .collect()
    }

    /// Gets all node rewards for analysis.
    pub fn get_all_rewards(&self) -> Vec<f64> {
        self.nodes
            .iter()
            .filter_map(|node| node.read().ok().map(|n| n.get_reward()))
            .collect()
    }

    /// Updates partition configuration.
    fn update_partitions(&self, step: usize) {
        let mut partitions = self.partitions.write().unwrap();
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
    fn check_consensus(&self) -> bool {
        let states = self.get_all_states();
        if states.is_empty() {
            return false;
        }

        // Check if the first node's consensus protocol agrees
        if let Ok(node) = self.nodes[0].read() {
            node.consensus_protocol.is_consensus(&states)
        } else {
            false
        }
    }

    /// Runs a single simulation step.
    async fn simulate_step(&self, step: usize) -> Result<(), AxiomError> {
        // Update partitions every 5 steps
        if step % 5 == 0 {
            self.update_partitions(step);
        }

        // Send random actions to some nodes to simulate activity
        let num_active_nodes = (self.nodes.len() / 2).max(1);
        let mut indices: Vec<usize> = (0..self.nodes.len()).collect();
        indices.shuffle(&mut rand::thread_rng());

        for &node_id in indices.iter().take(num_active_nodes) {
            if let Some(sender) = self.senders.get(node_id) {
                let action = if rand::thread_rng().gen_bool(0.7) {
                    Action::SendMessage(format!("Step {}", step))
                } else {
                    Action::UpdateState
                };

                let _ = sender.try_send(action);
            }
        }

        // Give nodes time to process
        tokio::time::sleep(Duration::from_millis(10)).await;
        Ok(())
    }

    /// Simulates the network, running nodes and updating partitions.
    pub async fn simulate(&self) -> Result<(), AxiomError> {
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();
        let mut node_handles = Vec::new();

        // Start all nodes
        for node in &self.nodes {
            let network = Arc::new(self.clone());
            let node_clone = Arc::clone(node);
            let (node_shutdown_tx, node_shutdown_rx) = tokio::sync::oneshot::channel();

            let handle = tokio::spawn(async move {
                let mut node_shutdown_rx = node_shutdown_rx;
                if let Ok(mut node) = node_clone.write() {
                    node.run(network, &mut node_shutdown_rx).await
                } else {
                    Err(AxiomError::NetworkSend("Failed to acquire node lock".to_string()))
                }
            });

            node_handles.push((handle, node_shutdown_tx));
        }

        // Run simulation steps
        for step in 0..self.max_steps {
            self.simulate_step(step).await?;

            // Check for consensus
            if self.check_consensus() {
                let states = self.get_all_states();
                println!("Consensus reached at step {}: {:?}", step, states);
                break;
            }

            // Optional: Add progress reporting
            if step % 100 == 0 {
                let states = self.get_all_states();
                println!("Step {}: States = {:?}", step, states);
            }
        }

        // Shutdown all nodes
        for (handle, shutdown_tx) in node_handles {
            let _ = shutdown_tx.send(());
            match timeout(Duration::from_secs(5), handle).await {
                Ok(Ok(Ok(()))) => {}, // Normal shutdown
                Ok(Ok(Err(e))) => return Err(e),
                Ok(Err(_)) => return Err(AxiomError::NetworkSend("Task panicked".to_string())),
                Err(_) => return Err(AxiomError::Timeout(5)),
            }
        }

        if !self.check_consensus() {
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
    use super::*;
    // Add your test implementations here

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
