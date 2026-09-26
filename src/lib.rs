//! paneMorph: send any herdr pane anywhere, fetch any pane here.

pub mod api;
pub mod exec;
pub mod journal;
pub mod model;
pub mod names;
pub mod plan;
pub mod sim;
pub mod state;
pub mod topology;

#[cfg(test)]
mod exec_tests;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
