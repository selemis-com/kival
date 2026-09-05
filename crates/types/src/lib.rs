//! Canonical vocabulary shared across Kival's internal and public Rust interfaces.
//!
//! These types name concepts whose meaning is shared by the kernel and the HTTP
//! contract. Transport-specific request and response models do not belong here.

#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![cfg_attr(docsrs, feature(doc_cfg))]

use uuid::Uuid;

/// Defines the stable API-key scope vocabulary from one source list.
macro_rules! api_key_scopes {
    (
        $(
            $(#[$meta:meta])*
            $variant:ident => $wire_name:literal
        ),+ $(,)?
    ) => {
        /// Capability granted to an API key.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[cfg_attr(feature = "wire", derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema))]
        pub enum ApiKeyScope {
            $(
                $(#[$meta])*
                #[cfg_attr(feature = "wire", serde(rename = $wire_name))]
                $variant,
            )+
        }

        impl ApiKeyScope {
            /// All supported scopes in stable declaration order.
            pub const ALL: &[Self] = &[
                $(Self::$variant,)+
            ];

            /// Returns the stable stored and serialized scope name.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $wire_name,)+
                }
            }
        }

        impl std::str::FromStr for ApiKeyScope {
            type Err = ();

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($wire_name => Ok(Self::$variant),)+
                    _ => Err(()),
                }
            }
        }
    };
}

api_key_scopes! {
    /// Discover and inspect permitted workspaces.
    WorkspaceRead => "workspaces:read",
    /// Modify the metadata and lifecycle of permitted existing workspaces.
    /// Also satisfies `workspaces:read`.
    WorkspaceWrite => "workspaces:write",
    /// Read, list, and search objects.
    ObjectRead => "objects:read",
    /// Create, update, archive, and unarchive objects.
    /// Also satisfies `objects:read`.
    ObjectWrite => "objects:write",
    /// Read attachment metadata and content.
    AttachmentRead => "attachments:read",
    /// Upload and reuse attachments.
    /// Also satisfies `attachments:read`.
    AttachmentWrite => "attachments:write",
    /// Read graphs, backlinks, and edges.
    GraphRead => "graph:read",
    /// Create and revoke graph edges.
    /// Also satisfies `graph:read`.
    GraphWrite => "graph:write",
    /// Read workspace and object activity.
    EventRead => "events:read",
    /// Receive ephemeral realtime invalidations for resources the key may read.
    RealtimeRead => "realtime:read",
    /// Manage workspace memberships, workspace group links, and object grants.
    AccessManage => "access:manage",
    /// Access API-key-enabled global user, group, and event operations.
    /// The owning user's current authority remains the upper bound.
    Admin => "admin",
}

impl ApiKeyScope {
    /// Returns whether this granted scope satisfies a route requiring `required`.
    ///
    /// Write scopes include the corresponding read capability because mutations may inspect or
    /// return the current resource state. Other scopes remain exact.
    #[must_use]
    pub const fn permits(self, required: Self) -> bool {
        match required {
            Self::WorkspaceRead => matches!(self, Self::WorkspaceRead | Self::WorkspaceWrite),
            Self::WorkspaceWrite => matches!(self, Self::WorkspaceWrite),
            Self::ObjectRead => matches!(self, Self::ObjectRead | Self::ObjectWrite),
            Self::ObjectWrite => matches!(self, Self::ObjectWrite),
            Self::AttachmentRead => matches!(self, Self::AttachmentRead | Self::AttachmentWrite),
            Self::AttachmentWrite => matches!(self, Self::AttachmentWrite),
            Self::GraphRead => matches!(self, Self::GraphRead | Self::GraphWrite),
            Self::GraphWrite => matches!(self, Self::GraphWrite),
            Self::EventRead => matches!(self, Self::EventRead),
            Self::RealtimeRead => matches!(self, Self::RealtimeRead),
            Self::AccessManage => matches!(self, Self::AccessManage),
            Self::Admin => matches!(self, Self::Admin),
        }
    }
}

/// Object permission role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "wire", derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema))]
#[cfg_attr(feature = "wire", serde(rename_all = "lowercase"))]
pub enum ObjectRole {
    /// Read access plus participation in object commentary.
    Viewer,
    /// Viewer capabilities plus canonical object editing.
    Editor,
    /// Editor capabilities plus object administration and grant management.
    Admin,
}

impl ObjectRole {
    /// Returns the stable stored and serialized representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Editor => "editor",
            Self::Admin => "admin",
        }
    }

    /// Parses a role from its stable stored and serialized representation.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        value.parse().ok()
    }
}

impl std::str::FromStr for ObjectRole {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "viewer" => Ok(Self::Viewer),
            "editor" => Ok(Self::Editor),
            "admin" => Ok(Self::Admin),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for ObjectRole {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Membership role for groups and workspaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "wire", derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema))]
