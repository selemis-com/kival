//! Reusable reference model and symbolic resources for stateful Kival tests.

/// Proptest reference state machine and generated operations.
mod machine;
/// Abstract state used to generate valid transitions.
mod model;
/// Symbolic-to-real resource mappings for concrete test drivers.
mod resources;
/// Shared symbolic identifiers used by the model and concrete drivers.
mod types;

pub use machine::{KivalStateMachine, Operation};
pub use model::{
    Lifecycle, Model, ModeledApiKey, ModeledAttachment, ModeledComment, ModeledCommentThread,
    ModeledEvent,
};
pub use resources::{OperationError, ResourceMap};
pub use types::{Handle, Principal, ResourceKind};

#[cfg(test)]
mod tests;
