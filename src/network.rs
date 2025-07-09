use crate::{
    consensus::ConsensusProtocol,
    error::AxiomError,
    incentive::IncentiveMechanism,
    state_machine::StateMachine,
    Action, Message,
};
use rand::seq::SliceRandom;
use std::sync::Arc;
use tokio::sync::mpsc::{self, Receiver, Sender};

/// A node in the Axiom network, integrating state machine, incentives, and consensus.
#[derive(Clone)]
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

    /// Runs the node's main loop, processing messages and updating state.
    ///
    /// # Arguments
    /// * `network` - Reference to the network for partition and peer state access.
    /// * `normal_weight` - Weight \( \alpha \) for non-partitioned updates.
    async fn run(&mut self, network: &Arc<Network<SM, IM, CP>>, normal_weight: f64) -> Result<(), AxiomError> {
        while let Some(action) = self.receiver.recv().await {
            match action {
                Action::SendMessage(msg) => {
                    // Process peer state
                    let is_partitioned = network.is_partitioned(self.id);
                    let peer_states = network.get_peer_states(self.id);
                    let target_state = self.consensus_protocol.propose(&peer_states);

                    // Transition state with w = alpha (normal) or p (partitioned)
                    let input = if is_partitioned { target_state } else { normal_weight };
                    let (new_state, actions) = self.state_machine.transition(
                        &self.state,
                        input,
                        is_partitioned,
                    )?;

                    // Update state and reward
                    self.state = new_state;
                    self.reward += self.incentive_mechanism.calculate_reward(&self.state, &peer_states)?;

                    // Broadcast actions
                    for action in actions {
                        let _ = self.sender.send(action).await;
                    }
                }
                Action::UpdateState => {
                    // Local update without broadcasting
                    let peer_states = network.get_peer_states(self.id);
                    self.reward += self.incentive_mechanism.calculate_reward(&self.state, &peer_states)?;
                }
            }
        }
        Ok(())
    }
}

/// Network managing a collection of nodes and simulating partitions.
pub struct Network<SM: StateMachine, IM: IncentiveMechanism, CP: ConsensusProtocol> {
    /// List of nodes.
    nodes: Vec<Node<SM, IM, CP>>,

    /// Senders for each node.
    senders: Vec<Sender<Action>>,

    /// Current partition configuration (node ID -> group).
    partitions: Vec<Vec<usize>>,

    /// Weight \( \alpha \) for non-partitioned updates.
    normal_weight: f64,

    /// Maximum simulation steps.
    max_steps: usize,
}

impl<SM, IM, CP> Network<SM, IM, CP>
where
    SM: StateMachine<State = f64, Input = f64>,
    IM: IncentiveMechanism<Reward = f64>,
    CP: ConsensusProtocol<State = f64>,
{
    /// Creates a new network with the specified configuration.
    ///
    /// # Arguments
    /// * `num_nodes` - Number of nodes.
    /// * `state_machine` - State machine instance.
    /// * `incentive_mechanism` - Incentive mechanism instance.
    /// * `consensus_protocol` - Consensus protocol instance.
    /// * `normal_weight` - Weight \( \alpha \) for non-partitioned updates (0 ≤ α ≤ 1).
    /// * `max_steps` - Maximum simulation steps.
    pub fn new(
        num_nodes: usize,
        state_machine: SM,
        incentive_mechanism: IM,
        consensus_protocol: CP,
        normal_weight: f64,
        max_steps: usize,
    ) -> Self {
        let mut nodes = Vec::new();
        let mut senders = Vec::new();
        for id in 0..num_nodes {
            let (node, sender) = Node::new(id, state_machine.clone(), incentive_mechanism.clone(), consensus_protocol.clone());
            nodes.push(node);
            senders.push(sender);
        }
        let partitions = vec![(0..num_nodes).collect()]; // Initially fully connected
        Self {
            nodes,
            senders,
            partitions,
            normal_weight,
            max_steps,
        }
    }

    /// Checks if a node is in a partition (not fully connected).
    pub fn is_partitioned(&self, node_id: usize) -> bool {
        let group = self.partitions.iter().find(|g| g.contains(&node_id)).unwrap();
        group.len() < self.nodes.len()
    }

    /// Gets the states of peers in the same partition group.
    pub fn get_peer_states(&self, node_id: usize) -> Vec<f64> {
        let group = self.partitions.iter().find(|g| g.contains(&node_id)).unwrap();
        group.iter().filter(|&&id| id != node_id).map(|&id| self.nodes[id].state).collect()
    }

    /// Simulates the network, running nodes and updating partitions.
    pub async fn simulate(&mut self) -> Result<(), AxiomError> {
        let network = Arc::new(self); // Share read-only network state
        for step in 0..self.max_steps {
            // Update partitions every 5 steps
            if step % 5 == 0 {
                network.partitions = if step % 10 == 0 {
                    // Split into two random groups
                    let mut indices: Vec<usize> = (0..network.nodes.len()).collect();
                    indices.shuffle(&mut rand::thread_rng());
                    let split = network.nodes.len() / 2;
                    vec![indices[..split].to_vec(), indices[split..].to_vec()]
                } else {
                    // Fully connected
                    vec![(0..network.nodes.len()).collect()]
                };
            }

            // Run nodes concurrently
            let mut handles = Vec::new();
            for node in &mut network.nodes {
                let network_clone = Arc::clone(&network);
                handles.push(tokio::spawn({
                    let mut node = node.clone();
                    async move { node.run(&network_clone, network_clone.normal_weight).await }
                }));
            }
            for handle in handles {
                handle.await??; // Propagate JoinError as AxiomError
            }

            // Check for consensus
            let states: Vec<f64> = network.nodes.iter().map(|n| n.state).collect();
            if network.nodes[0].consensus_protocol.is_consensus(&states) {
                println!("Consensus reached at step {}: {:?}", step, states);
                return Ok(());
            }
        }
        Err(AxiomError::Timeout(self.max_steps))
    }
}
