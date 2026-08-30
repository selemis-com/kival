use std::collections::{BTreeMap, BTreeSet};

use kival_sdk::{ApiKeyScope, MembershipRole, ObjectRole};

use super::{Handle, Principal};
use crate::Actor;

/// Lifecycle tracked by the abstract state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// The resource can be used by active-only operations.
    Active,
    /// The resource has been archived and can be restored.
    Archived,
}

/// Attachment data retained by the abstract state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeledAttachment {
    /// Workspace containing the attachment.
    pub workspace: Handle,
    /// Object owning the attachment.
    pub object: Handle,
    /// Attachment directly reused to create this attachment.
    pub source: Option<Handle>,
    /// Attachment display name.
    pub name: String,
    /// Stored attachment bytes.
    pub content: Vec<u8>,
}

/// Mutable commentary retained by the abstract state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeledComment {
    /// Thread containing the comment.
    pub thread: Handle,
    /// User that authored the comment.
    pub author: Actor,
    /// Current body while the comment remains active.
    pub body: Option<String>,
}

/// Commentary thread retained by the abstract state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeledCommentThread {
    /// Workspace containing the parent object.
    pub workspace: Handle,
    /// Parent object.
    pub object: Handle,
    /// Root comment author.
    pub author: Actor,
    /// Root comment handle.
    pub root: Handle,
    /// Whether the thread is resolved.
    pub resolved: bool,
}

/// Delegated API key retained by the abstract state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeledApiKey {
    /// User that owns the key.
    pub owner: Actor,
    /// Single delegated workspace used by the stateful model.
    pub workspace: Handle,
    /// Current delegated scope.
    pub scope: ApiKeyScope,
    /// Mutable authorization revision.
    pub revision: i32,
    /// Whether the key remains active.
    pub active: bool,
}

/// Stable event fields tracked by the abstract state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeledEvent {
    /// Event kind emitted by the mutation.
    pub kind: String,
    /// Actor that performed the mutation.
    pub actor: Actor,
    /// Workspace associated with the event, when any.
    pub workspace: Option<Handle>,
    /// Object associated with the event, when any.
    pub object: Option<Handle>,
    /// Object edge associated with the event, when any.
    pub object_edge: Option<Handle>,
    /// Object grant associated with the event, when any.
    pub object_grant: Option<Handle>,
    /// Group associated with the event, when any.
    pub group: Option<Handle>,
    /// User targeted by the mutation, when any.
    pub target_user: Option<Actor>,
}

impl ModeledEvent {
    /// Creates a modeled event with no optional resource targets.
    #[must_use]
    pub fn new(kind: &str, actor: Actor) -> Self {
        Self {
            kind: kind.to_owned(),
            actor,
            workspace: None,
            object: None,
            object_edge: None,
            object_grant: None,
            group: None,
            target_user: None,
        }
    }

    /// Attaches a workspace target.
    #[must_use]
    pub const fn workspace(mut self, workspace: Handle) -> Self {
        self.workspace = Some(workspace);
        self
    }

    /// Attaches an object target.
    #[must_use]
    pub const fn object(mut self, object: Handle) -> Self {
        self.object = Some(object);
        self
    }

    /// Attaches an object-edge target.
    #[must_use]
    pub const fn object_edge(mut self, object_edge: Handle) -> Self {
        self.object_edge = Some(object_edge);
        self
    }

    /// Attaches an object-grant target.
    #[must_use]
    pub const fn object_grant(mut self, object_grant: Handle) -> Self {
        self.object_grant = Some(object_grant);
        self
    }

    /// Attaches a group target.
    #[must_use]
    pub const fn group(mut self, group: Handle) -> Self {
        self.group = Some(group);
        self
    }

    /// Attaches a target user.
    #[must_use]
    pub const fn target_user(mut self, target_user: Actor) -> Self {
        self.target_user = Some(target_user);
        self
    }
}

/// Abstract Kival state used while generating and shrinking transitions.
#[derive(Debug, Clone, Default)]
pub struct Model {
    /// Next workspace handle index.
    pub(super) next_workspace: u32,
    /// Next group handle index.
    pub(super) next_group: u32,
    /// Next object handle index.
    pub(super) next_object: u32,
    /// Next edge handle index.
    pub(super) next_edge: u32,
    /// Next attachment handle index.
    pub(super) next_attachment: u32,
    /// Next workspace-membership handle index.
    pub(super) next_membership: u32,
    /// Next direct object-grant handle index.
    pub(super) next_grant: u32,
    /// Next commentary-thread handle index.
    pub(super) next_comment_thread: u32,
    /// Next comment handle index.
    pub(super) next_comment: u32,
    /// Next API-key handle index.
    pub(super) next_api_key: u32,
    /// Lifecycle and owner of every modeled workspace.
    pub(super) workspaces: BTreeMap<Handle, (Actor, Lifecycle)>,
    /// Current name of every modeled workspace.
    pub(super) workspace_names: BTreeMap<Handle, String>,
    /// Lifecycle of every modeled group.
    pub(super) groups: BTreeMap<Handle, Lifecycle>,
    /// Current name of every modeled group.
    pub(super) group_names: BTreeMap<Handle, String>,
    /// Parent workspace, creator, lifecycle, title, and version of every modeled object.
    pub(super) objects: BTreeMap<Handle, (Handle, Actor, Lifecycle, String, i64)>,
    /// Author of every generated object version.
    pub(super) object_version_authors: BTreeMap<(Handle, i64), Actor>,
    /// Workspace, user, role, and active status of generated memberships.
    pub(super) memberships: BTreeMap<Handle, (Handle, Actor, MembershipRole, bool)>,
    /// Group, user, role, and active status of generated group memberships.
    pub(super) group_memberships: BTreeMap<Handle, (Handle, Actor, MembershipRole, bool)>,
    /// Lifecycle of generated workspace-group links.
    pub(super) workspace_groups: BTreeMap<(Handle, Handle), Lifecycle>,
    /// Workspace, object, principal, role, and active status of generated grants.
    pub(super) grants: BTreeMap<Handle, (Handle, Handle, Principal, ObjectRole, bool)>,
    /// Workspace, endpoints, and active status of generated object edges.
    pub(super) edges: BTreeMap<Handle, (Handle, Handle, Handle, bool)>,
    /// Modeled attachments indexed by symbolic handle.
    pub(super) attachments: BTreeMap<Handle, ModeledAttachment>,
    /// Actor-relative workspace pins.
    pub(super) workspace_pins: BTreeSet<(Actor, Handle)>,
    /// Actor-relative object pins.
    pub(super) object_pins: BTreeSet<(Actor, Handle)>,
    /// Actor-relative object favorites.
    pub(super) object_favorites: BTreeSet<(Actor, Handle)>,
    /// Commentary threads indexed by symbolic handle.
    pub(super) comment_threads: BTreeMap<Handle, ModeledCommentThread>,
    /// Comments indexed by symbolic handle.
    pub(super) comments: BTreeMap<Handle, ModeledComment>,
    /// API keys indexed by symbolic handle.
    pub(super) api_keys: BTreeMap<Handle, ModeledApiKey>,
    /// Stable projection of events emitted by successful modeled mutations.
    pub(super) events: Vec<ModeledEvent>,
}

