use thiserror::Error;

/// Errors that can occur in the Axiom library.
#[derive(Error, Debug)]
pub enum AxiomError {
    /// Failed to send a message over the network.
    #[error("network send error: {0}")]
    NetworkSend(String),

    /// Invalid state transition attempted.
    #[error("invalid state transition: {0}")]
    InvalidTransition(String),

    /// Simulation reached maximum steps without convergence.
    #[error("simulation timed out after {0} steps")]
    Timeout(usize),

    /// Task execution failed.
    #[error("task execution failed: {0}")]
    TaskJoin(#[from] tokio::task::JoinError),
}
