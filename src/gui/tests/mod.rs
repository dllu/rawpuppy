//! Native widget, worker and document-history integration tests.
use super::*;

mod autosave;
mod brushes;
mod calibration;
mod curves;
#[cfg(debug_assertions)]
mod framing;
#[cfg(feature = "moebius")]
mod generation;
mod history;
mod perspective;
#[cfg(debug_assertions)]
mod viewport;
mod worker;