impl Model {
    /// Returns a workspace's modeled lifecycle.
    #[must_use]
    pub fn workspace(&self, handle: Handle) -> Option<Lifecycle> {
        self.workspaces.get(&handle).map(|(_, lifecycle)| *lifecycle)
    }

    /// Returns the actor that owns a modeled workspace.
    #[must_use]
    pub fn workspace_owner(&self, handle: Handle) -> Option<Actor> {
        self.workspaces.get(&handle).map(|(actor, _)| *actor)
    }

    /// Returns a workspace's current modeled name.
    #[must_use]
    pub fn workspace_name(&self, handle: Handle) -> Option<&str> {
        self.workspace_names.get(&handle).map(String::as_str)
    }

    /// Returns a group's modeled lifecycle.
    #[must_use]
    pub fn group(&self, handle: Handle) -> Option<Lifecycle> {
        self.groups.get(&handle).copied()
    }

    /// Returns a group's current modeled name.
    #[must_use]
    pub fn group_name(&self, handle: Handle) -> Option<&str> {
        self.group_names.get(&handle).map(String::as_str)
    }

    /// Returns an object's parent workspace and modeled lifecycle.
    #[must_use]
    pub fn object(&self, handle: Handle) -> Option<(Handle, Lifecycle)> {
        self.objects.get(&handle).map(|(workspace, _, lifecycle, _, _)| (*workspace, *lifecycle))
    }

    /// Returns the actor that created an object.
    #[must_use]
    pub fn object_creator(&self, handle: Handle) -> Option<Actor> {
        self.objects.get(&handle).map(|(_, actor, _, _, _)| *actor)
    }

    /// Returns an object's current modeled title.
    #[must_use]
    pub fn object_title(&self, handle: Handle) -> Option<&str> {
        self.objects.get(&handle).map(|(_, _, _, title, _)| title.as_str())
    }

    /// Returns an object's current modeled version number.
    #[must_use]
    pub fn object_version(&self, handle: Handle) -> Option<i64> {
        self.objects.get(&handle).map(|(_, _, _, _, version)| *version)
    }

    /// Returns the actor that authored a generated object version.
    #[must_use]
    pub fn object_version_author(&self, object: Handle, version: i64) -> Option<Actor> {
        self.object_version_authors.get(&(object, version)).copied()
    }

    /// Returns an actor's current effective workspace role.
    #[must_use]
    pub fn workspace_role(&self, workspace: Handle, actor: Actor) -> Option<MembershipRole> {
        if self.workspace(workspace) != Some(Lifecycle::Active) {
            return None;
        }
        if self.has_workspace_admin_role(workspace, actor) {
            return Some(MembershipRole::Admin);
        }
        self.memberships.values().find_map(|(member_workspace, member, role, active)| {
            (*member_workspace == workspace && *member == actor && *active).then_some(*role)
        })
    }

    /// Returns a generated membership's workspace, actor, role, and active status.
    #[must_use]
    pub fn membership(&self, handle: Handle) -> Option<(Handle, Actor, MembershipRole, bool)> {
        self.memberships.get(&handle).copied()
    }

    /// Returns a generated group membership's group, actor, role, and active status.
    #[must_use]
    pub fn group_membership(
        &self,
        handle: Handle,
    ) -> Option<(Handle, Actor, MembershipRole, bool)> {
        self.group_memberships.get(&handle).copied()
    }

    /// Returns a generated grant's workspace, object, actor, role, and active status.
    #[must_use]
    pub fn grant(&self, handle: Handle) -> Option<(Handle, Handle, Principal, ObjectRole, bool)> {
        self.grants.get(&handle).copied()
    }

    /// Returns a generated edge's workspace, endpoints, and active status.
    #[must_use]
    pub fn edge(&self, handle: Handle) -> Option<(Handle, Handle, Handle, bool)> {
        self.edges.get(&handle).copied()
    }

    /// Returns a generated attachment's modeled data.
    #[must_use]
    pub fn attachment(&self, handle: Handle) -> Option<&ModeledAttachment> {
        self.attachments.get(&handle)
    }

    /// Returns the stable event projection expected from successful mutations.
    #[must_use]
    pub fn events(&self) -> &[ModeledEvent] {
        &self.events
    }

    /// Returns every generated attachment owned by an object.
    #[must_use]
    pub fn object_attachments(&self, object: Handle) -> Vec<Handle> {
        self.attachments
            .iter()
            .filter_map(|(attachment, modeled)| (modeled.object == object).then_some(*attachment))
            .collect()
    }

    /// Returns whether an actor can observe an active edge between active objects.
    #[must_use]
    pub fn can_read_edge(&self, edge: Handle, actor: Actor) -> bool {
        self.edge(edge).is_some_and(|(_, source, target, active)| {
            active
                && self.object(source).is_some_and(|(_, lifecycle)| lifecycle == Lifecycle::Active)
                && self.object(target).is_some_and(|(_, lifecycle)| lifecycle == Lifecycle::Active)
                && self.can_read_object(source, actor)
                && self.can_read_object(target, actor)
        })
    }

