# Axiom 🛠️

![Rust Version](https://img.shields.io/badge/Rust-1.81+-orange?logo=rust)
![Dependencies](https://img.shields.io/badge/dependencies-tokio%2C%20serde%2C%20rand%2C%20thiserror-blue)
![CI Tests](https://github.com/your-org/axiom/workflows/CI/badge.svg)

A Rust library implementing the framework from ["Resolving CAP Through Automata-Theoretic Economic Design"](https://arxiv.org/abs/2507.02464) by Dr. Craig S. Wright. Axiom provides a partition-tolerant distributed system with iterative state updates, economic incentives, and provable convergence, designed for real-time applications.

## Features 🚀
- **Partition-Aware State Machines**: Nodes adapt states based on network conditions, using iterative updates instead of supermajority thresholds.
- **Economic Incentives**: Rewards align nodes with local consensus, stabilizing the system.
- **Iterative Consensus**: Converges when states are within a small range (ε), ensuring robustness.
- **Async Networking**: Built with Tokio for high-performance simulations.

## Usage 💻
```rust
use axiom::{AxiomConsensus, AxiomIncentive, AxiomStateMachine, Network};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let state_machine = AxiomStateMachine::new(0.1);
    let incentive = AxiomIncentive::new(1.0, 0.1);
    let consensus = AxiomConsensus::new(0.01);
    let mut network = Network::new(5, state_machine, incentive, consensus, 0.9, 20);
    network.simulate().await?;
    Ok(())
}
```

## Mathematical Basis 📐
- **State Update**: \( s_i^{(t)} = s_i^{(t-1)} \cdot (1 - w) + s_{\text{target}}^{(i)} \cdot w \), where \( w \) is \( \alpha \) (normal) or \( p \) (partitioned).
- **Incentive**: \( u_i^{(t)} = r - c \cdot |s_i^{(t)} - \bar{s}_{G_t^{(i)}}^{(t)}| \).
- **Convergence**: Achieved when \( \max(s_i^{(t)}) - \min(s_i^{(t)}) < \epsilon \).

## License 📜
MIT
