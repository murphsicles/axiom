use crate::{state_machine::StateMachine, AxiomError};

/// Trait for consensus protocols in the Axiom framework.
///
/// Implementors define how nodes propose states and check for global convergence
/// using an iterative approach without supermajority thresholds.
pub trait ConsensusProtocol: Send + Sync + Clone + 'static {
    /// Type of state used by the state machine.
    type State;

    /// Proposes a target state based on peer states.
    fn propose(&self, peer_states: &[Self::State]) -> Self::State;

    /// Checks if global consensus is achieved.
    fn is_consensus(&self, states: &[Self::State]) -> bool;
}

/// Default consensus protocol implementing the paper's iterative convergence.
///
/// Proposes target states via averaging and checks convergence when
/// \( \max(s_i^{(t)}) - \min(s_i^{(t)}) < \epsilon \).
#[derive(Debug, Clone)]
pub struct AxiomConsensus {
    /// Convergence threshold \( \epsilon \).
    epsilon: f64,
}

impl AxiomConsensus {
    /// Creates a new consensus protocol with the given threshold.
    ///
    /// # Arguments
    /// * `epsilon` - Convergence threshold \( \epsilon \) (must be positive).
    ///
    /// # Errors
    /// Returns an error if `epsilon` is not positive.
    pub fn new(epsilon: f64) -> Result<Self, AxiomError> {
        if epsilon <= 0.0 {
            return Err(AxiomError::InvalidTransition(
                "epsilon must be positive".into(),
            ));
        }
        Ok(Self { epsilon })
    }
}

impl ConsensusProtocol for AxiomConsensus {
    type State = f64;

    fn propose(&self, peer_states: &[Self::State]) -> Self::State {
        if peer_states.is_empty() {
            return 0.0; // Default if no peers
        }
        // Propose average: s_target = (1/|G|) * sum(s_j)
        peer_states.iter().sum::<f64>() / peer_states.len() as f64
    }

    fn is_consensus(&self, states: &[Self::State]) -> bool {
        if states.is_empty() {
            return true;
        }
        // Check: max(s_i) - min(s_i) < epsilon
        let max_state = states.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
        let min_state = states.iter().fold(f64::INFINITY, |a, &b| a.min(b));
        (max_state - min_state) < self.epsilon
    }
}