    /// Returns objects and workspaces affected by grants to a group.
    #[must_use]
    pub fn group_granted_objects(&self, group: Handle) -> Vec<(Handle, Handle)> {
        self.grants
            .values()
            .filter_map(|(workspace, object, principal, _, active)| {
                (*active && *principal == Principal::Group(group)).then_some((*object, *workspace))
            })
            .collect()
    }

    /// Returns active objects visible to an actor in a workspace.
    #[must_use]
    pub fn visible_active_objects(&self, workspace: Handle, actor: Actor) -> Vec<Handle> {
        self.objects
            .iter()
            .filter_map(|(object, (object_workspace, _, lifecycle, _, _))| {
                (*object_workspace == workspace
                    && *lifecycle == Lifecycle::Active
                    && self.can_read_object(*object, actor))
                .then_some(*object)
            })
            .collect()
    }

    /// Returns active edges visible to an actor in a workspace.
    #[must_use]
    pub fn visible_active_edges(&self, workspace: Handle, actor: Actor) -> Vec<Handle> {
        self.edges
            .iter()
            .filter_map(|(edge, (edge_workspace, _, _, _))| {
                (*edge_workspace == workspace && self.can_read_edge(*edge, actor)).then_some(*edge)
            })
            .collect()
    }

    /// Returns inbound edges projected by the backlinks endpoint.
    ///
    /// The default backlinks query excludes archived source objects, while an
    /// archived target remains readable to actors with object-admin access.
    #[must_use]
    pub fn visible_incoming_edges(&self, object: Handle, actor: Actor) -> Vec<Handle> {
        let Some((workspace, _, _, _, _)) = self.objects.get(&object) else {
            return Vec::new();
        };

        self.edges
            .iter()
            .filter_map(|(edge, (edge_workspace, source, target, active))| {
                let source_is_active = self
                    .object(*source)
                    .is_some_and(|(_, lifecycle)| lifecycle == Lifecycle::Active);
                (*edge_workspace == *workspace
                    && *target == object
                    && *active
                    && source_is_active
                    && self.can_read_object(*source, actor)
                    && self.can_read_object(object, actor))
                .then_some(*edge)
            })
            .collect()
    }

    /// Returns whether an actor has a direct active workspace membership.
    #[must_use]
    pub fn has_workspace_membership(&self, workspace: Handle, actor: Actor) -> bool {
        self.memberships.values().any(|(member_workspace, member, _, active)| {
            *member_workspace == workspace && *member == actor && *active
        }) || self.workspace_owner(workspace) == Some(actor)
    }

    /// Returns whether an actor can read a workspace in either lifecycle state.
    #[must_use]
    pub fn can_read_workspace(&self, workspace: Handle, actor: Actor) -> bool {
        self.workspace(workspace).is_some()
            && (actor == Actor::Admin || self.has_workspace_membership(workspace, actor))
    }

    /// Returns whether an actor can use an active workspace.
    #[must_use]
    pub fn can_use_workspace(&self, workspace: Handle, actor: Actor) -> bool {
        self.workspace(workspace) == Some(Lifecycle::Active)
            && (actor == Actor::Admin || self.has_workspace_membership(workspace, actor))
    }

    /// Returns whether an actor can administer an active workspace.
    #[must_use]
    pub fn can_admin_workspace(&self, workspace: Handle, actor: Actor) -> bool {
        self.workspace(workspace) == Some(Lifecycle::Active)
            && self.has_workspace_admin_role(workspace, actor)
    }

    /// Returns whether an actor can restore an archived workspace.
    #[must_use]
    pub fn can_restore_workspace(&self, workspace: Handle, actor: Actor) -> bool {
        self.workspace(workspace) == Some(Lifecycle::Archived)
            && self.has_workspace_admin_role(workspace, actor)
    }

    /// Returns whether an actor can read an object in its current lifecycle.
    #[must_use]
    pub fn can_read_object(&self, object: Handle, actor: Actor) -> bool {
        let Some((workspace, _, lifecycle, _, _)) = self.objects.get(&object) else {
            return false;
        };
        if self.workspace(*workspace) != Some(Lifecycle::Active) {
            return false;
        }

        match lifecycle {
            Lifecycle::Active => self.object_role(object, actor).is_some(),
            Lifecycle::Archived => self.object_role(object, actor) == Some(ObjectRole::Admin),
        }
    }

    /// Returns whether an actor can append a version to an active object.
    #[must_use]
    pub fn can_edit_object(&self, object: Handle, actor: Actor) -> bool {
        self.object(object).is_some_and(|(workspace, lifecycle)| {
            self.workspace(workspace) == Some(Lifecycle::Active)
                && lifecycle == Lifecycle::Active
                && matches!(
                    self.object_role(object, actor),
                    Some(ObjectRole::Editor | ObjectRole::Admin)
                )
        })
    }

    /// Returns whether an actor can administer an object in its current workspace.
    #[must_use]
    pub fn can_admin_object(&self, object: Handle, actor: Actor) -> bool {
        self.object(object).is_some_and(|(workspace, _)| {
            self.workspace(workspace) == Some(Lifecycle::Active)
                && self.object_role(object, actor) == Some(ObjectRole::Admin)
        })
    }

    /// Returns an actor's effective role for an object.
    #[must_use]
    pub fn object_role(&self, object: Handle, actor: Actor) -> Option<ObjectRole> {
        let (workspace, _, _, _, _) = self.objects.get(&object)?;
        if self.workspace(*workspace) != Some(Lifecycle::Active) {
            return None;
        }
        if self.has_workspace_admin_role(*workspace, actor) {
            return Some(ObjectRole::Admin);
        }
        if !self.has_workspace_membership(*workspace, actor) {
            return None;
        }

        self.grants
            .values()
            .filter_map(|(_, grant_object, principal, role, active)| {
                (*grant_object == object
                    && *active
                    && self.principal_contains(*workspace, *principal, actor))
                .then_some(*role)
            })
            .fold(None, strongest_role)
    }

