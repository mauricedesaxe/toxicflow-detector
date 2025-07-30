//! Sandwich attack detection algorithms.
//!
//! This module contains different approaches for detecting sandwich attacks:
//! - `same_block`: Detects sandwich attacks within the same block (most common case)
//! - Future: cross-block detection for more sophisticated MEV strategies

pub mod same_block;

// Re-export main types and functions for convenience
pub use same_block::{find_sandwiches, SandwichAttack, SwapTransaction};
