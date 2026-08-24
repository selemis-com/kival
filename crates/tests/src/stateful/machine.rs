use kival_sdk::{ApiKeyScope, MembershipRole, ObjectRole};
use proptest::{
    prelude::{BoxedStrategy, Just, Strategy, any},
    sample::select,
    strategy::Union,
};
use proptest_state_machine::ReferenceStateMachine;
use serde::{Deserialize, Serialize};

use super::{Handle, Lifecycle, Model, ModeledAttachment, ModeledEvent, Principal, ResourceKind};
use crate::actors::Actor;

/// A shrinkable transition in the Kival reference state machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum Operation {
    /// Reads the authenticated identity for an actor.
    CheckWhoAmI {
        /// Actor making the request.
        actor: Actor,
    },
    /// Creates a workspace and binds its returned ID to a handle.
    CreateWorkspace {
        /// Actor making the request.
        actor: Actor,
        /// Handle to bind to the created workspace.
        output: Handle,
        /// Workspace name.
        name: String,
    },
    /// Lists workspaces visible to an actor.
    ListWorkspaces {
        /// Actor making the request.
        actor: Actor,
    },
    /// Creates a global user group.
    CreateGroup {
        /// Actor making the request.
        actor: Actor,
        /// Handle to bind to the created group.
        output: Handle,
        /// Group name.
        name: String,
    },
    /// Lists groups visible to an actor.
    ListGroups {
        /// Actor making the request.
        actor: Actor,
    },
    /// Reads a group.
    GetGroup {
        /// Actor making the request.
        actor: Actor,
        /// Group to read.
        group: Handle,
    },
    /// Updates a group's name.
    UpdateGroup {
        /// Actor making the request.
        actor: Actor,
        /// Group being updated.
        group: Handle,
        /// New group name.
        name: String,
    },
    /// Archives a group.
    ArchiveGroup {
        /// Actor making the request.
        actor: Actor,
        /// Group being archived.
        group: Handle,
    },
    /// Restores an archived group.
    UnarchiveGroup {
        /// Actor making the request.
        actor: Actor,
        /// Group being restored.
        group: Handle,
    },
    /// Lists active memberships in a group.
    ListGroupMemberships {
        /// Actor making the request.
        actor: Actor,
        /// Group whose memberships are listed.
        group: Handle,
    },
    /// Adds an actor to a group.
    CreateGroupMembership {
        /// Actor making the request.
        actor: Actor,
        /// Group receiving the membership.
        group: Handle,
        /// Actor receiving the membership.
        member: Actor,
        /// Role assigned to the member.
        role: MembershipRole,
        /// Handle to bind to the created membership.
        output: Handle,
    },
    /// Revokes a generated group membership.
    RevokeGroupMembership {
        /// Actor making the request.
        actor: Actor,
        /// Group containing the membership.
        group: Handle,
        /// Membership to revoke.
        membership: Handle,
    },
    /// Replaces a group membership with one carrying a new role.
    UpdateGroupMembership {
        /// Actor making the request.
        actor: Actor,
        /// Group containing the membership.
        group: Handle,
        /// Existing membership to replace.
        membership: Handle,
        /// New role assigned to the member.
        role: MembershipRole,
        /// Handle to bind to the replacement membership.
        output: Handle,
    },
    /// Verifies group membership writes are rejected once the group is archived.
    ProbeArchivedGroupMembershipWrites {
        /// Actor making the rejected requests.
        actor: Actor,
        /// Archived group receiving the rejected requests.
        group: Handle,
        /// Existing active membership used for the revoke attempt.
        membership: Handle,
        /// Non-member used for the create attempt.
        member: Actor,
    },
    /// Links a group to a workspace.
    LinkWorkspaceGroup {
        /// Actor making the request.
        actor: Actor,
        /// Workspace receiving the link.
        workspace: Handle,
        /// Group being linked.
        group: Handle,
    },
    /// Archives a workspace-group link.
    ArchiveWorkspaceGroup {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the link.
        workspace: Handle,
        /// Linked group.
        group: Handle,
    },
    /// Restores a workspace-group link.
    UnarchiveWorkspaceGroup {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the link.
        workspace: Handle,
        /// Linked group.
        group: Handle,
    },
    /// Reads a workspace.
    GetWorkspace {
        /// Actor making the request.
        actor: Actor,
        /// Workspace to read.
        workspace: Handle,
    },
    /// Reads the authorized graph projection for a workspace.
    GetWorkspaceGraph {
        /// Actor making the request.
        actor: Actor,
        /// Workspace whose graph is projected.
        workspace: Handle,
    },
    /// Searches for an object's unique generated title.
    SearchWorkspace {
        /// Actor making the request.
        actor: Actor,
        /// Workspace being searched.
        workspace: Handle,
        /// Object whose title is used as the exact query.
        object: Handle,
    },
    /// Reads a workspace's audit events.
    GetWorkspaceEvents {
        /// Actor making the request.
        actor: Actor,
        /// Workspace whose events are requested.
        workspace: Handle,
    },
    /// Lists active direct memberships visible through a workspace.
    ListWorkspaceMemberships {
        /// Actor making the request.
        actor: Actor,
        /// Workspace whose memberships are listed.
        workspace: Handle,
    },
    /// Lists workspace-group links visible through a workspace.
    ListWorkspaceGroups {
        /// Actor making the request.
        actor: Actor,
        /// Workspace whose group links are listed.
        workspace: Handle,
    },
    /// Updates a workspace's name.
    UpdateWorkspace {
        /// Actor making the request.
        actor: Actor,
        /// Workspace being updated.
        workspace: Handle,
        /// New workspace name.
        name: String,
    },
    /// Archives a workspace.
    ArchiveWorkspace {
        /// Actor making the request.
        actor: Actor,
        /// Workspace to archive.
        workspace: Handle,
    },
    /// Restores an archived workspace.
    UnarchiveWorkspace {
        /// Actor making the request.
        actor: Actor,
        /// Workspace to restore.
        workspace: Handle,
    },
    /// Adds an actor to a workspace.
    CreateWorkspaceMembership {
        /// Actor making the request.
        actor: Actor,
        /// Workspace receiving the membership.
        workspace: Handle,
        /// Actor receiving the membership.
        member: Actor,
        /// Role assigned to the member.
        role: MembershipRole,
        /// Handle to bind to the created membership.
        output: Handle,
    },
    /// Revokes a generated workspace membership.
    RevokeWorkspaceMembership {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the membership.
        workspace: Handle,
        /// Membership to revoke.
        membership: Handle,
    },
    /// Replaces a workspace membership with one carrying a new role.
    UpdateWorkspaceMembership {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the membership.
        workspace: Handle,
        /// Existing membership to replace.
        membership: Handle,
        /// New role assigned to the member.
        role: MembershipRole,
        /// Handle to bind to the replacement membership.
        output: Handle,
    },
    /// Creates a direct object grant.
    CreateObjectGrant {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object receiving the grant.
        object: Handle,
        /// Actor receiving object authority.
        principal: Actor,
        /// Role assigned by the grant.
        role: ObjectRole,
        /// Handle to bind to the created grant.
        output: Handle,
    },
    /// Creates a group-backed object grant.
    CreateGroupObjectGrant {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object receiving the grant.
        object: Handle,
        /// Group receiving object authority.
        principal: Handle,
        /// Role assigned by the grant.
        role: ObjectRole,
        /// Handle to bind to the created grant.
        output: Handle,
    },
    /// Revokes a generated direct object grant.
    RevokeObjectGrant {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object containing the grant.
        object: Handle,
        /// Grant to revoke.
        grant: Handle,
    },
    /// Replaces a grant with one carrying a new role.
    UpdateObjectGrant {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object containing the grant.
        object: Handle,
        /// Existing grant to replace.
        grant: Handle,
        /// New role assigned to the principal.
        role: ObjectRole,
        /// Handle to bind to the replacement grant.
        output: Handle,
    },
    /// Creates a workspace-restricted API key.
    CreateApiKey {
        /// Session actor creating the key.
        actor: Actor,
        /// Workspace delegated to the key.
        workspace: Handle,
        /// Handle to bind to the API key.
        output: Handle,
        /// Single modeled delegated scope.
        scope: ApiKeyScope,
    },
    /// Replaces a modeled API key's delegated scope.
    UpdateApiKey {
        /// Owner session updating the key.
        actor: Actor,
        /// API key being updated.
        key: Handle,
        /// Replacement scope.
        scope: ApiKeyScope,
    },
    /// Revokes an active modeled API key.
    RevokeApiKey {
        /// Owner session revoking the key.
        actor: Actor,
        /// API key being revoked.
        key: Handle,
    },
    /// Exercises bearer authorization against workspace and object reads.
    ProbeApiKeyAccess {
        /// Owner of the delegated credential.
        actor: Actor,
        /// API key used for bearer requests.
        key: Handle,
        /// Delegated workspace.
        workspace: Handle,
        /// Object used to test scope and owner-authority composition.
        object: Handle,
    },
    /// Creates an object and binds its returned ID to a handle.
    CreateObject {
        /// Actor making the request.
        actor: Actor,
        /// Workspace in which to create the object.
        workspace: Handle,
        /// Handle to bind to the created object.
        output: Handle,
        /// Handle to bind to the automatic creator-admin grant.
        creator_grant: Handle,
        /// Object title.
        title: String,
    },
    /// Reads an object.
    GetObject {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object to read.
        object: Handle,
    },
    /// Reads the authorized graph neighborhood around an object.
    GetObjectGraph {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Root object.
        object: Handle,
    },
    /// Reads explicit and textual backlinks for an object.
    GetObjectBacklinks {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Backlink target.
        object: Handle,
    },
    /// Reads an object's audit events.
    GetObjectEvents {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object whose events are requested.
        object: Handle,
    },
    /// Lists active object grants through the administrative collection endpoint.
    ListObjectGrants {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object whose grants are listed.
        object: Handle,
    },
    /// Lists active explicit edges incident to an object.
    ListObjectEdges {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object whose incident edges are listed.
        object: Handle,
    },
    /// Reads an object's current version through its concrete version endpoint.
    GetObjectVersion {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object whose current version is requested.
        object: Handle,
    },
    /// Pins an active workspace for a session actor.
    PinWorkspace {
        /// Actor pinning the workspace.
        actor: Actor,
        /// Workspace being pinned.
        workspace: Handle,
    },
    /// Removes a previously created workspace pin.
    UnpinWorkspace {
        /// Actor removing the pin.
        actor: Actor,
        /// Workspace whose pin is removed.
        workspace: Handle,
    },
    /// Pins a readable object for a session actor.
    PinObject {
        /// Actor pinning the object.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object being pinned.
        object: Handle,
    },
    /// Removes a previously created object pin.
    UnpinObject {
        /// Actor removing the pin.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object whose pin is removed.
        object: Handle,
    },
    /// Favorites a readable object for a session actor.
    FavoriteObject {
        /// Actor favoriting the object.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object being favorited.
        object: Handle,
    },
    /// Removes a previously created object favorite.
    UnfavoriteObject {
        /// Actor removing the favorite.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object whose favorite is removed.
        object: Handle,
    },
    /// Creates a top-level commentary thread and root comment.
    CreateCommentThread {
        /// Actor creating the thread.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object receiving the thread.
        object: Handle,
        /// Thread handle to bind.
        thread_output: Handle,
        /// Root comment handle to bind.
        comment_output: Handle,
        /// Root comment body.
        body: String,
    },
    /// Replies to an open commentary thread.
    ReplyComment {
        /// Actor creating the reply.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object containing the thread.
        object: Handle,
        /// Thread receiving the reply.
        thread: Handle,
        /// Reply handle to bind.
        output: Handle,
        /// Reply body.
        body: String,
    },
    /// Edits an authored active comment in an open thread.
    EditComment {
        /// Comment author.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object containing the comment.
        object: Handle,
        /// Comment being edited.
        comment: Handle,
        /// Replacement body.
        body: String,
    },
    /// Soft-deletes an active comment as its author or an object administrator.
    DeleteComment {
        /// Actor deleting the comment.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object containing the comment.
        object: Handle,
        /// Comment being deleted.
        comment: Handle,
    },
    /// Resolves an open commentary thread.
    ResolveCommentThread {
        /// Thread author or object administrator.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object containing the thread.
        object: Handle,
        /// Thread being resolved.
        thread: Handle,
    },
    /// Reopens a resolved commentary thread.
    ReopenCommentThread {
        /// Thread author or object administrator.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object containing the thread.
        object: Handle,
        /// Thread being reopened.
        thread: Handle,
    },
    /// Lists comments in a modeled thread through an arbitrary actor.
    ListThreadComments {
        /// Actor reading the thread.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object containing the thread.
        object: Handle,
        /// Thread whose comments are listed.
        thread: Handle,
    },
    /// Lists current mention candidates through an arbitrary actor.
    ListMentionCandidates {
        /// Actor reading mention candidates.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object whose candidates are listed.
        object: Handle,
        /// Modeled user looked up by unique username.
        candidate: Actor,
    },
    /// Exercises comment mention hydration and replacement on a readable object.
    ProbeCommentMentions {
        /// Actor creating and editing the comment.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object receiving the comment.
        object: Handle,
        /// User mentioned by the initial comment.
        first_mention: Actor,
        /// User replacing the initial mention.
        second_mention: Actor,
    },
    /// Exercises notification preference, projection, inbox, and read-state routes.
    ProbeNotificationInbox {
        /// Recipient session exercising personal notification routes.
        actor: Actor,
        /// Workspace containing the notification source.
        workspace: Handle,
        /// Object containing the notification source.
        object: Handle,
    },
    /// Temporarily disables a user and verifies authentication gates before restoring them.
    ProbeUserDisableEnable {
        /// Global administrator performing the lifecycle transitions.
        actor: Actor,
        /// User temporarily disabled.
        target: Actor,
        /// Workspace the target can currently read.
        workspace: Handle,
        /// Object the target can currently read.
        object: Handle,
    },
    /// Exercises browser-session lifecycle and last-passkey protection.
    ProbeAuthLifecycle {
        /// Session owner exercising authentication lifecycle routes.
        actor: Actor,
    },
    /// Verifies non-admin group mutations remain rejected as authorization state evolves.
    ProbeUnauthorizedGroupMutations {
        /// Actor making the rejected requests.
        actor: Actor,
        /// Active group targeted by the rejected requests.
        group: Handle,
        /// Non-admin user used as a syntactically valid membership target.
        member: Actor,
    },
    /// Verifies workspace members without admin authority cannot mutate workspace access state.
    ProbeUnauthorizedWorkspaceMutations {
        /// Actor making the rejected requests.
        actor: Actor,
        /// Active workspace targeted by the rejected requests.
        workspace: Handle,
        /// Non-member used as a syntactically valid membership target.
        member: Actor,
    },
    /// Verifies readable objects cannot be mutated without edit/admin authority.
    ProbeUnauthorizedObjectMutations {
        /// Actor making the rejected requests.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Active readable object targeted by the rejected requests.
        object: Handle,
    },
    /// Verifies actors outside a workspace cannot create objects in it.
    ProbeUnauthorizedObjectCreate {
        /// Actor making the rejected request.
        actor: Actor,
        /// Active workspace targeted by the rejected request.
        workspace: Handle,
    },
    /// Exercises resolved, unresolved, retargeted, and removed wikilink projections.
    ProbeWikilinkReresolution {
        /// Actor editing both source and target objects.
        actor: Actor,
        /// Workspace containing both objects.
        workspace: Handle,
        /// Object whose body contains the generated wikilink.
        source: Handle,
        /// Object targeted by the generated wikilink.
        target: Handle,
        /// Suffix used for the temporary target title.
        suffix: u32,
    },
    /// Verifies active-object writes are rejected below an archived workspace.
    ProbeArchivedWorkspaceObjectWrites {
        /// Actor making the rejected requests.
        actor: Actor,
        /// Archived workspace containing the object.
        workspace: Handle,
        /// Active object beneath the archived workspace.
        object: Handle,
        /// User used as the syntactically valid grant principal.
        principal: Actor,
    },
    /// Verifies archived-object restoration is rejected below an archived workspace.
    ProbeArchivedWorkspaceObjectRestore {
        /// Actor making the rejected request.
        actor: Actor,
        /// Archived workspace containing the object.
        workspace: Handle,
        /// Archived object beneath the archived workspace.
        object: Handle,
    },
    /// Uploads bytes as a new object attachment.
    UploadObjectAttachment {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object receiving the attachment.
        object: Handle,
        /// Handle to bind to the created attachment.
        output: Handle,
        /// Attachment display name.
        name: String,
        /// Uploaded attachment bytes.
        content: Vec<u8>,
    },
    /// Lists attachments owned by an object.
    ListObjectAttachments {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object whose attachments are listed.
        object: Handle,
    },
    /// Reads attachment metadata.
    GetObjectAttachment {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object owning the attachment.
        object: Handle,
        /// Attachment to read.
        attachment: Handle,
    },
    /// Downloads attachment content.
    GetObjectAttachmentContent {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object owning the attachment.
        object: Handle,
        /// Attachment whose bytes are downloaded.
        attachment: Handle,
    },
    /// Reuses an authorized attachment on an editable object.
    ReuseObjectAttachment {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing both objects.
        workspace: Handle,
        /// Target object receiving the attachment.
        object: Handle,
        /// Existing source attachment.
        source: Handle,
        /// Handle to bind to the reused attachment.
        output: Handle,
    },
    /// Updates an object by appending a version.
    UpdateObject {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object to update.
        object: Handle,
        /// New object title.
        title: String,
    },
    /// Archives an object.
    ArchiveObject {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object to archive.
        object: Handle,
    },
    /// Restores an archived object.
    UnarchiveObject {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the object.
        workspace: Handle,
        /// Object to restore.
        object: Handle,
    },
    /// Creates a directed edge between two objects.
    CreateObjectEdge {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing both objects.
        workspace: Handle,
        /// Source object requiring edit authority.
        source: Handle,
        /// Target object requiring view authority.
        target: Handle,
        /// Handle to bind to the edge.
        output: Handle,
    },
    /// Reads an object edge.
    GetObjectEdge {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the edge.
        workspace: Handle,
        /// Edge to read.
        edge: Handle,
    },
    /// Revokes an active object edge.
    RevokeObjectEdge {
        /// Actor making the request.
        actor: Actor,
        /// Workspace containing the edge.
        workspace: Handle,
        /// Edge to revoke.
        edge: Handle,
    },
}