    /// Returns whether changing an active grant to `role` preserves an admin grant.
    #[must_use]
    pub fn can_update_grant(&self, grant: Handle, role: ObjectRole) -> bool {
        self.grants.get(&grant).is_some_and(|(_, object, _, current_role, active)| {
            *active
                && self.object(*object).is_some_and(|(_, lifecycle)| lifecycle == Lifecycle::Active)
                && (*current_role != ObjectRole::Admin
                    || role == ObjectRole::Admin
                    || self.active_admin_grant_count(*object) > 1)
        })
    }

    /// Returns whether revoking an active grant preserves an admin grant.
    #[must_use]
    pub fn can_revoke_grant(&self, grant: Handle) -> bool {
        self.grants.get(&grant).is_some_and(|(_, object, _, role, active)| {
            *active
                && self.object(*object).is_some_and(|(_, lifecycle)| lifecycle == Lifecycle::Active)
                && (*role != ObjectRole::Admin || self.active_admin_grant_count(*object) > 1)
        })
    }

    /// Returns whether an actor has pinned a workspace.
    #[must_use]
    pub fn workspace_pinned(&self, workspace: Handle, actor: Actor) -> bool {
        self.workspace_pins.contains(&(actor, workspace))
    }

    /// Returns modeled workspace pins as `(workspace, actor)` pairs.
    #[must_use]
    pub(super) fn workspace_pins(&self) -> Vec<(Handle, Actor)> {
        self.workspace_pins.iter().map(|(actor, workspace)| (*workspace, *actor)).collect()
    }

    /// Returns whether an actor has pinned an object.
    #[must_use]
    pub fn object_pinned(&self, object: Handle, actor: Actor) -> bool {
        self.object_pins.contains(&(actor, object))
    }

    /// Returns modeled object pins with their parent workspace.
    #[must_use]
    pub(super) fn object_pins(&self) -> Vec<(Handle, Handle, Actor)> {
        self.object_pins
            .iter()
            .filter_map(|(actor, object)| {
                self.object(*object).map(|(workspace, _)| (*object, workspace, *actor))
            })
            .collect()
    }

    /// Returns whether an actor has favorited an object.
    #[must_use]
    pub fn object_favorited(&self, object: Handle, actor: Actor) -> bool {
        self.object_favorites.contains(&(actor, object))
    }

    /// Returns modeled object favorites with their parent workspace.
    #[must_use]
    pub(super) fn object_favorites(&self) -> Vec<(Handle, Handle, Actor)> {
        self.object_favorites
            .iter()
            .filter_map(|(actor, object)| {
                self.object(*object).map(|(workspace, _)| (*object, workspace, *actor))
            })
            .collect()
    }

    /// Returns a modeled commentary thread.
    #[must_use]
    pub fn comment_thread(&self, thread: Handle) -> Option<ModeledCommentThread> {
        self.comment_threads.get(&thread).copied()
    }

    /// Returns a modeled comment.
    #[must_use]
    pub fn comment(&self, comment: Handle) -> Option<&ModeledComment> {
        self.comments.get(&comment)
    }

    /// Returns comments in a thread in symbolic creation order.
    #[must_use]
    pub fn thread_comments(&self, thread: Handle) -> Vec<Handle> {
        self.comments
            .iter()
            .filter_map(|(comment, modeled)| (modeled.thread == thread).then_some(*comment))
            .collect()
    }

    /// Returns active open threads that currently accept replies.
    pub(super) fn replyable_comment_threads(&self) -> Vec<(Handle, Handle, Handle, Actor)> {
        self.comment_threads
            .iter()
            .flat_map(|(thread, modeled)| {
                Actor::ALL.into_iter().filter_map(move |actor| {
                    (!modeled.resolved
                        && self.object(modeled.object)
                            == Some((modeled.workspace, Lifecycle::Active))
                        && self.can_read_object(modeled.object, actor))
                    .then_some((*thread, modeled.workspace, modeled.object, actor))
                })
            })
            .collect()
    }

    /// Returns active comments paired with their author when editing is allowed.
    pub(super) fn editable_comments(&self) -> Vec<(Handle, Handle, Handle, Actor)> {
        self.comments
            .iter()
            .filter_map(|(comment, modeled)| {
                let thread = self.comment_thread(modeled.thread)?;
                (modeled.body.is_some()
                    && !thread.resolved
                    && self.object(thread.object) == Some((thread.workspace, Lifecycle::Active))
                    && self.can_read_object(thread.object, modeled.author))
                .then_some((*comment, thread.workspace, thread.object, modeled.author))
            })
            .collect()
    }

    /// Returns comments and actors allowed to soft-delete them.
    pub(super) fn deletable_comments(&self) -> Vec<(Handle, Handle, Handle, Actor)> {
        self.comments
            .iter()
            .flat_map(|(comment, modeled)| {
                let Some(thread) = self.comment_thread(modeled.thread) else {
                    return Vec::new().into_iter();
                };
                if modeled.body.is_none()
                    || self.object(thread.object) != Some((thread.workspace, Lifecycle::Active))
                {
                    return Vec::new().into_iter();
                }
                Actor::ALL
                    .into_iter()
                    .filter(move |actor| {
                        self.can_read_object(thread.object, *actor)
                            && (*actor == modeled.author
                                || self.object_role(thread.object, *actor)
                                    == Some(ObjectRole::Admin))
                    })
                    .map(move |actor| (*comment, thread.workspace, thread.object, actor))
                    .collect::<Vec<_>>()
                    .into_iter()
            })
            .collect()
    }

