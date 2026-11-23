//! Everything except the window: model, registry access, staging, apply,
//! snapshots, diagnostics and the elevated helper. No UI imports here.

pub mod apply;
pub mod broadcast;
pub mod diagnostics;
pub mod elevation;
pub mod expand;
pub mod model;
pub mod records;
pub mod registry;
pub mod snapshot;
pub mod staging;