impl Operation {
    /// Returns the actor that performs this operation.
    #[must_use]
    pub const fn actor(&self) -> Actor {
        match self {
            Self::CheckWhoAmI { actor }
            | Self::CreateWorkspace { actor, .. }
            | Self::ListWorkspaces { actor }
            | Self::CreateGroup { actor, .. }
            | Self::ListGroups { actor }
            | Self::GetGroup { actor, .. }
            | Self::UpdateGroup { actor, .. }
            | Self::ArchiveGroup { actor, .. }
            | Self::UnarchiveGroup { actor, .. }
            | Self::ListGroupMemberships { actor, .. }
            | Self::CreateGroupMembership { actor, .. }
            | Self::RevokeGroupMembership { actor, .. }
            | Self::UpdateGroupMembership { actor, .. }
            | Self::ProbeArchivedGroupMembershipWrites { actor, .. }
            | Self::LinkWorkspaceGroup { actor, .. }
            | Self::ArchiveWorkspaceGroup { actor, .. }
            | Self::UnarchiveWorkspaceGroup { actor, .. }
            | Self::GetWorkspace { actor, .. }
            | Self::GetWorkspaceGraph { actor, .. }
            | Self::SearchWorkspace { actor, .. }
            | Self::GetWorkspaceEvents { actor, .. }
            | Self::ListWorkspaceMemberships { actor, .. }
            | Self::ListWorkspaceGroups { actor, .. }
            | Self::UpdateWorkspace { actor, .. }
            | Self::ArchiveWorkspace { actor, .. }
            | Self::UnarchiveWorkspace { actor, .. }
            | Self::CreateWorkspaceMembership { actor, .. }
            | Self::RevokeWorkspaceMembership { actor, .. }
            | Self::UpdateWorkspaceMembership { actor, .. }
            | Self::CreateObjectGrant { actor, .. }
            | Self::CreateGroupObjectGrant { actor, .. }
            | Self::RevokeObjectGrant { actor, .. }
            | Self::UpdateObjectGrant { actor, .. }
            | Self::CreateApiKey { actor, .. }
            | Self::UpdateApiKey { actor, .. }
            | Self::RevokeApiKey { actor, .. }
            | Self::ProbeApiKeyAccess { actor, .. }
            | Self::CreateObject { actor, .. }
            | Self::GetObject { actor, .. }
            | Self::GetObjectGraph { actor, .. }
            | Self::GetObjectBacklinks { actor, .. }
            | Self::GetObjectEvents { actor, .. }
            | Self::GetObjectVersion { actor, .. }
            | Self::ListObjectGrants { actor, .. }
            | Self::ListObjectEdges { actor, .. }
            | Self::PinWorkspace { actor, .. }
            | Self::UnpinWorkspace { actor, .. }
            | Self::PinObject { actor, .. }
            | Self::UnpinObject { actor, .. }
            | Self::FavoriteObject { actor, .. }
            | Self::UnfavoriteObject { actor, .. }
            | Self::CreateCommentThread { actor, .. }
            | Self::ReplyComment { actor, .. }
            | Self::EditComment { actor, .. }
            | Self::DeleteComment { actor, .. }
            | Self::ResolveCommentThread { actor, .. }
            | Self::ReopenCommentThread { actor, .. }
            | Self::ListThreadComments { actor, .. }
            | Self::ListMentionCandidates { actor, .. }
            | Self::ProbeCommentMentions { actor, .. }
            | Self::ProbeNotificationInbox { actor, .. }
            | Self::ProbeUserDisableEnable { actor, .. }
            | Self::ProbeAuthLifecycle { actor }
            | Self::ProbeUnauthorizedGroupMutations { actor, .. }
            | Self::ProbeUnauthorizedWorkspaceMutations { actor, .. }
            | Self::ProbeUnauthorizedObjectMutations { actor, .. }
            | Self::ProbeUnauthorizedObjectCreate { actor, .. }
            | Self::ProbeWikilinkReresolution { actor, .. }
            | Self::ProbeArchivedWorkspaceObjectWrites { actor, .. }
            | Self::ProbeArchivedWorkspaceObjectRestore { actor, .. }
            | Self::UploadObjectAttachment { actor, .. }
            | Self::ListObjectAttachments { actor, .. }
            | Self::GetObjectAttachment { actor, .. }
            | Self::GetObjectAttachmentContent { actor, .. }
            | Self::ReuseObjectAttachment { actor, .. }
            | Self::UpdateObject { actor, .. }
            | Self::ArchiveObject { actor, .. }
            | Self::UnarchiveObject { actor, .. }
            | Self::CreateObjectEdge { actor, .. }
            | Self::GetObjectEdge { actor, .. }
            | Self::RevokeObjectEdge { actor, .. } => *actor,
        }
    }
}

/// Proptest reference state machine for Kival operations.
///
/// Use this as [`proptest_state_machine::StateMachineTest::Reference`] in a
/// concrete server or SDK state-machine test. Proptest will generate valid
/// operation sequences and shrink failures while rechecking [`Self::preconditions`].
#[derive(Debug, Clone, Copy)]
pub struct KivalStateMachine;

impl ReferenceStateMachine for KivalStateMachine {
    type State = Model;
    type Transition = Operation;

    fn init_state() -> BoxedStrategy<Self::State> {
        Just(Model::default()).boxed()
    }