    /// Returns threads and actors allowed to change their resolution state.
    pub(super) fn resolvable_comment_threads(
        &self,
        resolved: bool,
    ) -> Vec<(Handle, Handle, Handle, Actor)> {
        self.comment_threads
            .iter()
            .flat_map(|(thread, modeled)| {
                if modeled.resolved == resolved
                    || self.object(modeled.object) != Some((modeled.workspace, Lifecycle::Active))
                {
                    return Vec::new().into_iter();
                }
                Actor::ALL
                    .into_iter()
                    .filter(move |actor| {
                        self.can_read_object(modeled.object, *actor)
                            && (*actor == modeled.author
                                || self.object_role(modeled.object, *actor)
                                    == Some(ObjectRole::Admin))
                    })
                    .map(move |actor| (*thread, modeled.workspace, modeled.object, actor))
                    .collect::<Vec<_>>()
                    .into_iter()
            })
            .collect()
    }

    /// Returns a modeled API key.
    #[must_use]
    pub fn api_key(&self, key: Handle) -> Option<ModeledApiKey> {
        self.api_keys.get(&key).copied()
    }

    /// Returns active API keys that can retain their current workspace restriction.
    pub(super) fn updatable_api_keys(&self) -> Vec<Handle> {
        self.api_keys
            .iter()
            .filter_map(|(key, modeled)| {
                (modeled.active && self.can_use_workspace(modeled.workspace, modeled.owner))
                    .then_some(*key)
            })
            .collect()
    }

    /// Returns active API keys.
    pub(super) fn revocable_api_keys(&self) -> Vec<Handle> {
        self.api_keys.iter().filter_map(|(key, modeled)| modeled.active.then_some(*key)).collect()
    }

    /// Returns API keys with objects available for an authorization probe.
    pub(super) fn api_key_object_probes(&self) -> Vec<(Handle, Handle)> {
        self.api_keys
            .iter()
            .flat_map(|(key, modeled)| {
                self.objects.iter().filter_map(move |(object, (workspace, _, _, _, _))| {
                    (*workspace == modeled.workspace).then_some((*key, *object))
                })
            })
            .collect()
    }

    /// Returns the number of modeled workspaces.
    #[must_use]
    pub fn workspace_count(&self) -> usize {
        self.workspaces.len()
    }

    /// Returns all workspaces visible to an actor in either lifecycle state.
    #[must_use]
    pub fn visible_workspaces(&self, actor: Actor) -> Vec<Handle> {
        self.workspaces
            .keys()
            .filter(|workspace| self.can_read_workspace(**workspace, actor))
            .copied()
            .collect()
    }

    /// Returns all groups visible to an actor in either lifecycle state.
    #[must_use]
    pub fn visible_groups(&self, actor: Actor) -> Vec<Handle> {
        self.groups
            .keys()
            .filter(|group| self.has_group_admin_role(**group, actor))
            .copied()
            .collect()
    }

    /// Returns whether an actor can read a group in either lifecycle state.
    #[must_use]
    pub fn can_read_group(&self, group: Handle, actor: Actor) -> bool {
        self.groups.contains_key(&group) && self.has_group_admin_role(group, actor)
    }

    /// Returns whether an actor can administer memberships in an active group.
    #[must_use]
    pub fn can_admin_group(&self, group: Handle, actor: Actor) -> bool {
        self.groups.get(&group) == Some(&Lifecycle::Active)
            && self.has_group_admin_role(group, actor)
    }

    /// Returns active direct memberships in a workspace.
    #[must_use]
    pub fn active_workspace_memberships(&self, workspace: Handle) -> Vec<Handle> {
        self.memberships
            .iter()
            .filter_map(|(membership, (member_workspace, _, _, active))| {
                (*member_workspace == workspace && *active).then_some(*membership)
            })
            .collect()
    }

    /// Returns workspace-group links and their modeled lifecycle.
    #[must_use]
    pub fn workspace_group_lifecycles(&self, workspace: Handle) -> Vec<(Handle, Lifecycle)> {
        self.workspace_groups
            .iter()
            .filter_map(|((member_workspace, group), lifecycle)| {
                (*member_workspace == workspace).then_some((*group, *lifecycle))
            })
            .collect()
    }

    /// Returns active grants belonging to an object.
    #[must_use]
    pub fn active_object_grants(&self, object: Handle) -> Vec<Handle> {
        self.grants
            .iter()
            .filter_map(|(grant, (_, grant_object, _, _, active))| {
                (*grant_object == object && *active).then_some(*grant)
            })
            .collect()
    }

    /// Returns active explicit edges incident to an object and readable at both endpoints.
    #[must_use]
    pub fn visible_incident_edges(&self, object: Handle, actor: Actor) -> Vec<Handle> {
        self.edges
            .iter()
            .filter_map(|(edge, (_, source, target, active))| {
                let incident = *source == object || *target == object;
                let source_active = self
                    .object(*source)
                    .is_some_and(|(_, lifecycle)| lifecycle == Lifecycle::Active);
                let target_active = self
                    .object(*target)
                    .is_some_and(|(_, lifecycle)| lifecycle == Lifecycle::Active);
                (*active
                    && incident
                    && source_active
                    && target_active
                    && self.can_read_object(*source, actor)
                    && self.can_read_object(*target, actor))
                .then_some(*edge)
            })
            .collect()
    }

    /// Returns active memberships in a group.
    #[must_use]
    pub fn active_group_memberships(&self, group: Handle) -> Vec<Handle> {
        self.group_memberships
            .iter()
            .filter_map(|(membership, (member_group, _, _, active))| {
                (*member_group == group && *active).then_some(*membership)
            })
            .collect()
    }

    /// Returns the number of modeled objects.
    #[must_use]
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// Returns handles and owners for workspaces in `lifecycle`.
    pub(super) fn workspaces_with(&self, lifecycle: Lifecycle) -> Vec<(Handle, Actor)> {
        self.workspaces
            .iter()
            .filter_map(|(handle, (actor, current))| {
                (*current == lifecycle).then_some((*handle, *actor))
            })
            .collect()
    }

    /// Returns all known workspace handles and owners.
    pub(super) fn workspaces(&self) -> Vec<(Handle, Actor)> {
        self.workspaces.iter().map(|(handle, (actor, _))| (*handle, *actor)).collect()
    }

