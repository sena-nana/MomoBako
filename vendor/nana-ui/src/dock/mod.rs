//! Host interaction adapter for the caller-owned Runtime [`crate::DockWorkspace`].
//! Geometry and tree mutation belong to Runtime. This module stores only pointer,
//! time and display environment. Floating effects use `runtime_dock_window_update`.
mod interaction;
pub use interaction::{DockController, DockDropTarget};