    fn transitions(state: &Self::State) -> BoxedStrategy<Self::Transition> {
        let workspace_output = Handle::new(ResourceKind::Workspace, state.next_workspace);
        let group_output = Handle::new(ResourceKind::Group, state.next_group);
        let membership_output = Handle::new(ResourceKind::Membership, state.next_membership);
        let object_output = Handle::new(ResourceKind::Object, state.next_object);
        let edge_output = Handle::new(ResourceKind::Edge, state.next_edge);
        let attachment_output = Handle::new(ResourceKind::Attachment, state.next_attachment);
        let grant_output = Handle::new(ResourceKind::Grant, state.next_grant);
        let comment_thread_output =
            Handle::new(ResourceKind::CommentThread, state.next_comment_thread);
        let comment_output = Handle::new(ResourceKind::Comment, state.next_comment);
        let api_key_output = Handle::new(ResourceKind::ApiKey, state.next_api_key);

        let mut transitions: Vec<(u32, BoxedStrategy<Operation>)> = vec![
            (1, actor().prop_map(|actor| Operation::CheckWhoAmI { actor }).boxed()),
            (2, actor().prop_map(|actor| Operation::ListWorkspaces { actor }).boxed()),
            (2, actor().prop_map(|actor| Operation::ListGroups { actor }).boxed()),
            (
                if state.workspace_count() == 0 { 6 } else { 1 },
                (actor(), any::<u32>())
                    .prop_map(move |(actor, suffix)| Operation::CreateWorkspace {
                        actor,
                        output: workspace_output,
                        name: generated_name("workspace", workspace_output.index, suffix),
                    })
                    .boxed(),
            ),
            (
                if state.active_groups().is_empty() { 4 } else { 1 },
                any::<u32>()
                    .prop_map(move |suffix| Operation::CreateGroup {
                        actor: Actor::Admin,
                        output: group_output,
                        name: generated_name("group", group_output.index, suffix),
                    })
                    .boxed(),
            ),
        ];

        push_strategy(&mut transitions, 3, selected_group_read(state));
        push_strategy(&mut transitions, 3, selected_group_update(state));
        push_strategy(&mut transitions, 2, selected_group_archive(state));
        push_strategy(&mut transitions, 2, selected_group_unarchive(state));
        push_strategy(&mut transitions, 3, selected_group_memberships_read(state));
        push_strategy(
            &mut transitions,
            4,
            selected_group_membership_create(state, membership_output),
        );
        push_strategy(&mut transitions, 2, selected_group_membership_revoke(state));
        push_strategy(
            &mut transitions,
            3,
            selected_group_membership_update(state, membership_output),
        );
        push_strategy(&mut transitions, 2, selected_archived_group_membership_write_probe(state));
        push_strategy(&mut transitions, 4, selected_workspace_group_link(state));
        push_strategy(&mut transitions, 2, selected_workspace_group_archive(state));
        push_strategy(&mut transitions, 2, selected_workspace_group_unarchive(state));
        push_strategy(&mut transitions, 3, selected_workspace_read(state));
        push_strategy(&mut transitions, 3, selected_workspace_update(state));
        push_strategy(&mut transitions, 4, selected_workspace_graph_read(state));
        push_strategy(&mut transitions, 3, selected_workspace_search(state));
        push_strategy(&mut transitions, 2, selected_workspace_events_read(state));
        push_strategy(&mut transitions, 3, selected_workspace_memberships_read(state));
        push_strategy(&mut transitions, 3, selected_workspace_groups_read(state));
        push_strategy(
            &mut transitions,
            2,
            selected_workspace(state.active_workspace_admins(), |actor, workspace| {
                Operation::ArchiveWorkspace { actor, workspace }
            }),
        );
        push_strategy(
            &mut transitions,
            2,
            selected_workspace(state.archived_workspace_admins(), |actor, workspace| {
                Operation::UnarchiveWorkspace { actor, workspace }
            }),
        );
        push_strategy(&mut transitions, 3, selected_membership_create(state, membership_output));
        push_strategy(&mut transitions, 2, selected_membership_revoke(state));
        push_strategy(&mut transitions, 3, selected_membership_update(state, membership_output));
        push_strategy(&mut transitions, 3, selected_api_key_create(state, api_key_output));
        push_strategy(&mut transitions, 2, selected_api_key_update(state));
        push_strategy(&mut transitions, 2, selected_api_key_revoke(state));
        push_strategy(&mut transitions, 3, selected_api_key_access_probe(state));
        push_strategy(
            &mut transitions,
            5,
            selected_object_create(state, object_output, grant_output),
        );
        push_strategy(&mut transitions, 4, selected_object_read(state));
        push_strategy(&mut transitions, 4, selected_object_graph_read(state));
        push_strategy(&mut transitions, 3, selected_object_backlinks_read(state));
        push_strategy(&mut transitions, 2, selected_object_events_read(state));
        push_strategy(&mut transitions, 3, selected_object_version_read(state));
        push_strategy(&mut transitions, 3, selected_object_grants_read(state));
        push_strategy(&mut transitions, 3, selected_object_edges_read(state));
        push_strategy(&mut transitions, 2, selected_workspace_pin(state));
        push_strategy(&mut transitions, 2, selected_workspace_unpin(state));
        push_strategy(&mut transitions, 2, selected_object_pin(state));
        push_strategy(&mut transitions, 2, selected_object_unpin(state));
        push_strategy(&mut transitions, 2, selected_object_favorite(state));
        push_strategy(&mut transitions, 2, selected_object_unfavorite(state));
        push_strategy(
            &mut transitions,
            4,
            selected_comment_thread_create(state, comment_thread_output, comment_output),
        );
        push_strategy(&mut transitions, 3, selected_comment_reply(state, comment_output));
        push_strategy(&mut transitions, 2, selected_comment_edit(state));
        push_strategy(&mut transitions, 2, selected_comment_delete(state));
        push_strategy(&mut transitions, 2, selected_comment_resolve(state));
        push_strategy(&mut transitions, 2, selected_comment_reopen(state));
        push_strategy(&mut transitions, 2, selected_thread_comments_read(state));
        push_strategy(&mut transitions, 2, selected_mention_candidates_read(state));
        push_strategy(&mut transitions, 1, selected_comment_mention_probe(state));
        push_strategy(&mut transitions, 2, selected_notification_inbox_probe(state));
        push_strategy(&mut transitions, 2, selected_user_disable_enable_probe(state));
        transitions
            .push((1, actor().prop_map(|actor| Operation::ProbeAuthLifecycle { actor }).boxed()));
        push_strategy(&mut transitions, 2, selected_unauthorized_group_mutation_probe(state));
        push_strategy(&mut transitions, 3, selected_unauthorized_workspace_mutation_probe(state));
        push_strategy(&mut transitions, 3, selected_unauthorized_object_mutation_probe(state));
        push_strategy(&mut transitions, 2, selected_unauthorized_object_create_probe(state));
        push_strategy(&mut transitions, 2, selected_wikilink_reresolution_probe(state));
        push_strategy(&mut transitions, 2, selected_archived_workspace_object_write_probe(state));
        push_strategy(&mut transitions, 2, selected_archived_workspace_object_restore_probe(state));
        push_strategy(&mut transitions, 4, selected_attachment_upload(state, attachment_output));
        push_strategy(&mut transitions, 2, selected_attachment_list(state));
        push_strategy(&mut transitions, 2, selected_attachment_read(state, false));
        push_strategy(&mut transitions, 2, selected_attachment_read(state, true));
        push_strategy(&mut transitions, 3, selected_attachment_reuse(state, attachment_output));
        push_strategy(&mut transitions, 3, selected_object_update(state));
        push_strategy(&mut transitions, 4, selected_grant_create(state, grant_output));
        push_strategy(&mut transitions, 4, selected_group_grant_create(state, grant_output));
        push_strategy(&mut transitions, 3, selected_grant_revoke(state));
        push_strategy(&mut transitions, 3, selected_grant_update(state, grant_output));
        push_strategy(
            &mut transitions,
            2,
            selected_object_mutation(
                state.administrable_active_objects(),
                |actor, workspace, object| Operation::ArchiveObject { actor, workspace, object },
            ),
        );
        push_strategy(
            &mut transitions,
            2,
            selected_object_mutation(
                state.administrable_archived_objects(),
                |actor, workspace, object| Operation::UnarchiveObject { actor, workspace, object },
            ),
        );
        push_strategy(&mut transitions, 6, selected_edge_create(state, edge_output));
        push_strategy(&mut transitions, 3, selected_edge_read(state));
        push_strategy(&mut transitions, 3, selected_edge_revoke(state));

        Union::new_weighted(transitions).boxed()
    }