#[cfg_attr(feature = "wire", serde(rename_all = "lowercase"))]
pub enum MembershipRole {
    /// Regular member access.
    Member,
    /// Administrative access.
    Admin,
}

impl MembershipRole {
    /// Returns the stable stored and serialized representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Member => "member",
            Self::Admin => "admin",
        }
    }
}

impl std::str::FromStr for MembershipRole {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "member" => Ok(Self::Member),
            "admin" => Ok(Self::Admin),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for MembershipRole {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Principal targeted by an object grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "wire", derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema))]
#[cfg_attr(feature = "wire", serde(tag = "type", content = "id", rename_all = "snake_case"))]
pub enum GrantPrincipal {
    /// User principal.
    User(Uuid),
    /// Group principal.
    Group(Uuid),
}

/// Archive lifecycle status shared by archivable resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "wire", derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema))]
#[cfg_attr(feature = "wire", serde(rename_all = "lowercase"))]
pub enum ArchiveStatus {
    /// Active resource.
    Active,
    /// Archived resource.
    Archived,
}

impl ArchiveStatus {
    /// Returns the stable stored and serialized representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
        }
    }
}

impl std::str::FromStr for ArchiveStatus {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "active" => Ok(Self::Active),
            "archived" => Ok(Self::Archived),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for ArchiveStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Archive status filter for collection queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "wire", derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema))]
#[cfg_attr(feature = "wire", serde(rename_all = "lowercase"))]
pub enum ArchiveListStatus {
    /// Only active resources.
    #[default]
    Active,
    /// Only archived resources.
    Archived,
    /// Active and archived resources.
    All,
}

impl ArchiveListStatus {
    /// Returns the stable query representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
            Self::All => "all",
        }
    }
}

/// User lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "wire", derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema))]
#[cfg_attr(feature = "wire", serde(rename_all = "lowercase"))]
pub enum UserStatus {
    /// Active user.
    Active,
    /// Disabled user.
    Disabled,
}

impl UserStatus {
    /// Returns the stable stored and serialized representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disabled => "disabled",
        }
    }
}

impl std::str::FromStr for UserStatus {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "active" => Ok(Self::Active),
            "disabled" => Ok(Self::Disabled),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for UserStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// User lifecycle selector for administration queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "wire", derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema))]
#[cfg_attr(feature = "wire", serde(rename_all = "lowercase"))]
pub enum UserListStatus {
    /// Only active users.
    #[default]
    Active,
    /// Only disabled users.
    Disabled,
    /// Active and disabled users.
    All,
}

impl UserListStatus {
    /// Returns the stable query representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Disabled => "disabled",
            Self::All => "all",
        }
    }
}

/// Sort order for object collection queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "wire", derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema))]
#[cfg_attr(feature = "wire", serde(rename_all = "lowercase"))]
pub enum ObjectListOrder {
    /// Sort by creation time, newest first.
    #[default]
    Created,
    /// Sort by last update time, newest first.
    Updated,
}

impl ObjectListOrder {
    /// Returns the stable query representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Updated => "updated",
        }
    }
}

/// Search document category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "wire", derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema))]
#[cfg_attr(feature = "wire", serde(rename_all = "lowercase"))]
pub enum SearchCategory {
    /// Object-version title.
    Title,
    /// Object-version body.
    Body,
    /// Serialized object-version metadata.
    Metadata,
}

impl SearchCategory {
    /// Returns the stable stored and serialized representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Body => "body",
            Self::Metadata => "metadata",
        }
    }
}

impl std::str::FromStr for SearchCategory {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "title" => Ok(Self::Title),
            "body" => Ok(Self::Body),
            "metadata" => Ok(Self::Metadata),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for SearchCategory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Search match classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "wire", derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema))]
#[cfg_attr(feature = "wire", serde(rename_all = "snake_case"))]
pub enum SearchMatchKind {
    /// Full-text match.
    Text,
    /// Literal substring match.
    Literal,
    /// Exact category match.
    Exact,
}

impl SearchMatchKind {
    /// Returns the stable stored and serialized representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Literal => "literal",
            Self::Exact => "exact",
        }
    }
}

impl std::str::FromStr for SearchMatchKind {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "text" => Ok(Self::Text),
            "literal" => Ok(Self::Literal),
            "exact" => Ok(Self::Exact),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for SearchMatchKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Current lifecycle state of a comment body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "wire", derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema))]
#[cfg_attr(feature = "wire", serde(rename_all = "snake_case"))]
pub enum CommentStatus {
    /// Body remains available.
    Active,
    /// Body was removed by a user or administrator.
    Deleted,
    /// Body was removed by retention.
    Expired,
}

