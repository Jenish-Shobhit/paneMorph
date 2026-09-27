//! paneMorph: send any herdr pane anywhere, fetch any pane here.
//!
//! The library behind the `panemorph` binary, a herdr plugin that moves live
//! panes between tabs and spaces with herdr's `pane.move`, so no terminal is
//! ever closed or restarted.
//!
//! - [`actions`]: the commands herdr runs for keys and popups.
//! - [`api`] and [`model`]: the herdr socket client and its typed snapshot.
//! - [`plan`] and [`topology`]: pure decisions about what a move does.
//! - [`exec`] and [`journal`]: carrying out moves, rollback and undo.
//! - [`names`]: how panes, tabs and spaces are named.
//! - [`state`]: invocation context, the key queue lock and logging.
//! - [`sim`]: an in-memory herdr for tests and `panemorph preview`.
//! - [`ui`]: the Send and Fetch windows and the notice popup.

pub mod actions;
pub mod api;
pub mod exec;
pub mod journal;
pub mod model;
pub mod names;
pub mod plan;
pub mod sim;
pub mod state;
pub mod topology;
pub mod ui;

#[cfg(test)]
mod exec_tests;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
