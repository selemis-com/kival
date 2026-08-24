//! Typed Rust bindings for Kival's `PostgreSQL` state machine.
//!
//! `PostgreSQL` migrations define Kival's authoritative state and invariants.
//! This crate exposes Kival-specific reads and transitions over that state so
//! transports and operational code do not need to embed the state machine's
//! SQL directly. Its types deliberately follow `PostgreSQL`'s vocabulary rather
//! than defining a second schema or independent domain model.
//!
//! # Concurrency contract
//!
//! Domain authorization is an admission decision made by the server before opening a mutation
//! transaction. One authorization statement defines the admission linearization point; mutations
//! with several actor-authorization predicates evaluate those predicates in that same statement
//! snapshot. Once admitted, a request may finish even if its authority is concurrently revoked;
//! subsequent admission statements observe the committed revocation. Kernel transitions therefore
//! do not lock authorization provenance.
//!
//! Kernel mutations instead protect state validity: they acquire only the lifecycle or aggregate
//! locks required by the transition, acquire multiple resources in deterministic parent/ID order,
//! and re-check lifecycle/invariants at the write boundary. Long-lived external work must not hold
//! database locks and is revalidated in a short publication transaction. Protected reads remain
//! statement-authorized so permission and returned data share one PostgreSQL statement snapshot.

#![cfg_attr(not(test), warn(unused_crate_dependencies))]
#![cfg_attr(docsrs, feature(doc_cfg))]

mod access;
mod api_keys;
mod attachments;
mod commentary;
mod database;
mod error;
mod events;
mod favorites;
mod graph;
mod group_memberships;
mod groups;
mod notifications;
mod object_grants;
mod object_references;
mod object_versions;
mod objects;
mod operator;
mod passkeys;
mod realtime;
mod search;
mod sessions;
mod users;
mod workspace_groups;
mod workspace_memberships;
mod workspaces;

pub use access::{
    Capability, RoleCapability, active_group_admin_capability, active_object_role,
    active_object_role_pair, archived_object_admin_role, archived_workspace_admin_capability,
    can_manage_groups, is_global_admin, object_readable_role, workspace_membership_capability,
};
pub use api_keys::{
    ApiKeyAuthentication, ApiKeyAuthorizationUpdate, ApiKeyRow, api_key_workspaces_accessible,
    authenticate_api_key, create_api_key, fetch_api_key, list_api_key_scopes,
    list_api_key_workspaces, list_api_keys, lock_active_api_key, replace_api_key_authorization,
    revoke_api_key, set_api_key_delegation, touch_api_key_last_used,
};
pub use attachments::{
    CreateObjectAttachment, ObjectAttachmentContentRow, ObjectAttachmentRow, ReuseObjectAttachment,
    admit_attachment_reuse, create_object_attachment, fetch_object_attachment,
    fetch_object_attachment_content, list_object_attachments, object_version_belongs_to_object,
    reuse_object_attachment,
};
pub use commentary::{
    CommentPageQuery, CommentRow, LockedComment, MentionRow, ThreadRow, allowed_mentioned_user_ids,
    comment_mention_ids_in_tx, comment_thread_exists, create_comment, create_comment_thread,
    delete_comment, fetch_comment_for_mutation, fetch_comment_mentions,
    fetch_comment_mentions_for_mutation, fetch_comment_rows, fetch_comment_thread,
    fetch_comment_thread_for_mutation, fetch_initial_comment_rows,
    fetch_initial_comment_rows_for_mutation, fetch_thread_comment_page_rows, list_comment_threads,
    lock_comment, lock_thread_for_reply, lock_thread_resolution, mention_candidates,
    replace_comment_mentions, resolve_mentioned_usernames, set_thread_resolved,
    touch_comment_thread, update_comment_body,
};
pub use database::{
    DatabasePoolSettings, database_ready, open_pool_with_options, open_pool_with_settings,
};
pub use error::{KernelError, Result};