    fn apply(mut state: Self::State, transition: &Self::Transition) -> Self::State {
        match transition {
            Operation::CreateWorkspace { actor, output, name } => {
                state.next_workspace = state.next_workspace.max(output.index.saturating_add(1));
                state.workspaces.insert(*output, (*actor, Lifecycle::Active));
                state.workspace_names.insert(*output, name.clone());
                state.record_event(
                    ModeledEvent::new("workspace.created", *actor).workspace(*output),
                );
            }
            Operation::CreateGroup { actor, output, name } => {
                state.next_group = state.next_group.max(output.index.saturating_add(1));
                state.groups.insert(*output, Lifecycle::Active);
                state.group_names.insert(*output, name.clone());
                state.record_event(ModeledEvent::new("group.created", *actor).group(*output));
            }
            Operation::UpdateGroup { actor, group, name } => {
                state.group_names.insert(*group, name.clone());
                state.record_event(ModeledEvent::new("group.updated", *actor).group(*group));
            }
            Operation::ArchiveGroup { actor, group } => {
                state.groups.insert(*group, Lifecycle::Archived);
                state.record_event(ModeledEvent::new("group.archived", *actor).group(*group));
            }
            Operation::UnarchiveGroup { actor, group } => {
                state.groups.insert(*group, Lifecycle::Active);
                state.record_event(ModeledEvent::new("group.unarchived", *actor).group(*group));
            }
            Operation::CreateGroupMembership { actor, group, member, role, output } => {
                state.next_membership = state.next_membership.max(output.index.saturating_add(1));
                state.group_memberships.insert(*output, (*group, *member, *role, true));
                state.record_event(
                    ModeledEvent::new("group.membership_created", *actor)
                        .group(*group)
                        .target_user(*member),
                );
            }
            Operation::RevokeGroupMembership { actor, group, membership } => {
                let member =
                    state.group_memberships.get(membership).map(|(_, member, _, _)| *member);
                if let Some((_, _, _, active)) = state.group_memberships.get_mut(membership) {
                    *active = false;
                }
                if let Some(member) = member {
                    state.record_event(
                        ModeledEvent::new("group.membership_revoked", *actor)
                            .group(*group)
                            .target_user(member),
                    );
                }
            }
            Operation::UpdateGroupMembership { actor, group, membership, role, output } => {
                let Some((membership_group, member, _, active)) =
                    state.group_memberships.get(membership).copied()
                else {
                    return state;
                };
                if active {
                    if let Some((_, _, _, current_active)) =
                        state.group_memberships.get_mut(membership)
                    {
                        *current_active = false;
                    }
                    state.next_membership =
                        state.next_membership.max(output.index.saturating_add(1));
                    state
                        .group_memberships
                        .insert(*output, (membership_group, member, *role, true));
                    state.record_event(
                        ModeledEvent::new("group.membership_updated", *actor)
                            .group(*group)
                            .target_user(member),
                    );
                }
            }
            Operation::LinkWorkspaceGroup { actor, workspace, group } => {
                state.workspace_groups.insert((*workspace, *group), Lifecycle::Active);
                state.record_event(
                    ModeledEvent::new("workspace.group_linked", *actor)
                        .workspace(*workspace)
                        .group(*group),
                );
            }
            Operation::UnarchiveWorkspaceGroup { actor, workspace, group } => {
                state.workspace_groups.insert((*workspace, *group), Lifecycle::Active);
                state.record_event(
                    ModeledEvent::new("workspace.group_unarchived", *actor)
                        .workspace(*workspace)
                        .group(*group),
                );
            }
            Operation::ArchiveWorkspaceGroup { actor, workspace, group } => {
                state.workspace_groups.insert((*workspace, *group), Lifecycle::Archived);
                state.record_event(
                    ModeledEvent::new("workspace.group_archived", *actor)
                        .workspace(*workspace)
                        .group(*group),
                );
            }
            Operation::ArchiveWorkspace { actor, workspace } => {
                if let Some((_, lifecycle)) = state.workspaces.get_mut(workspace) {
                    *lifecycle = Lifecycle::Archived;
                }
                state.record_event(
                    ModeledEvent::new("workspace.archived", *actor).workspace(*workspace),
                );
            }
            Operation::UnarchiveWorkspace { actor, workspace } => {
                if let Some((_, lifecycle)) = state.workspaces.get_mut(workspace) {
                    *lifecycle = Lifecycle::Active;
                }
                state.record_event(
                    ModeledEvent::new("workspace.unarchived", *actor).workspace(*workspace),
                );
            }
            Operation::UpdateWorkspace { actor, workspace, name } => {
                state.workspace_names.insert(*workspace, name.clone());
                state.record_event(
                    ModeledEvent::new("workspace.updated", *actor).workspace(*workspace),
                );
            }
            Operation::CreateWorkspaceMembership { actor, workspace, member, role, output } => {
                state.next_membership = state.next_membership.max(output.index.saturating_add(1));
                state.memberships.insert(*output, (*workspace, *member, *role, true));
                state.record_event(
                    ModeledEvent::new("workspace.membership_created", *actor)
                        .workspace(*workspace)
                        .target_user(*member),
                );
            }
            Operation::RevokeWorkspaceMembership { actor, workspace, membership } => {
                let member = state.memberships.get(membership).map(|(_, member, _, _)| *member);
                if let Some((_, _, _, active)) = state.memberships.get_mut(membership) {
                    *active = false;
                }
                if let Some(member) = member {
                    state.record_event(
                        ModeledEvent::new("workspace.membership_revoked", *actor)
                            .workspace(*workspace)
                            .target_user(member),
                    );
                }
            }
            Operation::UpdateWorkspaceMembership { actor, workspace, membership, role, output } => {
                let Some((membership_workspace, member, _, active)) =
                    state.memberships.get(membership).copied()
                else {
                    return state;
                };
                if active {
                    if let Some((_, _, _, current_active)) = state.memberships.get_mut(membership) {
                        *current_active = false;
                    }
                    state.next_membership =
                        state.next_membership.max(output.index.saturating_add(1));
                    state.memberships.insert(*output, (membership_workspace, member, *role, true));
                    state.record_event(
                        ModeledEvent::new("workspace.membership_updated", *actor)
                            .workspace(*workspace)
                            .target_user(member),
                    );
                }
            }
            Operation::CreateObjectGrant { actor, workspace, object, principal, role, output } => {
                state.next_grant = state.next_grant.max(output.index.saturating_add(1));
                state.grants.insert(
                    *output,
                    (*workspace, *object, Principal::User(*principal), *role, true),
                );
                state.record_event(
                    ModeledEvent::new("object_grant.created", *actor)
                        .workspace(*workspace)
                        .object(*object)
                        .object_grant(*output)
                        .target_user(*principal),
                );
            }
            Operation::CreateGroupObjectGrant {
                actor,
                workspace,
                object,
                principal,
                role,
                output,
            } => {
                state.next_grant = state.next_grant.max(output.index.saturating_add(1));
                state.grants.insert(
                    *output,
                    (*workspace, *object, Principal::Group(*principal), *role, true),
                );
                state.record_event(
                    ModeledEvent::new("object_grant.created", *actor)
                        .workspace(*workspace)
                        .object(*object)
                        .object_grant(*output)
                        .group(*principal),
                );
            }
            Operation::RevokeObjectGrant { actor, workspace, object, grant } => {
                let allowed = state.can_revoke_grant(*grant);
                if allowed {
                    let principal =
                        state.grants.get(grant).map(|(_, _, principal, _, _)| *principal);
                    if let Some((_, _, _, _, active)) = state.grants.get_mut(grant) {
                        *active = false;
                    }
                    if let Some(principal) = principal {
                        let mut event = ModeledEvent::new("object_grant.revoked", *actor)
                            .workspace(*workspace)
                            .object(*object)
                            .object_grant(*grant);
                        event = match principal {
                            Principal::User(user) => event.target_user(user),
                            Principal::Group(group) => event.group(group),
                        };
                        state.record_event(event);
                    }
                }
            }
            Operation::UpdateObjectGrant { actor, workspace, object, grant, role, output } => {
                let allowed = state.can_update_grant(*grant, *role);
                if allowed {
                    let Some((grant_workspace, grant_object, principal, _, _)) =
                        state.grants.get(grant).copied()
                    else {
                        return state;
                    };
                    if let Some((_, _, _, _, active)) = state.grants.get_mut(grant) {
                        *active = false;
                    }
                    state.next_grant = state.next_grant.max(output.index.saturating_add(1));
                    state
                        .grants
                        .insert(*output, (grant_workspace, grant_object, principal, *role, true));
                    let mut event = ModeledEvent::new("object_grant.updated", *actor)
                        .workspace(*workspace)
                        .object(*object)
                        .object_grant(*output);
                    event = match principal {
                        Principal::User(user) => event.target_user(user),
                        Principal::Group(group) => event.group(group),
                    };
                    state.record_event(event);
                }
            }
            Operation::CreateApiKey { actor, workspace, output, scope } => {
                state.next_api_key = state.next_api_key.max(output.index.saturating_add(1));
                state.api_keys.insert(
                    *output,
                    super::ModeledApiKey {
                        owner: *actor,
                        workspace: *workspace,
                        scope: *scope,
                        revision: 0,
                        active: true,
                    },
                );
            }
            Operation::UpdateApiKey { key, scope, .. } => {
                if let Some(modeled) = state.api_keys.get_mut(key) {
                    modeled.scope = *scope;
                    modeled.revision = modeled.revision.saturating_add(1);
                }
            }
            Operation::RevokeApiKey { key, .. } => {
                if let Some(modeled) = state.api_keys.get_mut(key) {
                    modeled.active = false;
                }
            }
            Operation::CreateObject { actor, workspace, output, creator_grant, title } => {
                state.next_object = state.next_object.max(output.index.saturating_add(1));
                state.next_grant = state.next_grant.max(creator_grant.index.saturating_add(1));
                state
                    .objects
                    .insert(*output, (*workspace, *actor, Lifecycle::Active, title.clone(), 1));
                state.object_version_authors.insert((*output, 1), *actor);
                state.grants.insert(
                    *creator_grant,
                    (*workspace, *output, Principal::User(*actor), ObjectRole::Admin, true),
                );
                state.record_event(
                    ModeledEvent::new("object.created", *actor)
                        .workspace(*workspace)
                        .object(*output),
                );
                state.record_event(
                    ModeledEvent::new("object_grant.created", *actor)
                        .workspace(*workspace)
                        .object(*output)
                        .object_grant(*creator_grant)
                        .target_user(*actor),
                );
            }
            Operation::CreateCommentThread {
                actor,
                workspace,
                object,
                thread_output,
                comment_output,
                body,
            } => {
                state.next_comment_thread =
                    state.next_comment_thread.max(thread_output.index.saturating_add(1));
                state.next_comment = state.next_comment.max(comment_output.index.saturating_add(1));
                state.comment_threads.insert(
                    *thread_output,
                    super::ModeledCommentThread {
                        workspace: *workspace,
                        object: *object,
                        author: *actor,
                        root: *comment_output,
                        resolved: false,
                    },
                );
                state.comments.insert(
                    *comment_output,
                    super::ModeledComment {
                        thread: *thread_output,
                        author: *actor,
                        body: Some(body.clone()),
                    },
                );
                state.record_event(
                    ModeledEvent::new("comment.created", *actor)
                        .workspace(*workspace)
                        .object(*object),
                );
            }
            Operation::ReplyComment { actor, workspace, object, thread, output, body } => {
                let root_author = state.comment_thread(*thread).map(|thread| thread.author);
                state.next_comment = state.next_comment.max(output.index.saturating_add(1));
                state.comments.insert(
                    *output,
                    super::ModeledComment {
                        thread: *thread,
                        author: *actor,
                        body: Some(body.clone()),
                    },
                );
                let mut event = ModeledEvent::new("comment.replied", *actor)
                    .workspace(*workspace)
                    .object(*object);
                if let Some(root_author) = root_author {
                    event = event.target_user(root_author);
                }
                state.record_event(event);
            }
            Operation::EditComment { actor, workspace, object, comment, body } => {
                if let Some(modeled) = state.comments.get_mut(comment) {
                    modeled.body = Some(body.clone());
                }
                state.record_event(
                    ModeledEvent::new("comment.edited", *actor)
                        .workspace(*workspace)
                        .object(*object),
                );
            }
            Operation::DeleteComment { actor, workspace, object, comment } => {
                if let Some(modeled) = state.comments.get_mut(comment) {
                    modeled.body = None;
                }
                state.record_event(
                    ModeledEvent::new("comment.deleted", *actor)
                        .workspace(*workspace)
                        .object(*object),
                );
            }
            Operation::ResolveCommentThread { actor, workspace, object, thread } => {
                if let Some(modeled) = state.comment_threads.get_mut(thread) {
                    modeled.resolved = true;
                }
                state.record_event(
                    ModeledEvent::new("comment_thread.resolved", *actor)
                        .workspace(*workspace)
                        .object(*object),
                );
            }
            Operation::ReopenCommentThread { actor, workspace, object, thread } => {
                if let Some(modeled) = state.comment_threads.get_mut(thread) {
                    modeled.resolved = false;
                }
                state.record_event(
                    ModeledEvent::new("comment_thread.reopened", *actor)
                        .workspace(*workspace)
                        .object(*object),
                );
            }
            Operation::ProbeCommentMentions { actor, workspace, object, .. } => {
                state.record_event(
                    ModeledEvent::new("comment.created", *actor)
                        .workspace(*workspace)
                        .object(*object),
                );
                state.record_event(
                    ModeledEvent::new("comment.edited", *actor)
                        .workspace(*workspace)
                        .object(*object),
                );
            }
            Operation::ProbeNotificationInbox { workspace, object, .. } => {
                state.record_event(
                    ModeledEvent::new("comment.created", Actor::Admin)
                        .workspace(*workspace)
                        .object(*object),
                );
            }
            Operation::ProbeUserDisableEnable { actor, target, .. } => {
                state.record_event(ModeledEvent::new("user.disabled", *actor).target_user(*target));
                state.record_event(ModeledEvent::new("user.enabled", *actor).target_user(*target));
            }
            Operation::PinWorkspace { actor, workspace } => {
                state.workspace_pins.insert((*actor, *workspace));
            }
            Operation::UnpinWorkspace { actor, workspace } => {
                state.workspace_pins.remove(&(*actor, *workspace));
            }
            Operation::PinObject { actor, object, .. } => {
                state.object_pins.insert((*actor, *object));
            }
            Operation::UnpinObject { actor, object, .. } => {
                state.object_pins.remove(&(*actor, *object));
            }
            Operation::FavoriteObject { actor, object, .. } => {
                state.object_favorites.insert((*actor, *object));
            }
            Operation::UnfavoriteObject { actor, object, .. } => {
                state.object_favorites.remove(&(*actor, *object));
            }
            Operation::CheckWhoAmI { .. }
            | Operation::GetGroup { .. }
            | Operation::GetObject { .. }
            | Operation::GetObjectAttachment { .. }
            | Operation::GetObjectAttachmentContent { .. }
            | Operation::GetObjectBacklinks { .. }
            | Operation::GetObjectEdge { .. }
            | Operation::GetObjectEvents { .. }
            | Operation::GetObjectGraph { .. }
            | Operation::GetObjectVersion { .. }
            | Operation::GetWorkspace { .. }
            | Operation::GetWorkspaceEvents { .. }
            | Operation::GetWorkspaceGraph { .. }
            | Operation::ListGroupMemberships { .. }
            | Operation::ListGroups { .. }
            | Operation::ListObjectAttachments { .. }
            | Operation::ListObjectGrants { .. }
            | Operation::ListObjectEdges { .. }
            | Operation::ListThreadComments { .. }
            | Operation::ListMentionCandidates { .. }
            | Operation::ProbeApiKeyAccess { .. }
            | Operation::ProbeAuthLifecycle { .. }
            | Operation::ListWorkspaceMemberships { .. }
            | Operation::ListWorkspaceGroups { .. }
            | Operation::ListWorkspaces { .. }
            | Operation::ProbeArchivedGroupMembershipWrites { .. }
            | Operation::ProbeArchivedWorkspaceObjectRestore { .. }
            | Operation::ProbeArchivedWorkspaceObjectWrites { .. }
            | Operation::ProbeUnauthorizedGroupMutations { .. }
            | Operation::ProbeUnauthorizedWorkspaceMutations { .. }
            | Operation::ProbeUnauthorizedObjectMutations { .. }
            | Operation::ProbeUnauthorizedObjectCreate { .. }
            | Operation::SearchWorkspace { .. } => {}
            Operation::ProbeWikilinkReresolution { actor, workspace, source, target, .. } => {
                let source_version = state.object_version(*source).expect("modeled source version");
                let target_version = state.object_version(*target).expect("modeled target version");
                for version in 1..=3 {
                    state
                        .object_version_authors
                        .insert((*source, source_version + version), *actor);
                }
                for version in 1..=2 {
                    state
                        .object_version_authors
                        .insert((*target, target_version + version), *actor);
                }
                if let Some((_, _, _, _, version)) = state.objects.get_mut(source) {
                    *version += 3;
                }
                if let Some((_, _, _, _, version)) = state.objects.get_mut(target) {
                    *version += 2;
                }
                for object in [*source, *target, *source, *source, *target] {
                    state.record_event(
                        ModeledEvent::new("object.version_appended", *actor)
                            .workspace(*workspace)
                            .object(object),
                    );
                    state.record_event(
                        ModeledEvent::new("object.updated", *actor)
                            .workspace(*workspace)
                            .object(object),
                    );
                }
            }
            Operation::UploadObjectAttachment {
                actor,
                workspace,
                object,
                output,
                name,
                content,
            } => {
                state.next_attachment = state.next_attachment.max(output.index.saturating_add(1));
                state.attachments.insert(
                    *output,
                    ModeledAttachment {
                        workspace: *workspace,
                        object: *object,
                        source: None,
                        name: name.clone(),
                        content: content.clone(),
                    },
                );
                state.record_event(
                    ModeledEvent::new("object.attachment_created", *actor)
                        .workspace(*workspace)
                        .object(*object),
                );
            }
            Operation::ReuseObjectAttachment { actor, workspace, object, source, output } => {
                let Some(source_attachment) = state.attachment(*source) else {
                    return state;
                };
                let name = source_attachment.name.clone();
                let content = source_attachment.content.clone();
                state.next_attachment = state.next_attachment.max(output.index.saturating_add(1));
                state.attachments.insert(
                    *output,
                    ModeledAttachment {
                        workspace: *workspace,
                        object: *object,
                        source: Some(*source),
                        name,
                        content,
                    },
                );
                state.record_event(
                    ModeledEvent::new("object.attachment_created", *actor)
                        .workspace(*workspace)
                        .object(*object),
                );
            }
            Operation::UpdateObject { actor, workspace, object, title } => {
                let allowed = state.can_edit_object(*object, *actor);
                if allowed
                    && let Some((_, _, _, current_title, version)) = state.objects.get_mut(object)
                {
                    *current_title = title.clone();
                    *version = version.saturating_add(1);
                    state.object_version_authors.insert((*object, *version), *actor);
                    state.record_event(
                        ModeledEvent::new("object.version_appended", *actor)
                            .workspace(*workspace)
                            .object(*object),
                    );
                    state.record_event(
                        ModeledEvent::new("object.updated", *actor)
                            .workspace(*workspace)
                            .object(*object),
                    );
                }
            }
            Operation::ArchiveObject { actor, workspace, object } => {
                if let Some((_, _, lifecycle, _, _)) = state.objects.get_mut(object) {
                    *lifecycle = Lifecycle::Archived;
                }
                state.record_event(
                    ModeledEvent::new("object.archived", *actor)
                        .workspace(*workspace)
                        .object(*object),
                );
            }
            Operation::UnarchiveObject { actor, workspace, object } => {
                if let Some((_, _, lifecycle, _, _)) = state.objects.get_mut(object) {
                    *lifecycle = Lifecycle::Active;
                }
                state.record_event(
                    ModeledEvent::new("object.unarchived", *actor)
                        .workspace(*workspace)
                        .object(*object),
                );
            }
            Operation::CreateObjectEdge { actor, workspace, source, target, output, .. } => {
                state.next_edge = state.next_edge.max(output.index.saturating_add(1));
                state.edges.insert(*output, (*workspace, *source, *target, true));
                state.record_event(
                    ModeledEvent::new("object_edge.created", *actor)
                        .workspace(*workspace)
                        .object(*source)
                        .object_edge(*output),
                );
            }
            Operation::RevokeObjectEdge { actor, workspace, edge } => {
                let source = state.edge(*edge).map(|(_, source, _, _)| source);
                if let Some((_, _, _, active)) = state.edges.get_mut(edge) {
                    *active = false;
                }
                if let Some(source) = source {
                    state.record_event(
                        ModeledEvent::new("object_edge.revoked", *actor)
                            .workspace(*workspace)
                            .object(source)
                            .object_edge(*edge),
                    );
                }
            }
        }
        state
    }

