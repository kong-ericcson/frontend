pub use bluetext_model_core::*;
pub use bluetext_model_macros::*;

/// Re-export the core crate so proc macros can reference `bluetext_model_core::`
/// paths even when users only depend on `bluetext-model`.
#[doc(hidden)]
pub use bluetext_model_core;

/// Re-export tokio so command code can use `tokio::try_join!` etc.
/// without adding tokio as a direct dependency.
#[doc(hidden)]
pub use tokio;

/// Annotate the next conditional in a command with a label for the flowchart diagram.
/// Expands to a no-op that the `#[commands]` proc macro can detect and strip.
#[macro_export]
macro_rules! label {
    ($label:expr) => {
        let _ = $label;
    };
}

pub mod prelude {
    pub use bluetext_model_core::metadata::*;
    pub use bluetext_model_core::resonate::{CommandError, RequestError};
    pub use bluetext_model_core::simulation::*;
    pub use bluetext_model_core::stores::*;
    pub use bluetext_model_core::workflow::*;
    pub use bluetext_model_macros::{ModelType, StateMachine, model_alias, state_machine_impl, state_machine_actions, commands};
    pub use serde::{Serialize, Deserialize};
    pub use std::collections::HashMap;
    pub use crate::label;
}
