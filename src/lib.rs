//! Axiom: A Rust library implementing a partition-tolerant distributed system with iterative
//! consensus and economic incentives, based on Dr. Craig S. Wright's paper, "Resolving CAP Through
//! Automata-Theoretic Economic Design." The library provides a modular framework for real-time,
//! partition-tolerant systems, using state machines, game-theoretic incentives, and iterative state
//! updates to achieve provable convergence without supermajority thresholds.
//!
//! # Features
//! - Partition-aware state machines with adaptive transitions.
//! - Economic incentives to stabilize consensus.
//! - Iterative consensus via weighted state averaging.
//! - Async network simulation with Tokio for high performance.
//!
//! # Example
//! ```
//! use axiom::{Network, AxiomStateMachine, AxiomIncentive, AxiomConsensus, AxiomError};
//!
//! # async fn example() -> Result<(), AxiomError> {
//! let state_machine = AxiomStateMachine::new(0.1).unwrap();
//! let incentive = AxiomIncentive::new(1.0, 0.1).unwrap();
//! let consensus = AxiomConsensus::new(0.01).unwrap();
//! let mut network = Network::new(5, state_machine, incentive, consensus, 0.9, 20, 0.01);
//! network.simulate().await?;
//! # Ok(())
//! # }
//! ```

mod consensus;
mod error;
mod incentive;
mod network;
mod state_machine;
mod types;

pub use consensus::{AxiomConsensus, ConsensusProtocol};
pub use error::AxiomError;
pub use incentive::{AxiomIncentive, IncentiveMechanism};
pub use network::Network;
pub use state_machine::{AxiomStateMachine, StateMachine};
pub use types::{Action, Message};

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_state_machine_transition() {
        let state_machine = AxiomStateMachine::new(0.1).unwrap();
        let initial_state = 0.5;
        let input = 1.0;

        // Normal transition (w = input)
        let (new_state, actions) = state_machine
            .transition(&initial_state, input, false)
            .unwrap();
        assert!((new_state - input).abs() < 0.01, "Normal transition failed");
        assert_eq!(actions.len(), 1, "Expected one action");

        // Partitioned transition (w = 0.1)
        let (new_state, actions) = state_machine
            .transition(&initial_state, input, true)
            .unwrap();
        let expected = initial_state * 0.9 + input * 0.1; // s * (1 - p) + target * p
        assert!(
            (new_state - expected).abs() < 0.01,
            "Partitioned transition failed"
        );
        assert_eq!(actions.len(), 1, "Expected one action");
    }

    #[tokio::test]
    async fn test_incentive_calculation() {
        let incentive = AxiomIncentive::new(1.0, 0.1).unwrap();
        let state = 0.5;
        let peer_states = vec![0.5, 0.6, 0.4];
        let reward = incentive
            .calculate_reward::<AxiomStateMachine>(&state, &peer_states)
            .unwrap();
        let avg_state = peer_states.iter().sum::<f64>() / peer_states.len() as f64;
        let expected = 1.0 - 0.1 * (state - avg_state).abs();
        assert!(
            (reward - expected).abs() < 0.01,
            "Reward calculation failed"
        );
    }
}
