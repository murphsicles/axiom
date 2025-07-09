use serde::{Deserialize, Serialize};

/// Actions a node can take after a state transition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Action {
    /// Send a message to other nodes.
    SendMessage(Message),

    /// Update local state without communication.
    UpdateState,
}

/// Messages exchanged between nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// ID of the sending node.
    pub sender_id: usize,

    /// Node's current state.
    pub state: f64,
}