    fn preconditions(state: &Self::State, transition: &Self::Transition) -> bool {
        match transition {
            Operation::CheckWhoAmI { .. }
            | Operation::ListWorkspaces { .. }
            | Operation::ListGroups { .. }
            | Operation::ProbeAuthLifecycle { .. } => true,
            Operation::CreateWorkspace { output, .. } => {
                output.kind == ResourceKind::Workspace && state.workspace(*output).is_none()
            }
            Operation::CreateGroup { actor, output, .. } => {
                *actor == Actor::Admin
                    && output.kind == ResourceKind::Group
                    && !state.groups.contains_key(output)
            }
            Operation::GetGroup { group, .. } | Operation::ListGroupMemberships { group, .. } => {
                state.group(*group).is_some()
            }
            Operation::UpdateGroup { actor, group, .. }
            | Operation::ArchiveGroup { actor, group } => {
                *actor == Actor::Admin && state.group(*group) == Some(Lifecycle::Active)
            }
            Operation::UnarchiveGroup { actor, group } => {
                *actor == Actor::Admin && state.group(*group) == Some(Lifecycle::Archived)
            }
            Operation::CreateGroupMembership { actor, group, member, output, .. } => {
                *member != Actor::Admin
                    && state.can_admin_group(*group, *actor)
                    && !state.has_active_group_membership(*group, *member)
                    && output.kind == ResourceKind::Membership
                    && !state.group_memberships.contains_key(output)
            }
            Operation::RevokeGroupMembership { actor, group, membership } => {
                state.can_admin_group(*group, *actor)
                    && state.group_memberships.get(membership).is_some_and(
                        |(member_group, _, _, active)| *member_group == *group && *active,
                    )
            }
            Operation::UpdateGroupMembership { actor, group, membership, output, .. } => {
                state.can_admin_group(*group, *actor)
                    && state.group_memberships.get(membership).is_some_and(
                        |(member_group, _, _, active)| *member_group == *group && *active,
                    )
                    && output.kind == ResourceKind::Membership
                    && !state.group_memberships.contains_key(output)
                    && !state.memberships.contains_key(output)
            }
            Operation::ProbeArchivedGroupMembershipWrites { actor, group, membership, member } => {
                *actor == Actor::Admin
                    && *member != Actor::Admin
                    && state.group(*group) == Some(Lifecycle::Archived)
                    && state.group_membership(*membership).is_some_and(
                        |(member_group, _, _, active)| member_group == *group && active,
                    )
                    && !state.group_memberships.values().any(
                        |(member_group, existing_member, _, active)| {
                            *member_group == *group && *existing_member == *member && *active
                        },
                    )
            }
            Operation::LinkWorkspaceGroup { actor, workspace, group } => {
                state.can_admin_workspace(*workspace, *actor)
                    && state.groups.get(group) == Some(&Lifecycle::Active)
                    && !state.workspace_groups.contains_key(&(*workspace, *group))
            }
            Operation::ArchiveWorkspaceGroup { actor, workspace, group } => {
                state.can_admin_workspace(*workspace, *actor)
                    && state.workspace_groups.get(&(*workspace, *group)) == Some(&Lifecycle::Active)
            }
            Operation::UnarchiveWorkspaceGroup { actor, workspace, group } => {
                state.can_admin_workspace(*workspace, *actor)
                    && state.groups.get(group) == Some(&Lifecycle::Active)
                    && state.workspace_groups.get(&(*workspace, *group))
                        == Some(&Lifecycle::Archived)
            }
            Operation::GetWorkspace { workspace, .. } => state.workspace(*workspace).is_some(),
            Operation::GetWorkspaceGraph { workspace, .. }
            | Operation::GetWorkspaceEvents { workspace, .. }
            | Operation::ListWorkspaceMemberships { workspace, .. }
            | Operation::ListWorkspaceGroups { workspace, .. } => {
                state.workspace(*workspace).is_some()
            }
            Operation::UpdateWorkspace { actor, workspace, .. }
            | Operation::ArchiveWorkspace { actor, workspace } => {
                state.can_admin_workspace(*workspace, *actor)
            }
            Operation::SearchWorkspace { workspace, object, .. } => state
                .object(*object)
                .is_some_and(|(object_workspace, _)| object_workspace == *workspace),
            Operation::UnarchiveWorkspace { actor, workspace } => {
                state.can_restore_workspace(*workspace, *actor)
            }
            Operation::CreateWorkspaceMembership { actor, workspace, member, output, .. } => {
                state.can_admin_workspace(*workspace, *actor)
                    && *member != Actor::Admin
                    && !state.has_workspace_membership(*workspace, *member)
                    && output.kind == ResourceKind::Membership
                    && !state.memberships.contains_key(output)
            }
            Operation::RevokeWorkspaceMembership { actor, workspace, membership } => {
                state.can_admin_workspace(*workspace, *actor)
                    && state.memberships.get(membership).is_some_and(
                        |(member_workspace, _, _, active)| {
                            *member_workspace == *workspace && *active
                        },
                    )
            }
            Operation::UpdateWorkspaceMembership {
                actor, workspace, membership, output, ..
            } => {
                state.can_admin_workspace(*workspace, *actor)
                    && state.memberships.get(membership).is_some_and(
                        |(member_workspace, _, _, active)| {
                            *member_workspace == *workspace && *active
                        },
                    )
                    && output.kind == ResourceKind::Membership
                    && !state.memberships.contains_key(output)
                    && !state.group_memberships.contains_key(output)
            }
            Operation::CreateObjectGrant {
                actor, workspace, object, principal, output, ..
            } => {
                state.object(*object) == Some((*workspace, Lifecycle::Active))
                    && state.can_admin_object(*object, *actor)
                    && *principal != Actor::Admin
                    && state.has_workspace_membership(*workspace, *principal)
                    && state.object_creator(*object) != Some(*principal)
                    && output.kind == ResourceKind::Grant
                    && !state.grants.contains_key(output)
            }
            Operation::CreateGroupObjectGrant {
                actor,
                workspace,
                object,
                principal,
                output,
                ..
            } => {
                state.object(*object) == Some((*workspace, Lifecycle::Active))
                    && state.can_admin_object(*object, *actor)
                    && state.group(*principal) == Some(Lifecycle::Active)
                    && state.workspace_groups.get(&(*workspace, *principal))
                        == Some(&Lifecycle::Active)
                    && output.kind == ResourceKind::Grant
                    && !state.grants.contains_key(output)
            }
            Operation::RevokeObjectGrant { actor, workspace, object, grant } => {
                state.object(*object) == Some((*workspace, Lifecycle::Active))
                    && state.can_admin_object(*object, *actor)
                    && state.grants.get(grant).is_some_and(
                        |(grant_workspace, grant_object, _, _, active)| {
                            *grant_workspace == *workspace && *grant_object == *object && *active
                        },
                    )
            }
            Operation::UpdateObjectGrant { actor, workspace, object, grant, output, .. } => {
                state.object(*object) == Some((*workspace, Lifecycle::Active))
                    && state.can_admin_object(*object, *actor)
                    && state.grants.get(grant).is_some_and(
                        |(grant_workspace, grant_object, _, _, active)| {
                            *grant_workspace == *workspace && *grant_object == *object && *active
                        },
                    )
                    && output.kind == ResourceKind::Grant
                    && !state.grants.contains_key(output)
            }
            Operation::CreateApiKey { actor, workspace, output, scope } => {
                state.can_use_workspace(*workspace, *actor)
                    && matches!(scope, ApiKeyScope::WorkspaceRead | ApiKeyScope::ObjectRead)
                    && output.kind == ResourceKind::ApiKey
                    && state.api_key(*output).is_none()
            }
            Operation::UpdateApiKey { actor, key, scope } => {
                state.api_key(*key).is_some_and(|modeled| {
                    modeled.active
                        && modeled.owner == *actor
                        && state.can_use_workspace(modeled.workspace, *actor)
                        && modeled.scope != *scope
                }) && matches!(scope, ApiKeyScope::WorkspaceRead | ApiKeyScope::ObjectRead)
            }
            Operation::RevokeApiKey { actor, key } => {
                state.api_key(*key).is_some_and(|modeled| modeled.active && modeled.owner == *actor)
            }
            Operation::ProbeApiKeyAccess { actor, key, workspace, object } => {
                state.api_key(*key).is_some_and(|modeled| {
                    modeled.owner == *actor
                        && modeled.workspace == *workspace
                        && state.object(*object).is_some_and(|(parent, _)| parent == *workspace)
                })
            }
            Operation::CreateObject { actor, workspace, output, creator_grant, .. } => {
                state.can_use_workspace(*workspace, *actor)
                    && output.kind == ResourceKind::Object
                    && state.object(*output).is_none()
                    && creator_grant.kind == ResourceKind::Grant
                    && !state.grants.contains_key(creator_grant)
            }
            Operation::GetObject { workspace, object, .. } => {
                state.object(*object).is_some_and(|(parent, _)| parent == *workspace)
            }
            Operation::GetObjectGraph { workspace, object, .. }
            | Operation::GetObjectBacklinks { workspace, object, .. }
            | Operation::GetObjectEvents { workspace, object, .. }
            | Operation::GetObjectVersion { workspace, object, .. }
            | Operation::ListObjectAttachments { workspace, object, .. }
            | Operation::ListObjectGrants { workspace, object, .. }
            | Operation::ListObjectEdges { workspace, object, .. } => {
                state.object(*object).is_some_and(|(parent, _)| parent == *workspace)
            }
            Operation::PinWorkspace { actor, workspace } => {
                state.can_use_workspace(*workspace, *actor)
                    && !state.workspace_pinned(*workspace, *actor)
            }
            Operation::UnpinWorkspace { actor, workspace } => {
                state.workspace(*workspace).is_some() && state.workspace_pinned(*workspace, *actor)
            }
            Operation::PinObject { actor, workspace, object } => {
                state.object(*object).is_some_and(|(parent, _)| parent == *workspace)
                    && state.can_read_object(*object, *actor)
                    && !state.object_pinned(*object, *actor)
            }
            Operation::UnpinObject { actor, workspace, object } => {
                state.object(*object).is_some_and(|(parent, _)| parent == *workspace)
                    && state.object_pinned(*object, *actor)
            }
            Operation::FavoriteObject { actor, workspace, object } => {
                state.object(*object).is_some_and(|(parent, _)| parent == *workspace)
                    && state.can_read_object(*object, *actor)
                    && !state.object_favorited(*object, *actor)
            }
            Operation::UnfavoriteObject { actor, workspace, object } => {
                state.object(*object).is_some_and(|(parent, _)| parent == *workspace)
                    && state.object_favorited(*object, *actor)
            }
            Operation::CreateCommentThread {
                actor,
                workspace,
                object,
                thread_output,
                comment_output,
                ..
            } => {
                state.object(*object) == Some((*workspace, Lifecycle::Active))
                    && state.can_read_object(*object, *actor)
                    && thread_output.kind == ResourceKind::CommentThread
                    && comment_output.kind == ResourceKind::Comment
                    && state.comment_thread(*thread_output).is_none()
                    && state.comment(*comment_output).is_none()
            }
            Operation::ReplyComment { actor, workspace, object, thread, output, .. } => {
                state.comment_thread(*thread).is_some_and(|modeled| {
                    modeled.workspace == *workspace
                        && modeled.object == *object
                        && !modeled.resolved
                        && state.object(*object) == Some((*workspace, Lifecycle::Active))
                        && state.can_read_object(*object, *actor)
                }) && output.kind == ResourceKind::Comment
                    && state.comment(*output).is_none()
            }
            Operation::EditComment { actor, workspace, object, comment, .. } => {
                state.comment(*comment).is_some_and(|modeled| {
                    let Some(thread) = state.comment_thread(modeled.thread) else {
                        return false;
                    };
                    modeled.author == *actor
                        && modeled.body.is_some()
                        && !thread.resolved
                        && thread.workspace == *workspace
                        && thread.object == *object
                        && state.object(*object) == Some((*workspace, Lifecycle::Active))
                        && state.can_read_object(*object, *actor)
                })
            }
            Operation::DeleteComment { actor, workspace, object, comment } => {
                state.comment(*comment).is_some_and(|modeled| {
                    let Some(thread) = state.comment_thread(modeled.thread) else {
                        return false;
                    };
                    modeled.body.is_some()
                        && thread.workspace == *workspace
                        && thread.object == *object
                        && state.object(*object) == Some((*workspace, Lifecycle::Active))
                        && state.can_read_object(*object, *actor)
                        && (*actor == modeled.author
                            || state.object_role(*object, *actor) == Some(ObjectRole::Admin))
                })
            }
            Operation::ResolveCommentThread { actor, workspace, object, thread } => {
                state.comment_thread(*thread).is_some_and(|modeled| {
                    !modeled.resolved
                        && modeled.workspace == *workspace
                        && modeled.object == *object
                        && state.object(*object) == Some((*workspace, Lifecycle::Active))
                        && state.can_read_object(*object, *actor)
                        && (*actor == modeled.author
                            || state.object_role(*object, *actor) == Some(ObjectRole::Admin))
                })
            }
            Operation::ReopenCommentThread { actor, workspace, object, thread } => {
                state.comment_thread(*thread).is_some_and(|modeled| {
                    modeled.resolved
                        && modeled.workspace == *workspace
                        && modeled.object == *object
                        && state.object(*object) == Some((*workspace, Lifecycle::Active))
                        && state.can_read_object(*object, *actor)
                        && (*actor == modeled.author
                            || state.object_role(*object, *actor) == Some(ObjectRole::Admin))
                })
            }
            Operation::ListThreadComments { workspace, object, thread, .. } => {
                state.comment_thread(*thread).is_some_and(|modeled| {
                    modeled.workspace == *workspace && modeled.object == *object
                })
            }
            Operation::ListMentionCandidates { workspace, object, .. } => {
                state.object(*object).is_some_and(|(parent, _)| parent == *workspace)
            }
            Operation::ProbeCommentMentions {
                actor,
                workspace,
                object,
                first_mention,
                second_mention,
            } => {
                state.object(*object) == Some((*workspace, Lifecycle::Active))
                    && state.can_read_object(*object, *actor)
                    && state.can_read_object(*object, *first_mention)
                    && state.can_read_object(*object, *second_mention)
                    && first_mention != second_mention
            }
            Operation::ProbeWikilinkReresolution { actor, workspace, source, target, .. } => {
                source != target
                    && state.workspace(*workspace) == Some(Lifecycle::Active)
                    && state.object(*source) == Some((*workspace, Lifecycle::Active))
                    && state.object(*target) == Some((*workspace, Lifecycle::Active))
                    && state.can_edit_object(*source, *actor)
                    && state.can_edit_object(*target, *actor)
            }
            Operation::ProbeArchivedWorkspaceObjectWrites { workspace, object, .. } => {
                state.workspace(*workspace) == Some(Lifecycle::Archived)
                    && state.object(*object) == Some((*workspace, Lifecycle::Active))
            }
            Operation::ProbeArchivedWorkspaceObjectRestore { workspace, object, .. } => {
                state.workspace(*workspace) == Some(Lifecycle::Archived)
                    && state.object(*object) == Some((*workspace, Lifecycle::Archived))
            }
            Operation::ProbeNotificationInbox { actor, workspace, object } => {
                *actor != Actor::Admin
                    && state.object(*object) == Some((*workspace, Lifecycle::Active))
                    && state.can_read_object(*object, *actor)
            }
            Operation::ProbeUserDisableEnable { actor, target, workspace, object } => {
                *actor == Actor::Admin
                    && *target != Actor::Admin
                    && state.workspace(*workspace) == Some(Lifecycle::Active)
                    && state.object(*object) == Some((*workspace, Lifecycle::Active))
                    && state.can_read_workspace(*workspace, *target)
                    && state.can_read_object(*object, *target)
            }
            Operation::ProbeUnauthorizedGroupMutations { actor, group, member } => {
                *actor != Actor::Admin
                    && *member != Actor::Admin
                    && state.group(*group) == Some(Lifecycle::Active)
                    && !state.can_admin_group(*group, *actor)
                    && !state.has_active_group_membership(*group, *member)
            }
            Operation::ProbeUnauthorizedWorkspaceMutations { actor, workspace, member } => {
                *member != Actor::Admin
                    && state.workspace(*workspace) == Some(Lifecycle::Active)
                    && state.can_use_workspace(*workspace, *actor)
                    && !state.can_admin_workspace(*workspace, *actor)
                    && !state.has_workspace_membership(*workspace, *member)
            }
            Operation::ProbeUnauthorizedObjectMutations { actor, workspace, object } => {
                state.object(*object) == Some((*workspace, Lifecycle::Active))
                    && state.can_read_object(*object, *actor)
                    && !state.can_edit_object(*object, *actor)
                    && !state.can_admin_object(*object, *actor)
            }
            Operation::ProbeUnauthorizedObjectCreate { actor, workspace } => {
                state.workspace(*workspace) == Some(Lifecycle::Active)
                    && !state.can_use_workspace(*workspace, *actor)
            }
            Operation::UploadObjectAttachment { actor, workspace, object, output, .. } => {
                state.object(*object) == Some((*workspace, Lifecycle::Active))
                    && state.can_edit_object(*object, *actor)
                    && output.kind == ResourceKind::Attachment
                    && !state.attachments.contains_key(output)
            }
            Operation::GetObjectAttachment { workspace, object, attachment, .. }
            | Operation::GetObjectAttachmentContent { workspace, object, attachment, .. } => {
                state.attachment(*attachment).is_some_and(|modeled| {
                    modeled.workspace == *workspace && modeled.object == *object
                })
            }
            Operation::ReuseObjectAttachment { actor, workspace, object, source, output } => {
                state.object(*object) == Some((*workspace, Lifecycle::Active))
                    && state.can_edit_object(*object, *actor)
                    && state.attachment(*source).is_some_and(|modeled| {
                        modeled.workspace == *workspace
                            && state.object(modeled.object) == Some((*workspace, Lifecycle::Active))
                            && state.can_read_object(modeled.object, *actor)
                    })
                    && output.kind == ResourceKind::Attachment
                    && !state.attachments.contains_key(output)
            }
            Operation::UpdateObject { workspace, object, .. } => {
                state.object(*object) == Some((*workspace, Lifecycle::Active))
                    && state.workspace(*workspace) == Some(Lifecycle::Active)
            }
            Operation::ArchiveObject { actor, workspace, object } => {
                state.object(*object) == Some((*workspace, Lifecycle::Active))
                    && state.can_admin_object(*object, *actor)
            }
            Operation::UnarchiveObject { actor, workspace, object } => {
                state.object(*object) == Some((*workspace, Lifecycle::Archived))
                    && state.can_admin_object(*object, *actor)
            }
            Operation::CreateObjectEdge { actor, workspace, source, target, output, .. } => {
                source != target
                    && state.object(*source) == Some((*workspace, Lifecycle::Active))
                    && state.object(*target) == Some((*workspace, Lifecycle::Active))
                    && state.can_edit_object(*source, *actor)
                    && state.can_read_object(*target, *actor)
                    && output.kind == ResourceKind::Edge
                    && !state.edges.contains_key(output)
            }
            Operation::GetObjectEdge { workspace, edge, .. } => {
                state.edge(*edge).is_some_and(|(edge_workspace, _, _, active)| {
                    edge_workspace == *workspace && active
                })
            }
            Operation::RevokeObjectEdge { actor, workspace, edge } => {
                state.edge(*edge).is_some_and(|(edge_workspace, source, target, active)| {
                    edge_workspace == *workspace
                        && active
                        && state.can_edit_object(source, *actor)
                        && state.can_read_object(target, *actor)
                })
            }
        }
    }
}

