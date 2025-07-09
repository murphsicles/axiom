use crate::{state_machine::StateMachine, AxiomError};

/// Trait for economic incentive mechanisms in the Axiom framework.
///
/// Implementors define how nodes are rewarded or penalized based on their actions and states,
/// encouraging alignment with local or global consensus.
pub trait IncentiveMechanism: Send + Sync + Clone + 'static {
    /// Type of reward (e.g., numeric utility).
    type Reward;

    /// Calculates the reward for a node based on its state and peers' states.
    fn calculate_reward<SM: StateMachine>(
        &self,
        state: &SM::State,
        peer_states: &[SM::State],
    ) -> Result<Self::Reward, AxiomError>;
}

/// Default incentive mechanism implementing the paper's utility function.
///
/// Uses \( u_i^{(t)} = r - c \cdot |s_i^{(t)} - \bar{s}_{G_t^{(i)}}^{(t)}| \),
/// rewarding nodes for aligning with their local group's average state.
#[derive(Debug, Clone)]
pub struct AxiomIncentive {
    /// Base reward \( r \).
    reward_base: f64,

    /// Penalty coefficient \( c \).
    penalty_coeff: f64,
}

impl AxiomIncentive {
    /// Creates a new incentive mechanism with the given parameters.
    ///
    /// # Arguments
    /// * `reward_base` - Base reward \( r \) (must be non-negative).
    /// * `penalty_coeff` - Penalty coefficient \( c \) (must be non-negative).
    ///
    /// # Errors
    /// Returns an error if parameters are invalid.
    pub fn new(reward_base: f64, penalty_coeff: f64) -> Result<Self, AxiomError> {
        if reward_base < 0.0 || penalty_coeff < 0.0 {
            return Err(AxiomError::InvalidTransition(
                "reward_base and penalty_coeff must be non-negative".into(),
            ));
        }
        Ok(Self {
            reward_base,
            penalty_coeff,
        })
    }
}

impl IncentiveMechanism for AxiomIncentive {
    type Reward = f64;

    fn calculate_reward<SM: StateMachine>(
        &self,
        state: &SM::State,
        peer_states: &[SM::State],
    ) -> Result<Self::Reward, AxiomError> {
        let state = *state as f64; // Assuming SM::State is f64
        let peer_states: Vec<f64> = peer_states.iter().map(|&s| s as f64).collect();
        if peer_states.is_empty() {
            return Ok(self.reward_base); // No peers, no penalty
        }

        // Calculate group average: \bar{s}_{G_t^{(i)}}^{(t)}
        let avg_state = peer_states.iter().sum::<f64>() / peer_states.len() as f64;

        // Utility: u_i^{(t)} = r - c * |s_i^{(t)} - \bar{s}_{G_t^{(i)}}^{(t)}|
        let deviation = (state - avg_state).abs();
        Ok(self.reward_base - self.penalty_coeff * deviation)
    }
}
