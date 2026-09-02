use serde::{Deserialize, Serialize};

use crate::actors::Actor;

/// Kind of resource represented by a symbolic handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    /// Workspace resource.
    Workspace,
    /// Object resource.
    Object,
    /// Edge between objects.
    Edge,
    /// Binary attachment owned by an object.
    Attachment,
    /// Authorization grant.
    Grant,
    /// User group.
    Group,
    /// User membership in a group.
    Membership,
    /// API key.
    ApiKey,
    /// Authenticated session.
    Session,
    /// Commentary thread.
    CommentThread,
    /// Comment within a commentary thread.
    Comment,
}

impl std::fmt::Display for ResourceKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Workspace => "workspace",
            Self::Object => "object",
            Self::Edge => "edge",
            Self::Attachment => "attachment",
            Self::Grant => "grant",
            Self::Group => "group",
            Self::Membership => "membership",
            Self::ApiKey => "api_key",
            Self::Session => "session",
            Self::CommentThread => "comment_thread",
            Self::Comment => "comment",
        })
    }
}

/// Principal receiving a modeled object grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Principal {
    /// A direct user principal.
    User(Actor),
    /// A group principal.
    Group(Handle),
}

/// Stable symbolic reference to a resource created during a test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Handle {
    /// Kind of resource referenced by the handle.
    pub kind: ResourceKind,
    /// Per-kind allocation index.
    pub index: u32,
}

impl Handle {
    /// Creates a symbolic handle from its resource kind and allocation index.
    #[must_use]
    pub const fn new(kind: ResourceKind, index: u32) -> Self {
        Self { kind, index }
    }
}

impl std::fmt::Display for Handle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}#{}", self.kind, self.index)
    }
}
