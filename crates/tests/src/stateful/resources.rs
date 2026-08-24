use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};

use thiserror::Error;
use uuid::Uuid;

use super::Handle;

/// Maps symbolic handles to real IDs observed in the system under test.
#[derive(Debug, Clone, Default)]
pub struct ResourceMap {
    /// Real IDs indexed by symbolic handles.
    ids: BTreeMap<Handle, Uuid>,
    /// Handles currently considered live by the concrete test.
    live: BTreeSet<Handle>,
}

impl ResourceMap {
    /// Associates `handle` with `id` and marks it live.
    ///
    /// # Errors
    ///
    /// Returns [`OperationError::HandleAlreadyBound`] if `handle` already has an ID.
    pub fn bind(&mut self, handle: Handle, id: Uuid) -> Result<(), OperationError> {
        match self.ids.entry(handle) {
            Entry::Vacant(entry) => {
                entry.insert(id);
                self.live.insert(handle);
                Ok(())
            }
            Entry::Occupied(_) => Err(OperationError::HandleAlreadyBound(handle)),
        }
    }

    /// Resolves a symbolic handle to a real ID.
    ///
    /// # Errors
    ///
    /// Returns [`OperationError::UnknownHandle`] if `handle` has not been bound.
    pub fn resolve(&self, handle: Handle) -> Result<Uuid, OperationError> {
        self.ids.get(&handle).copied().ok_or(OperationError::UnknownHandle(handle))
    }

    /// Marks a handle as archived.
    ///
    /// # Errors
    ///
    /// Returns [`OperationError::UnknownOrRetiredHandle`] if `handle` is not live.
    pub fn archive(&mut self, handle: Handle) -> Result<(), OperationError> {
        if !self.live.remove(&handle) {
            return Err(OperationError::UnknownOrRetiredHandle(handle));
        }
        Ok(())
    }

    /// Marks a bound handle as active again.
    ///
    /// # Errors
    ///
    /// Returns [`OperationError::UnknownHandle`] if `handle` has not been bound.
    pub fn unarchive(&mut self, handle: Handle) -> Result<(), OperationError> {
        if !self.ids.contains_key(&handle) {
            return Err(OperationError::UnknownHandle(handle));
        }
        self.live.insert(handle);
        Ok(())
    }
}

/// Failure produced while resolving or executing a modeled operation.
#[derive(Debug, Error)]
pub enum OperationError {
    /// A symbolic handle was bound more than once.
    #[error("symbolic handle is already bound: {0}")]
    HandleAlreadyBound(Handle),
    /// A symbolic handle has never been bound.
    #[error("unknown symbolic handle: {0}")]
    UnknownHandle(Handle),
    /// A symbolic handle is unknown or no longer live.
    #[error("unknown or retired symbolic handle: {0}")]
    UnknownOrRetiredHandle(Handle),
    /// A raw HTTP request failed.
    #[error("HTTP operation failed: {0}")]
    Http(String),
    /// An SDK request failed.
    #[error("SDK operation failed: {0}")]
    Sdk(String),
    /// Observed state violated a model invariant.
    #[error("model invariant failed: {0}")]
    Invariant(String),
    /// A response did not match the operation's expected shape or status.
    #[error("unexpected response: {0}")]
    UnexpectedResponse(String),
}
