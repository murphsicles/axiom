use crate::{Action, AxiomError, Message};

/// Trait for partition-aware state machines in the Axiom framework.
///
/// Implementors define how nodes transition between states based on inputs and partition status,
/// producing actions like sending messages or updating local state.
pub trait StateMachine: Send + Sync + Clone + 'static {
    /// Type of input to the state machine (e.g., proposed state).
    type Input;

    /// Type of state maintained by the node.
    type State;

    /// Transitions the state based on input and partition status.
    ///
    /// Returns the new state and a list of actions to perform.
    fn transition(
        &self,
        state: &Self::State,
        input: Self::Input,
        is_partitioned: bool,
    ) -> Result<(Self::State, Vec<Action>), AxiomError>;
}

/// Default state machine implementing the paper's iterative update rule.
///
/// Uses a numeric state updated via \( s_i^{(t)} = s_i^{(t-1)} \cdot (1 - w) + s_{\text{target}} \cdot w \),
/// where \( w \) is \( p \) (partitioned) or \( \alpha \) (normal, provided externally).
#[derive(Debug, Clone)]
pub struct AxiomStateMachine {
    /// Partition weight \( p \) for state updates during partitions (0 ≤ p ≤ 1).
    partition_weight: f64,
}

impl AxiomStateMachine {
    /// Creates a new state machine with the given partition weight.
    ///
    /// # Arguments
    /// * `partition_weight` - Weight \( p \) for partitioned updates (0 ≤ p ≤ 1).
    ///
    /// # Errors
    /// Returns an error if `partition_weight` is not in [0, 1].
    pub fn new(partition_weight: f64) -> Result<Self, AxiomError> {
        if !(0.0..=1.0).contains(&partition_weight) {
            return Err(AxiomError::InvalidTransition(
                "partition weight must be in [0, 1]".into(),
            ));
        }
        Ok(Self { partition_weight })
    }
}

impl StateMachine for AxiomStateMachine {
    type Input = f64; // Target state from peer averaging
    type State = f64; // Numeric state

    fn transition(
        &self,
        state: &Self::State,
        input: Self::Input,
        is_partitioned: bool,
    ) -> Result<(Self::State, Vec<Action>), AxiomError> {
        // Weight w: partition_weight (p) if partitioned, else alpha (provided externally)
        let w = if is_partitioned {
            self.partition_weight
        } else {
            // Alpha is passed via input context (handled by Network)
            input
        };

        // Update: s_i^{(t)} = s_i^{(t-1)} * (1 - w) + s_target * w
        let new_state = state * (1.0 - w) + input * w;

        // Action: Broadcast new state
        let actions = vec![Action::SendMessage(Message {
            sender_id: 0, // Set by Node
            state: new_state,
        })];

        Ok((new_state, actions))
    }
}