    /// Returns active workspaces paired with actors that can use them.
    pub(super) fn active_workspace_users(&self) -> Vec<(Handle, Actor)> {
        self.workspaces_with(Lifecycle::Active)
            .into_iter()
            .flat_map(|(workspace, _)| {
                let copies = 1 + self.objects_in_workspace(workspace).min(4);
                Actor::ALL
                    .into_iter()
                    .filter(move |actor| self.can_use_workspace(workspace, *actor))
                    .flat_map(move |actor| std::iter::repeat_n((workspace, actor), copies))
            })
            .collect()
    }

    /// Returns active workspaces paired with actors that can administer them.
    pub(super) fn active_workspace_admins(&self) -> Vec<(Handle, Actor)> {
        self.workspaces_with(Lifecycle::Active)
            .into_iter()
            .flat_map(|(workspace, _)| {
                Actor::ALL
                    .into_iter()
                    .filter(move |actor| self.can_admin_workspace(workspace, *actor))
                    .map(move |actor| (workspace, actor))
            })
            .collect()
    }

    /// Returns workspaces paired with actors that can restore them.
    pub(super) fn archived_workspace_admins(&self) -> Vec<(Handle, Actor)> {
        self.workspaces_with(Lifecycle::Archived)
            .into_iter()
            .flat_map(|(workspace, _)| {
                Actor::ALL
                    .into_iter()
                    .filter(move |actor| self.has_workspace_admin_role(workspace, *actor))
                    .map(move |actor| (workspace, actor))
            })
            .collect()
    }

    /// Returns possible new direct memberships and an administrator that can add each one.
    pub(super) fn membership_candidates(&self) -> Vec<(Handle, Actor, Actor)> {
        self.active_workspace_admins()
            .into_iter()
            .flat_map(|(workspace, administrator)| {
                Actor::ALL
                    .into_iter()
                    .filter(move |member| {
                        *member != Actor::Admin
                            && !self.has_workspace_membership(workspace, *member)
                    })
                    .map(move |member| (workspace, administrator, member))
            })
            .collect()
    }

    /// Returns active generated memberships and actors that may revoke them.
    pub(super) fn revocable_memberships(&self) -> Vec<(Handle, Handle, Actor)> {
        self.memberships
            .iter()
            .filter(|(_, (_, _, _, active))| *active)
            .flat_map(|(membership, (workspace, _, _, _))| {
                Actor::ALL
                    .into_iter()
                    .filter(move |actor| self.can_admin_workspace(*workspace, *actor))
                    .map(move |actor| (*membership, *workspace, actor))
            })
            .collect()
    }

    /// Returns active workspace memberships paired with actors able to replace their role.
    pub(super) fn updatable_memberships(&self) -> Vec<(Handle, Handle, Actor)> {
        self.memberships
            .iter()
            .filter(|(_, (_, _, _, active))| *active)
            .flat_map(|(membership, (workspace, _, _, _))| {
                Actor::ALL
                    .into_iter()
                    .filter(move |actor| self.can_admin_workspace(*workspace, *actor))
                    .map(move |actor| (*membership, *workspace, actor))
            })
            .collect()
    }

    /// Returns all modeled object handles and their parent workspaces.
    #[must_use]
    pub fn objects(&self) -> Vec<(Handle, Handle)> {
        self.objects.iter().map(|(object, (workspace, _, _, _, _))| (*object, *workspace)).collect()
    }

    /// Returns active objects paired with actors that may attempt to update them.
    pub(super) fn update_attempts(&self) -> Vec<(Handle, Handle, Actor)> {
        self.objects
            .iter()
            .filter(|(_, (workspace, _, lifecycle, _, _))| {
                *lifecycle == Lifecycle::Active
                    && self.workspace(*workspace) == Some(Lifecycle::Active)
            })
            .flat_map(|(object, (workspace, _, _, _, _))| {
                Actor::ALL.into_iter().map(move |actor| (*object, *workspace, actor))
            })
            .collect()
    }

    /// Returns active objects paired with actors that can administer them.
    pub(super) fn administrable_active_objects(&self) -> Vec<(Handle, Handle, Actor)> {
        self.objects
            .iter()
            .filter(|(_, (_, _, lifecycle, _, _))| *lifecycle == Lifecycle::Active)
            .flat_map(|(object, (workspace, _, _, _, _))| {
                Actor::ALL
                    .into_iter()
                    .filter(move |actor| self.can_admin_object(*object, *actor))
                    .map(move |actor| (*object, *workspace, actor))
            })
            .collect()
    }

    /// Returns archived objects paired with actors that can administer them.
    pub(super) fn administrable_archived_objects(&self) -> Vec<(Handle, Handle, Actor)> {
        self.objects
            .iter()
            .filter(|(_, (_, _, lifecycle, _, _))| *lifecycle == Lifecycle::Archived)
            .flat_map(|(object, (workspace, _, _, _, _))| {
                Actor::ALL
                    .into_iter()
                    .filter(move |actor| self.can_admin_object(*object, *actor))
                    .map(move |actor| (*object, *workspace, actor))
            })
            .collect()
    }

    /// Returns candidates for new direct grants.
    pub(super) fn grant_candidates(&self) -> Vec<(Handle, Handle, Actor, Actor)> {
        self.administrable_active_objects()
            .into_iter()
            .flat_map(|(object, workspace, administrator)| {
                Actor::ALL
                    .into_iter()
                    .filter(move |principal| {
                        *principal != Actor::Admin
                            && self.has_workspace_membership(workspace, *principal)
                            && !self.grants.values().any(
                                |(_, grant_object, grant_principal, _, active)| {
                                    *grant_object == object
                                        && *grant_principal == Principal::User(*principal)
                                        && *active
                                },
                            )
                    })
                    .map(move |principal| (object, workspace, administrator, principal))
            })
            .collect()
    }