/// Generates a fixture actor.
fn actor() -> BoxedStrategy<Actor> {
    select(Actor::ALL.to_vec()).boxed()
}

/// Adds an available weighted transition strategy.
fn push_strategy(
    transitions: &mut Vec<(u32, BoxedStrategy<Operation>)>,
    weight: u32,
    strategy: Option<BoxedStrategy<Operation>>,
) {
    if let Some(strategy) = strategy {
        transitions.push((weight, strategy));
    }
}

/// Generates a group read by any actor.
fn selected_group_read(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.groups.keys().copied().collect()).map(|groups| {
        (actor(), select(groups))
            .prop_map(|(actor, group)| Operation::GetGroup { actor, group })
            .boxed()
    })
}

/// Generates a group name update by the global administrator.
fn selected_group_update(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.active_groups()).map(|groups| {
        (select(groups), any::<u32>())
            .prop_map(|(group, suffix)| Operation::UpdateGroup {
                actor: Actor::Admin,
                group,
                name: generated_name("updated-group", group.index, suffix),
            })
            .boxed()
    })
}

/// Generates archival of an active group.
fn selected_group_archive(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.active_groups()).map(|groups| {
        select(groups)
            .prop_map(|group| Operation::ArchiveGroup { actor: Actor::Admin, group })
            .boxed()
    })
}

/// Generates restoration of an archived group.
fn selected_group_unarchive(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.archived_group_admins()).map(|groups| {
        select(groups).prop_map(|(group, actor)| Operation::UnarchiveGroup { actor, group }).boxed()
    })
}

/// Generates a group-membership collection read by any actor.
fn selected_group_memberships_read(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.groups.keys().copied().collect()).map(|groups| {
        (actor(), select(groups))
            .prop_map(|(actor, group)| Operation::ListGroupMemberships { actor, group })
            .boxed()
    })
}

/// Generates a group membership creation.
fn selected_group_membership_create(
    state: &Model,
    output: Handle,
) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.group_membership_candidates()).map(|candidates| {
        (select(candidates), select(vec![MembershipRole::Member, MembershipRole::Admin]))
            .prop_map(move |((group, actor, member), role)| Operation::CreateGroupMembership {
                actor,
                group,
                member,
                role,
                output,
            })
            .boxed()
    })
}

/// Generates a group membership revocation.
fn selected_group_membership_revoke(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.revocable_group_memberships()).map(|memberships| {
        select(memberships)
            .prop_map(|(membership, group, actor)| Operation::RevokeGroupMembership {
                actor,
                group,
                membership,
            })
            .boxed()
    })
}

/// Generates replacement of an active group membership with another role.
fn selected_group_membership_update(
    state: &Model,
    output: Handle,
) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.updatable_group_memberships()).map(|memberships| {
        (select(memberships), select(vec![MembershipRole::Member, MembershipRole::Admin]))
            .prop_map(move |((membership, group, actor), role)| Operation::UpdateGroupMembership {
                actor,
                group,
                membership,
                role,
                output,
            })
            .boxed()
    })
}

/// Generates rejected group membership writes against an archived group.
fn selected_archived_group_membership_write_probe(
    state: &Model,
) -> Option<BoxedStrategy<Operation>> {
    let mut candidates = Vec::new();
    for group in state.groups.keys().copied() {
        if state.group(group) != Some(Lifecycle::Archived) {
            continue;
        }
        for membership in state.active_group_memberships(group) {
            for member in Actor::ALL {
                let already_member = state.group_memberships.values().any(
                    |(member_group, existing_member, _, active)| {
                        *member_group == group && *existing_member == member && *active
                    },
                );
                if member != Actor::Admin && !already_member {
                    candidates.push((group, membership, member));
                }
            }
        }
    }

    non_empty(candidates).map(|candidates| {
        select(candidates)
            .prop_map(|(group, membership, member)| Operation::ProbeArchivedGroupMembershipWrites {
                actor: Actor::Admin,
                group,
                membership,
                member,
            })
            .boxed()
    })
}

/// Generates a workspace-group link.
fn selected_workspace_group_link(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.workspace_group_candidates()).map(|links| {
        select(links)
            .prop_map(|(workspace, group, actor)| Operation::LinkWorkspaceGroup {
                actor,
                workspace,
                group,
            })
            .boxed()
    })
}

/// Generates workspace-group link archival.
fn selected_workspace_group_archive(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.active_workspace_groups()).map(|links| {
        select(links)
            .prop_map(|(workspace, group, actor)| Operation::ArchiveWorkspaceGroup {
                actor,
                workspace,
                group,
            })
            .boxed()
    })
}

/// Generates workspace-group link restoration.
fn selected_workspace_group_unarchive(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.archived_workspace_groups()).map(|links| {
        select(links)
            .prop_map(|(workspace, group, actor)| Operation::UnarchiveWorkspaceGroup {
                actor,
                workspace,
                group,
            })
            .boxed()
    })
}

/// Generates a workspace operation when a matching workspace exists.
fn selected_workspace(
    workspaces: Vec<(Handle, Actor)>,
    operation: fn(Actor, Handle) -> Operation,
) -> Option<BoxedStrategy<Operation>> {
    if workspaces.is_empty() {
        None
    } else {
        Some(
            select(workspaces)
                .prop_map(move |(workspace, actor)| operation(actor, workspace))
                .boxed(),
        )
    }
}

/// Generates a workspace read by any actor.
fn selected_workspace_read(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let workspaces: Vec<_> =
        state.workspaces().into_iter().map(|(workspace, _)| workspace).collect();
    if workspaces.is_empty() {
        None
    } else {
        Some(
            (actor(), select(workspaces))
                .prop_map(|(actor, workspace)| Operation::GetWorkspace { actor, workspace })
                .boxed(),
        )
    }
}

/// Generates a workspace graph read by any actor.
fn selected_workspace_graph_read(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let workspaces: Vec<_> =
        state.workspaces().into_iter().map(|(workspace, _)| workspace).collect();
    non_empty(workspaces).map(|workspaces| {
        (actor(), select(workspaces))
            .prop_map(|(actor, workspace)| Operation::GetWorkspaceGraph { actor, workspace })
            .boxed()
    })
}

/// Generates a workspace name update by an administrator.
fn selected_workspace_update(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.active_workspace_admins()).map(|workspaces| {
        (select(workspaces), any::<u32>())
            .prop_map(|((workspace, actor), suffix)| Operation::UpdateWorkspace {
                actor,
                workspace,
                name: generated_name("updated-workspace", workspace.index, suffix),
            })
            .boxed()
    })
}

/// Generates an exact-title workspace search by any actor.
fn selected_workspace_search(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.objects()).map(|objects| {
        (actor(), select(objects))
            .prop_map(|(actor, (object, workspace))| Operation::SearchWorkspace {
                actor,
                workspace,
                object,
            })
            .boxed()
    })
}

/// Generates a workspace event read by any actor.
fn selected_workspace_events_read(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let workspaces: Vec<_> =
        state.workspaces().into_iter().map(|(workspace, _)| workspace).collect();
    non_empty(workspaces).map(|workspaces| {
        (actor(), select(workspaces))
            .prop_map(|(actor, workspace)| Operation::GetWorkspaceEvents { actor, workspace })
            .boxed()
    })
}

/// Generates a workspace-membership collection read by any actor.
fn selected_workspace_memberships_read(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let workspaces =
        state.workspaces().into_iter().map(|(workspace, _)| workspace).collect::<Vec<_>>();
    non_empty(workspaces).map(|workspaces| {
        (actor(), select(workspaces))
            .prop_map(|(actor, workspace)| Operation::ListWorkspaceMemberships { actor, workspace })
            .boxed()
    })
}

/// Generates a workspace-group collection read by any actor.
fn selected_workspace_groups_read(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let workspaces =
        state.workspaces().into_iter().map(|(workspace, _)| workspace).collect::<Vec<_>>();
    non_empty(workspaces).map(|workspaces| {
        (actor(), select(workspaces))
            .prop_map(|(actor, workspace)| Operation::ListWorkspaceGroups { actor, workspace })
            .boxed()
    })
}