/// Search matching mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "wire", derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema))]
#[cfg_attr(feature = "wire", serde(rename_all = "snake_case"))]
pub enum SearchMode {
    /// Full-text or literal matching, with low-ranked partial-term fallback for plain multi-word
    /// queries.
    Auto,
    /// `PostgreSQL` web-search full-text matching.
    Text,
    /// Contiguous literal substring matching.
    Literal,
    /// Complete-value equality matching.
    Exact,
}

impl SearchMode {
    /// Returns the stable query representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Text => "text",
            Self::Literal => "literal",
            Self::Exact => "exact",
        }
    }
}

/// Traversal direction for an object-centered graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "wire", derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema))]
#[cfg_attr(feature = "wire", serde(rename_all = "lowercase"))]
pub enum ObjectGraphDirection {
    /// Follow edges from source to target.
    Outgoing,
    /// Follow edges from target to source.
    Incoming,
    /// Follow edges in both directions.
    #[default]
    Both,
}

impl ObjectGraphDirection {
    /// Returns the stable query representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Outgoing => "outgoing",
            Self::Incoming => "incoming",
            Self::Both => "both",
        }
    }

    /// Parses a traversal direction.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "outgoing" => Some(Self::Outgoing),
            "incoming" => Some(Self::Incoming),
            "both" => Some(Self::Both),
            _ => None,
        }
    }
}

impl std::fmt::Display for ObjectGraphDirection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Event sequence ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "wire", derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema))]
#[cfg_attr(feature = "wire", serde(rename_all = "lowercase"))]
pub enum EventOrder {
    /// Oldest events first.
    #[default]
    Asc,
    /// Newest events first.
    Desc,
}

impl EventOrder {
    /// Returns the stable query representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

impl std::str::FromStr for EventOrder {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "asc" => Ok(Self::Asc),
            "desc" => Ok(Self::Desc),
            _ => Err("expected `asc` or `desc`"),
        }
    }
}

impl std::fmt::Display for EventOrder {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ApiKeyScope, ArchiveStatus, EventOrder, MembershipRole, ObjectRole, SearchMatchKind,
        UserStatus,
    };

    #[test]
    fn event_order_names_round_trip() {
        for (value, order) in [("asc", EventOrder::Asc), ("desc", EventOrder::Desc)] {
            assert_eq!(value.parse(), Ok(order));
            assert_eq!(order.to_string(), value);
        }
        assert!("newest".parse::<EventOrder>().is_err());
    }

    #[test]
    fn api_key_scope_names_round_trip() {
        for scope in ApiKeyScope::ALL {
            assert_eq!(scope.as_str().parse(), Ok(*scope));
        }
    }

    #[test]
    fn stored_roles_and_statuses_round_trip() {
        for role in [ObjectRole::Viewer, ObjectRole::Editor, ObjectRole::Admin] {
            assert_eq!(role.as_str().parse(), Ok(role));
        }
        for role in [MembershipRole::Member, MembershipRole::Admin] {
            assert_eq!(role.as_str().parse(), Ok(role));
        }
        for status in [ArchiveStatus::Active, ArchiveStatus::Archived] {
            assert_eq!(status.as_str().parse(), Ok(status));
        }
        for status in [UserStatus::Active, UserStatus::Disabled] {
            assert_eq!(status.as_str().parse(), Ok(status));
        }
        for kind in [SearchMatchKind::Text, SearchMatchKind::Literal, SearchMatchKind::Exact] {
            assert_eq!(kind.as_str().parse(), Ok(kind));
        }
    }

    #[test]
    fn api_key_write_scopes_include_corresponding_read_scope() {
        for (write, read) in [
            (ApiKeyScope::WorkspaceWrite, ApiKeyScope::WorkspaceRead),
            (ApiKeyScope::ObjectWrite, ApiKeyScope::ObjectRead),
            (ApiKeyScope::AttachmentWrite, ApiKeyScope::AttachmentRead),
            (ApiKeyScope::GraphWrite, ApiKeyScope::GraphRead),
        ] {
            assert!(write.permits(write));
            assert!(write.permits(read));
            assert!(!read.permits(write));
        }
    }

    #[test]
    fn unrelated_api_key_scopes_do_not_imply_each_other() {
        assert!(!ApiKeyScope::Admin.permits(ApiKeyScope::ObjectRead));
        assert!(!ApiKeyScope::ObjectWrite.permits(ApiKeyScope::AttachmentRead));
        assert!(!ApiKeyScope::RealtimeRead.permits(ApiKeyScope::ObjectRead));
        assert!(!ApiKeyScope::RealtimeRead.permits(ApiKeyScope::EventRead));
        assert!(!ApiKeyScope::EventRead.permits(ApiKeyScope::RealtimeRead));
        assert!(!ApiKeyScope::AccessManage.permits(ApiKeyScope::WorkspaceRead));
    }
}