    /// Returns active generated grants paired with administrators that can revoke them.
    pub(super) fn revocable_grants(&self) -> Vec<(Handle, Handle, Handle, Actor)> {
        self.grants
            .iter()
            .filter(|(_, (_, _, _, _, active))| *active)
            .flat_map(|(grant, (workspace, object, _, _, _))| {
                Actor::ALL
                    .into_iter()
                    .filter(move |actor| {
                        self.object(*object)
                            .is_some_and(|(_, lifecycle)| lifecycle == Lifecycle::Active)
                            && self.can_admin_object(*object, *actor)
                    })
                    .map(move |actor| (*grant, *object, *workspace, actor))
            })
            .collect()
    }

    /// Returns active groups.
    pub(super) fn active_groups(&self) -> Vec<Handle> {
        self.groups
            .iter()
            .filter_map(|(group, lifecycle)| (*lifecycle == Lifecycle::Active).then_some(*group))
            .collect()
    }

    /// Returns active groups paired with every actor able to administer them.
    pub(super) fn active_group_admins(&self) -> Vec<(Handle, Actor)> {
        self.active_groups()
            .into_iter()
            .flat_map(|group| {
                Actor::ALL
                    .into_iter()
                    .filter(move |actor| self.has_group_admin_role(group, *actor))
                    .map(move |actor| (group, actor))
            })
            .collect()
    }

    /// Returns archived groups paired with the global administrator.
    pub(super) fn archived_group_admins(&self) -> Vec<(Handle, Actor)> {
        self.groups
            .iter()
            .filter_map(|(group, lifecycle)| {
                (*lifecycle == Lifecycle::Archived).then_some((*group, Actor::Admin))
            })
            .collect()
    }

    /// Returns group membership candidates and an actor that can add them.
    pub(super) fn group_membership_candidates(&self) -> Vec<(Handle, Actor, Actor)> {
        self.active_group_admins()
            .into_iter()
            .flat_map(|(group, administrator)| {
                Actor::ALL
                    .into_iter()
                    .filter(move |member| {
                        *member != Actor::Admin && !self.has_active_group_membership(group, *member)
                    })
                    .map(move |member| (group, administrator, member))
            })
            .collect()
    }

    /// Returns active group memberships that can be revoked.
    pub(super) fn revocable_group_memberships(&self) -> Vec<(Handle, Handle, Actor)> {
        self.group_memberships
            .iter()
            .filter(|(_, (_, _, _, active))| *active)
            .flat_map(|(membership, (group, _, _, _))| {
                Actor::ALL
                    .into_iter()
                    .filter(move |actor| self.can_admin_group(*group, *actor))
                    .map(move |actor| (*membership, *group, actor))
            })
            .collect()
    }

    /// Returns active group memberships paired with actors able to replace their role.
    pub(super) fn updatable_group_memberships(&self) -> Vec<(Handle, Handle, Actor)> {
        self.group_memberships
            .iter()
            .filter(|(_, (_, _, _, active))| *active)
            .flat_map(|(membership, (group, _, _, _))| {
                Actor::ALL
                    .into_iter()
                    .filter(move |actor| {
                        self.groups.get(group) == Some(&Lifecycle::Active)
                            && self.has_group_admin_role(*group, *actor)
                    })
                    .map(move |actor| (*membership, *group, actor))
            })
            .collect()
    }

    /// Returns active workspace/group pairs that can be linked.
    pub(super) fn workspace_group_candidates(&self) -> Vec<(Handle, Handle, Actor)> {
        self.active_workspace_admins()
            .into_iter()
            .flat_map(|(workspace, actor)| {
                self.active_groups()
                    .into_iter()
                    .filter(move |group| !self.workspace_groups.contains_key(&(workspace, *group)))
                    .map(move |group| (workspace, group, actor))
            })
            .collect()
    }

    /// Returns active workspace-group links and workspace administrators.
    pub(super) fn active_workspace_groups(&self) -> Vec<(Handle, Handle, Actor)> {
        self.workspace_groups
            .iter()
            .filter(|(_, lifecycle)| **lifecycle == Lifecycle::Active)
            .flat_map(|((workspace, group), _)| {
                Actor::ALL
                    .into_iter()
                    .filter(move |actor| self.can_admin_workspace(*workspace, *actor))
                    .map(move |actor| (*workspace, *group, actor))
            })
            .collect()
    }

    /// Returns archived workspace-group links and workspace administrators.
    pub(super) fn archived_workspace_groups(&self) -> Vec<(Handle, Handle, Actor)> {
        self.workspace_groups
            .iter()
            .filter(|((_, group), lifecycle)| {
                **lifecycle == Lifecycle::Archived
                    && self.groups.get(group) == Some(&Lifecycle::Active)
            })
            .flat_map(|((workspace, group), _)| {
                Actor::ALL
                    .into_iter()
                    .filter(move |actor| self.can_admin_workspace(*workspace, *actor))
                    .map(move |actor| (*workspace, *group, actor))
            })
            .collect()
    }

    /// Returns candidates for new group-backed object grants.
    pub(super) fn group_grant_candidates(&self) -> Vec<(Handle, Handle, Actor, Handle)> {
        self.administrable_active_objects()
            .into_iter()
            .flat_map(|(object, workspace, administrator)| {
                self.workspace_groups
                    .iter()
                    .filter(move |((linked_workspace, _), lifecycle)| {
                        *linked_workspace == workspace && **lifecycle == Lifecycle::Active
                    })
                    .filter_map(move |((_, group), _)| {
                        let duplicate =
                            self.grants.values().any(|(_, grant_object, principal, _, active)| {
                                *grant_object == object
                                    && *principal == Principal::Group(*group)
                                    && *active
                            });
                        (self.groups.get(group) == Some(&Lifecycle::Active) && !duplicate)
                            .then_some((object, workspace, administrator, *group))
                    })
            })
            .collect()
    }

