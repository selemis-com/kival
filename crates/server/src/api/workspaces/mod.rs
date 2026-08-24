//! Workspace handlers.

mod groups;
mod lifecycle;
mod memberships;

pub(crate) use groups::{
    handle_archive_workspace_group, handle_create_workspace_group, handle_list_workspace_groups,
    handle_unarchive_workspace_group,
};
pub(crate) use lifecycle::{
    handle_archive_workspace, handle_create_workspace, handle_get_workspace,
    handle_list_workspaces, handle_unarchive_workspace, handle_update_workspace,
};
pub(crate) use memberships::{
    handle_create_workspace_membership, handle_list_workspace_memberships,
    handle_revoke_workspace_membership, handle_update_workspace_membership,
};