/// Parses constrained stored vocabulary at the kernel boundary.
fn parse_stored<T>(kind: &'static str, value: String) -> Result<T>
where
    T: std::str::FromStr<Err = ()>,
{
    value.parse().map_err(|()| KernelError::InvalidStoredValue { kind, value })
}

/// Parses optional constrained stored vocabulary at the kernel boundary.
fn parse_optional_stored<T>(kind: &'static str, value: Option<String>) -> Result<Option<T>>
where
    T: std::str::FromStr<Err = ()>,
{
    value.map(|value| parse_stored(kind, value)).transpose()
}
pub use events::{
    ApiKeyAttribution, EventInsert, EventKind, EventRow, ListEvents, append_event, list_events,
    list_object_events, list_workspace_events,
};
pub use favorites::{
    favorite_object, pin_object, pin_workspace, unfavorite_object, unpin_object, unpin_workspace,
};
pub use graph::{
    ListObjectBacklinks, ListObjectEdges, ObjectBacklinkReferenceRow, ObjectBacklinkRow,
    ObjectEdgeRow, ObjectGraphNodeRow, ObjectGraphQuery, WorkspaceGraphEdgeRow,
    WorkspaceGraphNodeRow, create_object_edge, fetch_object_edge, fetch_object_edge_for_revoke,
    list_object_backlink_edges, list_object_backlink_references, list_object_edges,
    object_graph_edges_for_nodes, object_graph_nodes, revoke_object_edge,
    workspace_graph_edges_for_nodes, workspace_graph_nodes,
};
pub use group_memberships::{
    GroupMembershipRow, create_group_membership, list_group_memberships, replace_group_membership,
    revoke_group_membership,
};
pub use groups::{
    GroupRow, archive_group, create_group, fetch_group, list_groups, unarchive_group, update_group,
};
pub use kival_types::{
    ApiKeyScope, ArchiveListStatus, ArchiveStatus, EventOrder, GrantPrincipal, MembershipRole,
    ObjectGraphDirection, ObjectListOrder, ObjectRole, SearchCategory, SearchMatchKind, SearchMode,
    UserListStatus, UserStatus,
};
pub use notifications::{
    InboxEntryRow, NotificationProjectionBatch, NotificationRetentionBatch,
    apply_notification_retention, inbox_unread_count, list_inbox_entries, mark_inbox_read,
    notification_candidates_exist_for_event, object_notification_preference,
    pending_notification_candidates_exist, process_notification_projection_batch,
    publish_inbox_updated, publish_inbox_updated_for_user, set_object_notification_preference,
    update_inbox_entry_read_state,
};
pub use object_grants::{
    ObjectGrantRow, create_object_grant, list_object_grants, replace_object_grant,
    revoke_object_grant,
};
pub use object_references::{
    ObjectReferenceMaintenance, ObjectReferenceUpdate, ObjectVersionWikilinkRow,
    ReferenceReresolutionSummary, list_object_version_wikilinks, maintain_object_references,
    re_resolve_current_wikilinks_for_titles,
};
pub(crate) use object_versions::create_object_version;
pub use object_versions::{
    CreateObjectVersion, ObjectVersion, ObjectVersionCreator, UpdateObjectVersion,
    UpdatedObjectVersion, fetch_object_version, fetch_object_version_by_number,
    fetch_object_version_creator_for_mutation, fetch_object_version_in_tx,
    list_object_version_creators, list_object_versions, update_object_version,
};
pub use objects::{
    CreateInitialObject, CreatedObject, ListObjects, Object, ObjectListEntry, ReadableObject,
    archive_object, create_initial_object, fetch_object, fetch_object_in_tx, list_objects,
    unarchive_object,
};
pub use operator::{
    OperatorRecoveryRevocations, create_operator_enrollment_code, enabled_global_admin_count,
    grant_global_admin_as_operator, is_bootstrapped, lock_active_user_for_operator,
    lock_admin_provisioning, lock_user_for_operator, record_bootstrap_completed,
    record_operator_passkey_recovery, record_operator_user_created, record_operator_user_lifecycle,
    revoke_credentials_for_operator_recovery, revoke_outstanding_enrollment_codes_as_operator,
    set_user_disabled_as_operator, user_count,
};
pub use passkeys::{
    EnrollmentCompletion, EnrollmentIdentity, FreshAuthenticationCeremony,
    PasskeyEnrollmentPurpose, PasskeyRow, active_credential_ids_in_tx, active_enrollment_ceremony,
    consume_ceremony, consume_enrollment_code, consume_expired_enrollment_ceremonies,
    create_authentication_ceremony, create_enrollment_ceremony,
    create_fresh_authentication_ceremony, create_passkey, create_registration_ceremony,
    enrollment_completion_user_id, has_active_passkey, has_active_passkey_in_tx, list_passkeys,
    lock_active_passkey_ids, lock_enrollment_completion, lock_enrollment_identity,
    lock_fresh_authentication_ceremony, lock_login_ceremony, lock_passkey,
    lock_registration_ceremony, login_ceremony_user_id, prune_terminal_ceremonies,
    record_passkey_use, registration_identity, revoke_passkey,
};
pub use realtime::{
    realtime_api_key_active, realtime_api_key_object_authorized, realtime_object_authorized,
    realtime_session_active, realtime_workspace_authorized,
};
pub use search::{SearchDocumentRow, SearchDocuments, search_documents};
pub use sessions::{
    AuthenticatedSession, FreshAuthenticationSessionRotation, SessionRow, active_session_csrf_hash,
    authenticate_session, create_session, current_session_id, list_active_sessions,
    lock_fresh_session, prune_terminal_sessions, revoke_session, revoke_session_for_logout,
    rotate_session_after_fresh_authentication, touch_session_last_seen,
};
pub use users::{
    CreatedUser, UserRow, active_user_id_by_username, create_user, disable_user, enable_user,
    fetch_active_user, fetch_user, list_users, lock_active_user_by_id, update_user_display_name,
};
pub use workspace_groups::{
    WorkspaceGroupRow, archive_workspace_group, create_workspace_group, list_workspace_groups,
    unarchive_workspace_group,
};
pub use workspace_memberships::{
    WorkspaceMembershipRow, create_workspace_membership, list_workspace_memberships,
    replace_workspace_membership, revoke_workspace_membership,
};
pub use workspaces::{
    ListVisibleWorkspaces, VisibleWorkspaceRow, WorkspaceRow, archive_workspace, create_workspace,
    fetch_visible_workspace, list_visible_workspaces, unarchive_workspace, update_workspace,
    workspace_exists,
};
