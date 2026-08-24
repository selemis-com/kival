//! Wire protocol for Kival.

mod api_keys;
mod auth;
mod commentary;
mod common;
mod error;
mod events;
mod graph;
mod groups;
mod notifications;
mod objects;
mod pagination;
mod search;
mod status;
mod users;
mod workspaces;

/// Default API prefix used by the Kival server.
pub const API_PREFIX: &str = "/api/v1";

pub use api_keys::{
    ApiKey, ApiKeyListResponse, ApiKeyResponse, ApiKeyScope, CreateApiKeyRequest,
    CreateApiKeyResponse, UpdateApiKeyRequest,
};
pub use auth::{AuthenticatedSessionResponse, Session, SessionListResponse, SessionOnlyResponse};
pub use commentary::{
    COMMENT_BODY_MAX_CHARS, COMMENT_MENTION_MAX_USERS, Comment, CommentAuthor, CommentListResponse,
    CommentMention, CommentMentionCandidate, CommentMentionCandidateListResponse,
    CommentMentionCandidateParams, CommentResponse, CommentStatus, CommentThread,
    CommentThreadListResponse, CommentThreadResponse, CreateCommentRequest, UpdateCommentRequest,
};
pub use common::{ArchiveListStatus, ArchiveStatus, FavoriteState, PatchField, PinState};
pub use error::{ApiErrorBody, ApiErrorResponse};
pub use events::{Event, EventListParams, EventOrder};
pub use graph::{
    BacklinkSourceObject, CreateObjectEdgeRequest, CreateObjectGrantRequest, GrantPrincipal,
    ObjectBacklink, ObjectBacklinkReference, ObjectBacklinksParams, ObjectBacklinksResponse,
    ObjectEdge, ObjectEdgeResponse, ObjectGrant, ObjectGrantResponse, ObjectGraphDirection,
    ObjectGraphEdge, ObjectGraphNode, ObjectGraphParams, ObjectGraphResponse,
    ObjectGraphTruncation, ObjectRole, UpdateObjectGrantRequest, WorkspaceGraphEdge,
    WorkspaceGraphLimits, WorkspaceGraphNode, WorkspaceGraphParams, WorkspaceGraphResponse,
};
pub use groups::{
    CreateGroupMembershipRequest, CreateGroupRequest, Group, GroupListParams, GroupMembership,
    GroupMembershipResponse, GroupResponse, MembershipRole, UpdateGroupMembershipRequest,
    UpdateGroupRequest,
};
pub use notifications::{
    InboxEntry, InboxListParams, InboxUnreadCountResponse, InboxUpdatedResponse,
    MarkInboxReadRequest, ObjectNotificationPreference, RealtimeMessage, UpdateInboxEntryRequest,
    UpdateObjectNotificationPreferenceRequest,
};
pub use objects::{
    CreateObjectRequest, ObjectAttachment, ObjectAttachmentResponse, ObjectListItem,
    ObjectListOrder, ObjectListParams, ObjectResource, ObjectResponse, ObjectVersion,
    ObjectVersionResponse, ObjectVersionWikilink, ObjectVersionWikilinksResponse,
    ReuseObjectAttachmentRequest, UpdateObjectRequest, UploadObjectAttachmentParams,
};
pub use pagination::{DEFAULT_LIMIT, ListParams, ListResponse, MAX_LIMIT};
pub use search::{
    SearchCategory, SearchHit, SearchMatchKind, SearchMode, SearchParams, SearchResponse,
};
pub use status::{Status, StatusResponse};
pub use users::{
    UpdateUserRequest, User, UserListParams, UserListStatus, UserResponse, UserStatus,
};
pub use workspaces::{
    CreateWorkspaceGroupRequest, CreateWorkspaceMembershipRequest, CreateWorkspaceRequest,
    UpdateWorkspaceMembershipRequest, UpdateWorkspaceRequest, Workspace, WorkspaceGroup,
    WorkspaceGroupListParams, WorkspaceGroupResponse, WorkspaceListItem, WorkspaceListParams,
    WorkspaceMembership, WorkspaceMembershipResponse, WorkspaceResponse,
};