/// Generates a direct workspace membership creation.
fn selected_membership_create(state: &Model, output: Handle) -> Option<BoxedStrategy<Operation>> {
    let candidates = state.membership_candidates();
    if candidates.is_empty() {
        None
    } else {
        Some(
            (select(candidates), select(vec![MembershipRole::Member, MembershipRole::Admin]))
                .prop_map(move |((workspace, actor, member), role)| {
                    Operation::CreateWorkspaceMembership { actor, workspace, member, role, output }
                })
                .boxed(),
        )
    }
}

/// Generates a workspace membership revocation.
fn selected_membership_revoke(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let memberships = state.revocable_memberships();
    if memberships.is_empty() {
        None
    } else {
        Some(
            select(memberships)
                .prop_map(|(membership, workspace, actor)| Operation::RevokeWorkspaceMembership {
                    actor,
                    workspace,
                    membership,
                })
                .boxed(),
        )
    }
}

/// Generates replacement of an active workspace membership with another role.
fn selected_membership_update(state: &Model, output: Handle) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.updatable_memberships()).map(|memberships| {
        (select(memberships), select(vec![MembershipRole::Member, MembershipRole::Admin]))
            .prop_map(move |((membership, workspace, actor), role)| {
                Operation::UpdateWorkspaceMembership { actor, workspace, membership, role, output }
            })
            .boxed()
    })
}

/// Generates a workspace-restricted API key owned by an active workspace user.
fn selected_api_key_create(state: &Model, output: Handle) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.active_workspace_users()).map(|candidates| {
        (select(candidates), select(vec![ApiKeyScope::WorkspaceRead, ApiKeyScope::ObjectRead]))
            .prop_map(move |((workspace, actor), scope)| Operation::CreateApiKey {
                actor,
                workspace,
                output,
                scope,
            })
            .boxed()
    })
}

/// Generates replacement of an API key's scope while retaining its workspace restriction.
fn selected_api_key_update(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let candidates = state
        .updatable_api_keys()
        .into_iter()
        .filter_map(|key| {
            let modeled = state.api_key(key)?;
            let scope = match modeled.scope {
                ApiKeyScope::WorkspaceRead => ApiKeyScope::ObjectRead,
                ApiKeyScope::ObjectRead => ApiKeyScope::WorkspaceRead,
                _ => return None,
            };
            Some((key, modeled.owner, scope))
        })
        .collect::<Vec<_>>();
    non_empty(candidates).map(|candidates| {
        select(candidates)
            .prop_map(|(key, actor, scope)| Operation::UpdateApiKey { actor, key, scope })
            .boxed()
    })
}

/// Generates revocation of an active API key.
fn selected_api_key_revoke(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let candidates = state
        .revocable_api_keys()
        .into_iter()
        .filter_map(|key| state.api_key(key).map(|modeled| (key, modeled.owner)))
        .collect::<Vec<_>>();
    non_empty(candidates).map(|candidates| {
        select(candidates).prop_map(|(key, actor)| Operation::RevokeApiKey { actor, key }).boxed()
    })
}

/// Generates bearer reads that compose API-key delegation with current owner authority.
fn selected_api_key_access_probe(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let candidates = state
        .api_key_object_probes()
        .into_iter()
        .filter_map(|(key, object)| {
            let modeled = state.api_key(key)?;
            Some((key, modeled.owner, modeled.workspace, object))
        })
        .collect::<Vec<_>>();
    non_empty(candidates).map(|candidates| {
        select(candidates)
            .prop_map(|(key, actor, workspace, object)| Operation::ProbeApiKeyAccess {
                actor,
                key,
                workspace,
                object,
            })
            .boxed()
    })
}

/// Generates object creation by an active workspace user.
fn selected_object_create(
    state: &Model,
    output: Handle,
    creator_grant: Handle,
) -> Option<BoxedStrategy<Operation>> {
    let workspaces = state.active_workspace_users();
    if workspaces.is_empty() {
        None
    } else {
        Some(
            (select(workspaces), any::<u32>())
                .prop_map(move |((workspace, actor), suffix)| Operation::CreateObject {
                    actor,
                    workspace,
                    output,
                    creator_grant,
                    title: generated_name("object", output.index, suffix),
                })
                .boxed(),
        )
    }
}

/// Generates an object read by any actor.
fn selected_object_read(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let objects = state.objects();
    if objects.is_empty() {
        None
    } else {
        Some(
            (actor(), select(objects))
                .prop_map(|(actor, (object, workspace))| Operation::GetObject {
                    actor,
                    workspace,
                    object,
                })
                .boxed(),
        )
    }
}

/// Generates an object graph read by any actor.
fn selected_object_graph_read(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.objects()).map(|objects| {
        (actor(), select(objects))
            .prop_map(|(actor, (object, workspace))| Operation::GetObjectGraph {
                actor,
                workspace,
                object,
            })
            .boxed()
    })
}

/// Generates an object backlinks read by any actor.
fn selected_object_backlinks_read(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.objects()).map(|objects| {
        (actor(), select(objects))
            .prop_map(|(actor, (object, workspace))| Operation::GetObjectBacklinks {
                actor,
                workspace,
                object,
            })
            .boxed()
    })
}

/// Generates an object event read by any actor.
fn selected_object_events_read(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.objects()).map(|objects| {
        (actor(), select(objects))
            .prop_map(|(actor, (object, workspace))| Operation::GetObjectEvents {
                actor,
                workspace,
                object,
            })
            .boxed()
    })
}

/// Generates a concrete current-version read by any actor.
fn selected_object_version_read(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.objects()).map(|objects| {
        (actor(), select(objects))
            .prop_map(|(actor, (object, workspace))| Operation::GetObjectVersion {
                actor,
                workspace,
                object,
            })
            .boxed()
    })
}

/// Generates an object-grant collection read by any actor.
fn selected_object_grants_read(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.objects()).map(|objects| {
        (actor(), select(objects))
            .prop_map(|(actor, (object, workspace))| Operation::ListObjectGrants {
                actor,
                workspace,
                object,
            })
            .boxed()
    })
}

/// Generates an object-edge collection read by any actor.
fn selected_object_edges_read(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.objects()).map(|objects| {
        (actor(), select(objects))
            .prop_map(|(actor, (object, workspace))| Operation::ListObjectEdges {
                actor,
                workspace,
                object,
            })
            .boxed()
    })
}

/// Generates a new workspace pin for an actor with active membership.
fn selected_workspace_pin(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let candidates = state
        .active_workspace_users()
        .into_iter()
        .filter(|(workspace, actor)| !state.workspace_pinned(*workspace, *actor))
        .collect::<Vec<_>>();
    non_empty(candidates).map(|candidates| {
        select(candidates)
            .prop_map(|(workspace, actor)| Operation::PinWorkspace { actor, workspace })
            .boxed()
    })
}

/// Generates removal of an existing workspace pin, including after access/lifecycle changes.
fn selected_workspace_unpin(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.workspace_pins()).map(|pins| {
        select(pins)
            .prop_map(|(workspace, actor)| Operation::UnpinWorkspace { actor, workspace })
            .boxed()
    })
}

/// Generates a new object pin for a currently readable object.
fn selected_object_pin(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let candidates = state
        .objects()
        .into_iter()
        .flat_map(|(object, workspace)| {
            Actor::ALL
                .into_iter()
                .filter(move |actor| state.can_read_object(object, *actor))
                .map(move |actor| (object, workspace, actor))
        })
        .filter(|(object, _, actor)| !state.object_pinned(*object, *actor))
        .collect::<Vec<_>>();
    non_empty(candidates).map(|candidates| {
        select(candidates)
            .prop_map(|(object, workspace, actor)| Operation::PinObject {
                actor,
                workspace,
                object,
            })
            .boxed()
    })
}

/// Generates removal of an existing object pin without requiring current access.
fn selected_object_unpin(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.object_pins()).map(|pins| {
        select(pins)
            .prop_map(|(object, workspace, actor)| Operation::UnpinObject {
                actor,
                workspace,
                object,
            })
            .boxed()
    })
}

/// Generates a new favorite for a currently readable object.
fn selected_object_favorite(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let candidates = state
        .objects()
        .into_iter()
        .flat_map(|(object, workspace)| {
            Actor::ALL
                .into_iter()
                .filter(move |actor| state.can_read_object(object, *actor))
                .map(move |actor| (object, workspace, actor))
        })
        .filter(|(object, _, actor)| !state.object_favorited(*object, *actor))
        .collect::<Vec<_>>();
    non_empty(candidates).map(|candidates| {
        select(candidates)
            .prop_map(|(object, workspace, actor)| Operation::FavoriteObject {
                actor,
                workspace,
                object,
            })
            .boxed()
    })
}

/// Generates removal of an existing favorite without requiring current access.
fn selected_object_unfavorite(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.object_favorites()).map(|favorites| {
        select(favorites)
            .prop_map(|(object, workspace, actor)| Operation::UnfavoriteObject {
                actor,
                workspace,
                object,
            })
            .boxed()
    })
}

/// Generates a new commentary thread on a readable active object.
fn selected_comment_thread_create(
    state: &Model,
    thread_output: Handle,
    comment_output: Handle,
) -> Option<BoxedStrategy<Operation>> {
    let candidates = state
        .objects()
        .into_iter()
        .flat_map(|(object, workspace)| {
            Actor::ALL
                .into_iter()
                .filter(move |actor| {
                    state.object(object) == Some((workspace, Lifecycle::Active))
                        && state.can_read_object(object, *actor)
                })
                .map(move |actor| (object, workspace, actor))
        })
        .collect::<Vec<_>>();
    non_empty(candidates).map(|candidates| {
        (select(candidates), any::<u32>())
            .prop_map(move |((object, workspace, actor), suffix)| Operation::CreateCommentThread {
                actor,
                workspace,
                object,
                thread_output,
                comment_output,
                body: generated_name("comment", comment_output.index, suffix),
            })
            .boxed()
    })
}

/// Generates a reply to an open commentary thread.
fn selected_comment_reply(state: &Model, output: Handle) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.replyable_comment_threads()).map(|candidates| {
        (select(candidates), any::<u32>())
            .prop_map(move |((thread, workspace, object, actor), suffix)| Operation::ReplyComment {
                actor,
                workspace,
                object,
                thread,
                output,
                body: generated_name("reply", output.index, suffix),
            })
            .boxed()
    })
}

/// Generates an edit by the comment author.
fn selected_comment_edit(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.editable_comments()).map(|candidates| {
        (select(candidates), any::<u32>())
            .prop_map(|((comment, workspace, object, actor), suffix)| Operation::EditComment {
                actor,
                workspace,
                object,
                comment,
                body: generated_name("edited-comment", comment.index, suffix),
            })
            .boxed()
    })
}

/// Generates a soft deletion by the author or an object administrator.
fn selected_comment_delete(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.deletable_comments()).map(|candidates| {
        select(candidates)
            .prop_map(|(comment, workspace, object, actor)| Operation::DeleteComment {
                actor,
                workspace,
                object,
                comment,
            })
            .boxed()
    })
}

/// Generates resolution of an open thread.
fn selected_comment_resolve(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.resolvable_comment_threads(false)).map(|candidates| {
        select(candidates)
            .prop_map(|(thread, workspace, object, actor)| Operation::ResolveCommentThread {
                actor,
                workspace,
                object,
                thread,
            })
            .boxed()
    })
}

/// Generates reopening of a resolved thread.
fn selected_comment_reopen(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.resolvable_comment_threads(true)).map(|candidates| {
        select(candidates)
            .prop_map(|(thread, workspace, object, actor)| Operation::ReopenCommentThread {
                actor,
                workspace,
                object,
                thread,
            })
            .boxed()
    })
}

/// Generates a thread-comment collection read by an arbitrary actor.
fn selected_thread_comments_read(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let candidates = state
        .comment_threads
        .iter()
        .map(|(thread, modeled)| (*thread, modeled.workspace, modeled.object))
        .collect::<Vec<_>>();
    non_empty(candidates).map(|candidates| {
        (actor(), select(candidates))
            .prop_map(|(actor, (thread, workspace, object))| Operation::ListThreadComments {
                actor,
                workspace,
                object,
                thread,
            })
            .boxed()
    })
}

/// Generates a mention-candidate read by an arbitrary actor.
fn selected_mention_candidates_read(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.objects()).map(|objects| {
        (actor(), actor(), select(objects))
            .prop_map(|(actor, candidate, (object, workspace))| Operation::ListMentionCandidates {
                actor,
                workspace,
                object,
                candidate,
            })
            .boxed()
    })
}

/// Generates a comment mention replacement probe on a readable active object.
fn selected_comment_mention_probe(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let mut candidates = Vec::new();
    for (object, workspace) in state.objects() {
        if state.object(object) != Some((workspace, Lifecycle::Active)) {
            continue;
        }
        let readers: Vec<_> =
            Actor::ALL.into_iter().filter(|actor| state.can_read_object(object, *actor)).collect();
        for actor in &readers {
            for first_mention in &readers {
                for second_mention in &readers {
                    if first_mention != second_mention {
                        candidates.push((
                            object,
                            workspace,
                            *actor,
                            *first_mention,
                            *second_mention,
                        ));
                    }
                }
            }
        }
    }

    non_empty(candidates).map(|candidates| {
        select(candidates)
            .prop_map(|(object, workspace, actor, first_mention, second_mention)| {
                Operation::ProbeCommentMentions {
                    actor,
                    workspace,
                    object,
                    first_mention,
                    second_mention,
                }
            })
            .boxed()
    })
}

