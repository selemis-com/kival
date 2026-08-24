//! Group handlers.

mod lifecycle;
mod memberships;

pub(crate) use lifecycle::{
    handle_archive_group, handle_create_group, handle_get_group, handle_list_groups,
    handle_unarchive_group, handle_update_group,
};
pub(crate) use memberships::{
    handle_create_group_membership, handle_list_group_memberships, handle_revoke_group_membership,
    handle_update_group_membership,
};