    /// Returns active object pairs in one workspace and an actor able to connect them.
    pub(super) fn edge_candidates(&self) -> Vec<(Handle, Handle, Handle, Actor)> {
        self.objects
            .iter()
            .filter(|(_, (workspace, _, lifecycle, _, _))| {
                *lifecycle == Lifecycle::Active
                    && self.workspace(*workspace) == Some(Lifecycle::Active)
            })
            .flat_map(|(source, (workspace, _, _, _, _))| {
                self.objects
                    .iter()
                    .filter(move |(target, (target_workspace, _, lifecycle, _, _))| {
                        **target != *source
                            && *target_workspace == *workspace
                            && *lifecycle == Lifecycle::Active
                            && !self.edges.values().any(|(_, edge_source, edge_target, active)| {
                                *edge_source == *source && *edge_target == **target && *active
                            })
                    })
                    .flat_map(move |(target, _)| {
                        Actor::ALL
                            .into_iter()
                            .filter(move |actor| {
                                self.can_edit_object(*source, *actor)
                                    && self.can_read_object(*target, *actor)
                            })
                            .map(move |actor| (*workspace, *source, *target, actor))
                    })
            })
            .collect()
    }

    /// Returns active objects paired with actors able to upload attachments.
    pub(super) fn attachment_upload_candidates(&self) -> Vec<(Handle, Handle, Actor)> {
        self.objects
            .iter()
            .filter(|(_, (_, _, lifecycle, _, _))| *lifecycle == Lifecycle::Active)
            .flat_map(|(object, (workspace, _, _, _, _))| {
                Actor::ALL
                    .into_iter()
                    .filter(move |actor| self.can_edit_object(*object, *actor))
                    .map(move |actor| (*object, *workspace, actor))
            })
            .collect()
    }

    /// Returns attachments paired with their owner and arbitrary reading actors.
    pub(super) fn attachment_read_candidates(&self) -> Vec<(Handle, Handle, Handle, Actor)> {
        self.attachments
            .iter()
            .flat_map(|(attachment, modeled)| {
                Actor::ALL
                    .into_iter()
                    .map(move |actor| (*attachment, modeled.object, modeled.workspace, actor))
            })
            .collect()
    }

    /// Returns source attachments and active target objects usable for attachment reuse.
    pub(super) fn attachment_reuse_candidates(&self) -> Vec<(Handle, Handle, Handle, Actor)> {
        self.attachments
            .iter()
            .filter(|(_, modeled)| {
                self.object(modeled.object)
                    .is_some_and(|(_, lifecycle)| lifecycle == Lifecycle::Active)
            })
            .flat_map(|(source, modeled)| {
                self.objects
                    .iter()
                    .filter(move |(_, (target_workspace, _, lifecycle, _, _))| {
                        *target_workspace == modeled.workspace && *lifecycle == Lifecycle::Active
                    })
                    .flat_map(move |(target, _)| {
                        Actor::ALL
                            .into_iter()
                            .filter(move |actor| {
                                self.can_read_object(modeled.object, *actor)
                                    && self.can_edit_object(*target, *actor)
                            })
                            .map(move |actor| (*source, *target, modeled.workspace, actor))
                    })
            })
            .collect()
    }

    /// Returns active edges and actors able to revoke them.
    pub(super) fn revocable_edges(&self) -> Vec<(Handle, Handle, Actor)> {
        self.edges
            .iter()
            .filter(|(_, (_, _, _, active))| *active)
            .flat_map(|(edge, (workspace, source, _, _))| {
                Actor::ALL
                    .into_iter()
                    .filter(move |actor| {
                        self.can_edit_object(*source, *actor) && self.can_read_edge(*edge, *actor)
                    })
                    .map(move |actor| (*edge, *workspace, actor))
            })
            .collect()
    }

    /// Returns whether an actor has effective workspace-administrator authority.
    fn has_workspace_admin_role(&self, workspace: Handle, actor: Actor) -> bool {
        actor == Actor::Admin
            || self.workspace_owner(workspace) == Some(actor)
            || self.memberships.values().any(|(member_workspace, member, role, active)| {
                *member_workspace == workspace
                    && *member == actor
                    && *role == MembershipRole::Admin
                    && *active
            })
    }

    /// Returns whether an actor has effective group-administrator authority.
    fn has_group_admin_role(&self, group: Handle, actor: Actor) -> bool {
        actor == Actor::Admin
            || self.group_memberships.values().any(|(member_group, member, role, active)| {
                *member_group == group
                    && *member == actor
                    && *role == MembershipRole::Admin
                    && *active
            })
    }

    /// Returns whether an actor has an active membership in an active group.
    pub(super) fn has_active_group_membership(&self, group: Handle, actor: Actor) -> bool {
        self.groups.get(&group) == Some(&Lifecycle::Active)
            && self.group_memberships.values().any(|(member_group, member, _, active)| {
                *member_group == group && *member == actor && *active
            })
    }

    /// Returns whether a grant principal currently includes an actor.
    fn principal_contains(&self, workspace: Handle, principal: Principal, actor: Actor) -> bool {
        match principal {
            Principal::User(user) => user == actor,
            Principal::Group(group) => {
                self.has_active_group_membership(group, actor)
                    && self.workspace_groups.get(&(workspace, group)) == Some(&Lifecycle::Active)
            }
        }
    }

    /// Returns the number of modeled objects in a workspace.
    fn objects_in_workspace(&self, workspace: Handle) -> usize {
        self.objects
            .values()
            .filter(|(object_workspace, _, _, _, _)| *object_workspace == workspace)
            .count()
    }

    /// Counts active administrative grant rows for an object.
    fn active_admin_grant_count(&self, object: Handle) -> usize {
        self.grants
            .values()
            .filter(|(_, grant_object, _, role, active)| {
                *grant_object == object && *role == ObjectRole::Admin && *active
            })
            .count()
    }

    /// Records one expected event emitted by a successful mutation.
    pub(super) fn record_event(&mut self, event: ModeledEvent) {
        self.events.push(event);
    }
}

/// Chooses the stronger of an existing and candidate object role.
fn strongest_role(current: Option<ObjectRole>, candidate: ObjectRole) -> Option<ObjectRole> {
    let strength = |role| match role {
        ObjectRole::Viewer => 0,
        ObjectRole::Editor => 1,
        ObjectRole::Admin => 2,
    };
    match current {
        Some(role) if strength(role) >= strength(candidate) => Some(role),
        _ => Some(candidate),
    }
}
