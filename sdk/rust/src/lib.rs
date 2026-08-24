//! SDK for Kival.

#![doc = include_str!("../README.md")]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/selemis-com/.github/main/assets/logo.jpg",
    html_favicon_url = "https://raw.githubusercontent.com/selemis-com/.github/main/assets/favicon.ico"
)]
#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "client")]
#[cfg_attr(docsrs, doc(cfg(feature = "client")))]
pub mod client;
pub mod wire;

#[cfg(feature = "client")]
pub use client::{
    ApiError, ApiErrorKind, BaseUrlError, BoxTransport, BoxTransportFuture, ClientBuilder,
    ClientError, DefaultTransport, HttpTransport, KivalClient, MapTransportError,
    ObjectVersionIdentifier, ProviderBuilder, ResponseTransport, RootProvider, Transport,
    TransportError, TransportErrorKind, UrlError,
};
pub use wire::{
    API_PREFIX, ApiErrorBody, ApiErrorResponse, ApiKey, ApiKeyListResponse, ApiKeyResponse,
    ApiKeyScope, ArchiveListStatus, ArchiveStatus, AuthenticatedSessionResponse,
    BacklinkSourceObject, COMMENT_BODY_MAX_CHARS, COMMENT_MENTION_MAX_USERS, Comment,
    CommentAuthor, CommentListResponse, CommentMention, CommentMentionCandidate,
    CommentMentionCandidateListResponse, CommentMentionCandidateParams, CommentResponse,
    CommentStatus, CommentThread, CommentThreadListResponse, CommentThreadResponse,
    CreateApiKeyRequest, CreateApiKeyResponse, CreateCommentRequest, CreateGroupMembershipRequest,
    CreateGroupRequest, CreateObjectEdgeRequest, CreateObjectGrantRequest, CreateObjectRequest,
    CreateWorkspaceGroupRequest, CreateWorkspaceMembershipRequest, CreateWorkspaceRequest,
    DEFAULT_LIMIT, Event, EventListParams, EventOrder, FavoriteState, GrantPrincipal,
    GraphEdgeKind, Group, GroupListParams, GroupMembership, GroupMembershipResponse, GroupResponse,
    InboxEntry, InboxListParams, InboxUnreadCountResponse, InboxUpdatedResponse, ListParams,
    ListResponse, MAX_LIMIT, MarkInboxReadRequest, MembershipRole, ObjectAttachment,
    ObjectAttachmentResponse, ObjectBacklink, ObjectBacklinkReference, ObjectBacklinksParams,
    ObjectBacklinksResponse, ObjectEdge, ObjectEdgeResponse, ObjectGrant, ObjectGrantResponse,
    ObjectGraphDirection, ObjectGraphEdge, ObjectGraphNode, ObjectGraphParams, ObjectGraphResponse,
    ObjectGraphTruncation, ObjectListItem, ObjectListOrder, ObjectListParams,
    ObjectNotificationPreference, ObjectResource, ObjectResponse, ObjectRole, ObjectVersion,
    ObjectVersionResponse, ObjectVersionWikilink, ObjectVersionWikilinksResponse, PatchField,
    PinState, RealtimeMessage, ReuseObjectAttachmentRequest, SearchHit, SearchMatchKind,
    SearchMode, SearchParams, SearchResponse, Session, SessionListResponse, SessionOnlyResponse,
    Status, StatusResponse, UpdateApiKeyRequest, UpdateCommentRequest,
    UpdateGroupMembershipRequest, UpdateGroupRequest, UpdateInboxEntryRequest,
    UpdateObjectGrantRequest, UpdateObjectNotificationPreferenceRequest, UpdateObjectRequest,
    UpdateUserRequest, UpdateWorkspaceMembershipRequest, UpdateWorkspaceRequest,
    UploadObjectAttachmentParams, User, UserListParams, UserListStatus, UserResponse, UserStatus,
    Workspace, WorkspaceGraphEdge, WorkspaceGraphLimits, WorkspaceGraphNode, WorkspaceGraphParams,
    WorkspaceGraphResponse, WorkspaceGroup, WorkspaceGroupListParams, WorkspaceGroupResponse,
    WorkspaceListItem, WorkspaceListParams, WorkspaceMembership, WorkspaceMembershipResponse,
    WorkspaceResponse,
};
