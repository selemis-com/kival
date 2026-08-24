//! Object graph handlers.

mod edges;
mod grants;
mod traversal;

pub(crate) use edges::{
    handle_create_edge, handle_get_edge, handle_get_object_backlinks, handle_list_object_edges,
    handle_revoke_edge,
};
pub(crate) use grants::{
    handle_create_object_grant, handle_list_object_grants, handle_revoke_object_grant,
    handle_update_object_grant,
};
pub(crate) use traversal::{handle_get_object_graph, handle_get_workspace_graph};