/// Generates a notification/inbox probe for a current object viewer.
fn selected_notification_inbox_probe(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let candidates = state
        .objects()
        .into_iter()
        .flat_map(|(object, workspace)| {
            Actor::ALL
                .into_iter()
                .filter(move |actor| {
                    *actor != Actor::Admin
                        && state.object(object) == Some((workspace, Lifecycle::Active))
                        && state.can_read_object(object, *actor)
                })
                .map(move |actor| (workspace, object, actor))
        })
        .collect::<Vec<_>>();
    non_empty(candidates).map(|candidates| {
        select(candidates)
            .prop_map(|(workspace, object, actor)| Operation::ProbeNotificationInbox {
                actor,
                workspace,
                object,
            })
            .boxed()
    })
}

/// Generates an identity disable/enable probe anchored to current resource access.
fn selected_user_disable_enable_probe(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let candidates = state
        .objects()
        .into_iter()
        .flat_map(|(object, workspace)| {
            Actor::ALL
                .into_iter()
                .filter(move |target| {
                    *target != Actor::Admin
                        && state.object(object) == Some((workspace, Lifecycle::Active))
                        && state.can_read_workspace(workspace, *target)
                        && state.can_read_object(object, *target)
                })
                .map(move |target| (workspace, object, target))
        })
        .collect::<Vec<_>>();
    non_empty(candidates).map(|candidates| {
        select(candidates)
            .prop_map(|(workspace, object, target)| Operation::ProbeUserDisableEnable {
                actor: Actor::Admin,
                target,
                workspace,
                object,
            })
            .boxed()
    })
}

/// Generates rejected mutations by a non-admin actor against an active group.
fn selected_unauthorized_group_mutation_probe(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let mut candidates = Vec::new();
    for group in state.active_groups() {
        for actor in Actor::ALL {
            if actor == Actor::Admin || state.can_admin_group(group, actor) {
                continue;
            }
            for member in Actor::ALL {
                if member == Actor::Admin || state.has_active_group_membership(group, member) {
                    continue;
                }
                candidates.push((group, actor, member));
            }
        }
    }
    non_empty(candidates).map(|candidates| {
        select(candidates)
            .prop_map(|(group, actor, member)| Operation::ProbeUnauthorizedGroupMutations {
                actor,
                group,
                member,
            })
            .boxed()
    })
}

/// Generates rejected workspace mutations by a member without admin authority.
fn selected_unauthorized_workspace_mutation_probe(
    state: &Model,
) -> Option<BoxedStrategy<Operation>> {
    let mut candidates = Vec::new();
    for (workspace, _) in state.workspaces_with(Lifecycle::Active) {
        for actor in Actor::ALL {
            if !state.can_use_workspace(workspace, actor)
                || state.can_admin_workspace(workspace, actor)
            {
                continue;
            }
            for member in Actor::ALL {
                if member == Actor::Admin || state.has_workspace_membership(workspace, member) {
                    continue;
                }
                candidates.push((workspace, actor, member));
            }
        }
    }
    non_empty(candidates).map(|candidates| {
        select(candidates)
            .prop_map(|(workspace, actor, member)| Operation::ProbeUnauthorizedWorkspaceMutations {
                actor,
                workspace,
                member,
            })
            .boxed()
    })
}

/// Generates rejected mutations against an active object visible only as a viewer.
fn selected_unauthorized_object_mutation_probe(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let candidates = state
        .objects()
        .into_iter()
        .filter(|(object, workspace)| {
            state.object(*object) == Some((*workspace, Lifecycle::Active))
                && state.workspace(*workspace) == Some(Lifecycle::Active)
        })
        .flat_map(|(object, workspace)| {
            Actor::ALL.into_iter().filter_map(move |actor| {
                (state.can_read_object(object, actor)
                    && !state.can_edit_object(object, actor)
                    && !state.can_admin_object(object, actor))
                .then_some((object, workspace, actor))
            })
        })
        .collect::<Vec<_>>();
    non_empty(candidates).map(|candidates| {
        select(candidates)
            .prop_map(|(object, workspace, actor)| Operation::ProbeUnauthorizedObjectMutations {
                actor,
                workspace,
                object,
            })
            .boxed()
    })
}

/// Generates rejected object creation by an actor outside an active workspace.
fn selected_unauthorized_object_create_probe(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let candidates = state
        .workspaces_with(Lifecycle::Active)
        .into_iter()
        .flat_map(|(workspace, _)| {
            Actor::ALL
                .into_iter()
                .filter(move |actor| !state.can_use_workspace(workspace, *actor))
                .map(move |actor| (workspace, actor))
        })
        .collect::<Vec<_>>();
    non_empty(candidates).map(|candidates| {
        select(candidates)
            .prop_map(|(workspace, actor)| Operation::ProbeUnauthorizedObjectCreate {
                actor,
                workspace,
            })
            .boxed()
    })
}

/// Generates a wikilink projection probe across two editable active objects.
fn selected_wikilink_reresolution_probe(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let mut candidates = Vec::new();
    for (source, workspace) in state.objects() {
        if state.object(source) != Some((workspace, Lifecycle::Active)) {
            continue;
        }
        for (target, target_workspace) in state.objects() {
            if source == target
                || target_workspace != workspace
                || state.object(target) != Some((workspace, Lifecycle::Active))
            {
                continue;
            }
            for actor in Actor::ALL {
                if state.can_edit_object(source, actor) && state.can_edit_object(target, actor) {
                    candidates.push((source, target, workspace, actor));
                }
            }
        }
    }

    non_empty(candidates).map(|candidates| {
        (select(candidates), any::<u32>())
            .prop_map(|((source, target, workspace, actor), suffix)| {
                Operation::ProbeWikilinkReresolution { actor, workspace, source, target, suffix }
            })
            .boxed()
    })
}

/// Generates rejected active-object writes below an archived workspace.
fn selected_archived_workspace_object_write_probe(
    state: &Model,
) -> Option<BoxedStrategy<Operation>> {
    let candidates = state
        .objects()
        .into_iter()
        .filter(|(object, workspace)| {
            state.workspace(*workspace) == Some(Lifecycle::Archived)
                && state.object(*object) == Some((*workspace, Lifecycle::Active))
        })
        .collect::<Vec<_>>();
    non_empty(candidates).map(|candidates| {
        select(candidates)
            .prop_map(|(object, workspace)| Operation::ProbeArchivedWorkspaceObjectWrites {
                actor: Actor::Admin,
                workspace,
                object,
                principal: Actor::Alice,
            })
            .boxed()
    })
}

/// Generates rejected archived-object restoration below an archived workspace.
fn selected_archived_workspace_object_restore_probe(
    state: &Model,
) -> Option<BoxedStrategy<Operation>> {
    let candidates = state
        .objects()
        .into_iter()
        .filter(|(object, workspace)| {
            state.workspace(*workspace) == Some(Lifecycle::Archived)
                && state.object(*object) == Some((*workspace, Lifecycle::Archived))
        })
        .collect::<Vec<_>>();
    non_empty(candidates).map(|candidates| {
        select(candidates)
            .prop_map(|(object, workspace)| Operation::ProbeArchivedWorkspaceObjectRestore {
                actor: Actor::Admin,
                workspace,
                object,
            })
            .boxed()
    })
}

/// Generates an attachment upload to an editable active object.
fn selected_attachment_upload(state: &Model, output: Handle) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.attachment_upload_candidates()).map(|objects| {
        (select(objects), any::<u32>())
            .prop_map(move |((object, workspace, actor), suffix)| {
                let name = generated_name("attachment", output.index, suffix);
                let content = format!("{name} content").into_bytes();
                Operation::UploadObjectAttachment {
                    actor,
                    workspace,
                    object,
                    output,
                    name,
                    content,
                }
            })
            .boxed()
    })
}

/// Generates an attachment collection read by any actor.
fn selected_attachment_list(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.objects()).map(|objects| {
        (actor(), select(objects))
            .prop_map(|(actor, (object, workspace))| Operation::ListObjectAttachments {
                actor,
                workspace,
                object,
            })
            .boxed()
    })
}

/// Generates an attachment metadata or content read by any actor.
fn selected_attachment_read(state: &Model, content: bool) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.attachment_read_candidates()).map(|attachments| {
        select(attachments)
            .prop_map(move |(attachment, object, workspace, actor)| {
                if content {
                    Operation::GetObjectAttachmentContent { actor, workspace, object, attachment }
                } else {
                    Operation::GetObjectAttachment { actor, workspace, object, attachment }
                }
            })
            .boxed()
    })
}

/// Generates reuse of an authorized attachment on an editable object.
fn selected_attachment_reuse(state: &Model, output: Handle) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.attachment_reuse_candidates()).map(|attachments| {
        select(attachments)
            .prop_map(move |(source, object, workspace, actor)| Operation::ReuseObjectAttachment {
                actor,
                workspace,
                object,
                source,
                output,
            })
            .boxed()
    })
}

/// Generates an object update attempt by any actor.
fn selected_object_update(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let objects = state.update_attempts();
    if objects.is_empty() {
        None
    } else {
        Some(
            (select(objects), any::<u32>())
                .prop_map(|((object, workspace, actor), suffix)| Operation::UpdateObject {
                    actor,
                    workspace,
                    object,
                    title: generated_name("updated", object.index, suffix),
                })
                .boxed(),
        )
    }
}

/// Generates a direct object grant.
fn selected_grant_create(state: &Model, output: Handle) -> Option<BoxedStrategy<Operation>> {
    let candidates = state.grant_candidates();
    if candidates.is_empty() {
        None
    } else {
        Some(
            (
                select(candidates),
                select(vec![ObjectRole::Viewer, ObjectRole::Editor, ObjectRole::Admin]),
            )
                .prop_map(move |((object, workspace, actor, principal), role)| {
                    Operation::CreateObjectGrant {
                        actor,
                        workspace,
                        object,
                        principal,
                        role,
                        output,
                    }
                })
                .boxed(),
        )
    }
}

/// Generates a group-backed object grant.
fn selected_group_grant_create(state: &Model, output: Handle) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.group_grant_candidates()).map(|candidates| {
        (
            select(candidates),
            select(vec![ObjectRole::Viewer, ObjectRole::Editor, ObjectRole::Admin]),
        )
            .prop_map(move |((object, workspace, actor, principal), role)| {
                Operation::CreateGroupObjectGrant {
                    actor,
                    workspace,
                    object,
                    principal,
                    role,
                    output,
                }
            })
            .boxed()
    })
}

/// Generates a direct object-grant revocation.
fn selected_grant_revoke(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let grants = state.revocable_grants();
    if grants.is_empty() {
        None
    } else {
        Some(
            select(grants)
                .prop_map(|(grant, object, workspace, actor)| Operation::RevokeObjectGrant {
                    actor,
                    workspace,
                    object,
                    grant,
                })
                .boxed(),
        )
    }
}

/// Generates an immutable object-grant role replacement.
fn selected_grant_update(state: &Model, output: Handle) -> Option<BoxedStrategy<Operation>> {
    let grants = state.revocable_grants();
    if grants.is_empty() {
        None
    } else {
        Some(
            (
                select(grants),
                select(vec![ObjectRole::Viewer, ObjectRole::Editor, ObjectRole::Admin]),
            )
                .prop_map(move |((grant, object, workspace, actor), role)| {
                    Operation::UpdateObjectGrant { actor, workspace, object, grant, role, output }
                })
                .boxed(),
        )
    }
}

/// Generates an object lifecycle mutation.
fn selected_object_mutation(
    objects: Vec<(Handle, Handle, Actor)>,
    operation: fn(Actor, Handle, Handle) -> Operation,
) -> Option<BoxedStrategy<Operation>> {
    if objects.is_empty() {
        None
    } else {
        Some(
            select(objects)
                .prop_map(move |(object, workspace, actor)| operation(actor, workspace, object))
                .boxed(),
        )
    }
}

/// Generates an edge between two active objects.
fn selected_edge_create(state: &Model, output: Handle) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.edge_candidates()).map(|candidates| {
        select(candidates)
            .prop_map(move |(workspace, source, target, actor)| Operation::CreateObjectEdge {
                actor,
                workspace,
                source,
                target,
                output,
            })
            .boxed()
    })
}

/// Generates an edge read by any actor.
fn selected_edge_read(state: &Model) -> Option<BoxedStrategy<Operation>> {
    let edges: Vec<_> = state
        .edges
        .iter()
        .filter_map(|(edge, (workspace, _, _, active))| active.then_some((*edge, *workspace)))
        .collect();
    non_empty(edges).map(|edges| {
        (actor(), select(edges))
            .prop_map(|(actor, (edge, workspace))| Operation::GetObjectEdge {
                actor,
                workspace,
                edge,
            })
            .boxed()
    })
}

/// Generates an edge revocation.
fn selected_edge_revoke(state: &Model) -> Option<BoxedStrategy<Operation>> {
    non_empty(state.revocable_edges()).map(|edges| {
        select(edges)
            .prop_map(|(edge, workspace, actor)| Operation::RevokeObjectEdge {
                actor,
                workspace,
                edge,
            })
            .boxed()
    })
}

/// Returns a collection only when it contains selectable values.
fn non_empty<T>(values: Vec<T>) -> Option<Vec<T>> {
    (!values.is_empty()).then_some(values)
}

/// Builds a deterministic generated resource name.
fn generated_name(prefix: &str, index: u32, suffix: u32) -> String {
    format!("{prefix}-{index}-{suffix:08x}")
}
