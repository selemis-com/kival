//! HTTP API routes.

use std::sync::Arc;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware::from_fn_with_state,
    routing::{MethodRouter, delete, get, patch, post},
};
use kival_types::ApiKeyScope;

use crate::ServerState;

mod api_keys;
mod auth;
mod authz;
mod commentary;
mod emit;
pub(crate) mod error;
mod events;
mod favorites;
mod graph;
mod groups;
mod json;
mod maintenance;
mod metrics;
mod notification_tasks;
mod notifications;
pub(crate) use notification_tasks::{
    enqueue_backlog_if_needed as enqueue_notification_backlog_if_needed,
    worker as notification_worker,
};
pub(crate) use notifications::run_retention as run_notification_retention;
mod objects;
mod pagination;
mod passkeys;
mod query;
mod rate_limit;
mod realtime;
pub(crate) use rate_limit::RateLimiter;
pub(crate) use realtime::{RealtimeHub, run_listener as run_realtime_listener};
mod search;
pub mod status;
mod users;
mod validate;
mod workspaces;

use auth::ApiKeyRouteExt;

/// Applies the global admin API-key policy to a method route.
fn admin_api_key<S>(route: MethodRouter<S>) -> MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
{
    route.api_key(ApiKeyScope::Admin)
}

/// Builds the API router.
pub fn router(state: Arc<ServerState>) -> Router {
    use ApiKeyScope::{
        AccessManage, AttachmentRead, AttachmentWrite, EventRead, GraphRead, GraphWrite,
        ObjectRead, ObjectWrite, RealtimeRead, WorkspaceRead, WorkspaceWrite,
    };

    Router::new()
        .route("/readyz", get(status::handle_get_ready))
        .route("/healthz", get(status::handle_get_health))
        .route(
            "/auth/passkey/authentication/options",
            post(passkeys::handle_start_authentication).route_layer(from_fn_with_state(
                state.clone(),
                rate_limit::enforce_authentication_start,
            )),
        )
        .route(
            "/auth/passkey/authentication/finish",
            post(passkeys::handle_finish_authentication).route_layer(from_fn_with_state(
                state.clone(),
                rate_limit::enforce_authentication_finish,
            )),
        )
        .route(
            "/auth/passkey/enrollment/options",
            post(passkeys::handle_start_enrollment).route_layer(from_fn_with_state(
                state.clone(),
                rate_limit::enforce_authentication_start,
            )),
        )
        .route(
            "/auth/passkey/enrollment/finish",
            post(passkeys::handle_finish_enrollment).route_layer(from_fn_with_state(
                state.clone(),
                rate_limit::enforce_authentication_finish,
            )),
        )
        .route("/auth/passkeys", get(passkeys::handle_list_passkeys))
        .route(
            "/auth/passkeys/registration/options",
            post(passkeys::handle_start_registration).route_layer(from_fn_with_state(
                state.clone(),
                rate_limit::enforce_authentication_start,
            )),
        )
        .route(
            "/auth/passkeys/registration/finish",
            post(passkeys::handle_finish_registration).route_layer(from_fn_with_state(
                state.clone(),
                rate_limit::enforce_authentication_finish,
            )),
        )
        .route(
            "/auth/passkeys/fresh/options",
            post(passkeys::handle_start_fresh_authentication).route_layer(from_fn_with_state(
                state.clone(),
                rate_limit::enforce_authentication_start,
            )),
        )
        .route(
            "/auth/passkeys/fresh/finish",
            post(passkeys::handle_finish_fresh_authentication).route_layer(from_fn_with_state(
                state.clone(),
                rate_limit::enforce_authentication_finish,
            )),
        )
        .route("/auth/passkeys/{passkey_id}/revoke", post(passkeys::handle_revoke_passkey))
        .route("/auth/logout", post(auth::handle_logout))
        .route("/auth/whoami", get(users::handle_get_current_user).any_api_key())
        .route("/auth/sessions", get(auth::handle_list_sessions))
        .route("/auth/sessions/{session_id}/revoke", post(auth::handle_revoke_session))
        .route(
            "/auth/api-keys",
            get(api_keys::handle_list_api_keys).post(api_keys::handle_create_api_key),
        )
        .route("/auth/api-keys/{api_key_id}", patch(api_keys::handle_update_api_key))
        .route("/auth/api-keys/{api_key_id}/revoke", post(api_keys::handle_revoke_api_key))
        .route("/inbox", get(notifications::handle_list_inbox))
        .route("/inbox/unread-count", get(notifications::handle_get_inbox_unread_count))
        .route("/inbox/read", post(notifications::handle_mark_inbox_read))
        .route("/inbox/{inbox_entry_id}", patch(notifications::handle_update_inbox_entry))
        .route("/realtime", get(realtime::handle_realtime).api_key(RealtimeRead))
        .route("/events", admin_api_key(get(events::handle_list_events)))
        .route("/users", admin_api_key(get(users::handle_list_users)))
        .route(
            "/users/{user_id}",
            admin_api_key(get(users::handle_get_user).patch(users::handle_update_user)),
        )
        .route("/users/{user_id}/disable", admin_api_key(post(users::handle_disable_user)))
        .route("/users/{user_id}/enable", admin_api_key(post(users::handle_enable_user)))
        .route(
            "/groups",
            admin_api_key(get(groups::handle_list_groups).post(groups::handle_create_group)),
        )
        .route(
            "/groups/{group_id}",
            admin_api_key(get(groups::handle_get_group).patch(groups::handle_update_group)),
        )
        .route("/groups/{group_id}/archive", admin_api_key(post(groups::handle_archive_group)))
        .route("/groups/{group_id}/unarchive", admin_api_key(post(groups::handle_unarchive_group)))
        .route(
            "/groups/{group_id}/memberships",
            admin_api_key(
                get(groups::handle_list_group_memberships)
                    .post(groups::handle_create_group_membership),
            ),
        )
        .route(
            "/groups/{group_id}/memberships/{membership_id}",
            admin_api_key(patch(groups::handle_update_group_membership)),
        )
        .route(
            "/groups/{group_id}/memberships/{membership_id}/revoke",
            admin_api_key(post(groups::handle_revoke_group_membership)),
        )
        // Creating a new security boundary requires an interactive session.
        .route(
            "/workspaces",
            get(workspaces::handle_list_workspaces)
                .api_key(WorkspaceRead)
                .merge(post(workspaces::handle_create_workspace)),
        )
        .route(
            "/workspaces/{workspace_id}",
            get(workspaces::handle_get_workspace).workspace_api_key(WorkspaceRead).merge(
                patch(workspaces::handle_update_workspace).workspace_api_key(WorkspaceWrite),
            ),
        )
        .route(
            "/workspaces/{workspace_id}/archive",
            post(workspaces::handle_archive_workspace).workspace_api_key(WorkspaceWrite),
        )
        .route(
            "/workspaces/{workspace_id}/pin",
            post(favorites::handle_pin_workspace).delete(favorites::handle_unpin_workspace),
        )
        .route(
            "/workspaces/{workspace_id}/unarchive",
            post(workspaces::handle_unarchive_workspace).workspace_api_key(WorkspaceWrite),
        )
        .route(
            "/workspaces/{workspace_id}/events",
            get(events::handle_list_workspace_events).workspace_api_key(EventRead),
        )
        .route(
            "/workspaces/{workspace_id}/search",
            get(search::handle_search_workspace).workspace_api_key(ObjectRead),
        )
        .route(
            "/workspaces/{workspace_id}/graph",
            get(graph::handle_get_workspace_graph).workspace_api_key(GraphRead),
        )
        .route(
            "/workspaces/{workspace_id}/memberships",
            get(workspaces::handle_list_workspace_memberships)
                .post(workspaces::handle_create_workspace_membership)
                .workspace_api_key(AccessManage),
        )
        .route(
            "/workspaces/{workspace_id}/memberships/{membership_id}",
            patch(workspaces::handle_update_workspace_membership).workspace_api_key(AccessManage),
        )
        .route(
            "/workspaces/{workspace_id}/memberships/{membership_id}/revoke",
            post(workspaces::handle_revoke_workspace_membership).workspace_api_key(AccessManage),
        )
        .route(
            "/workspaces/{workspace_id}/groups",
            get(workspaces::handle_list_workspace_groups)
                .post(workspaces::handle_create_workspace_group)
                .workspace_api_key(AccessManage),
        )
        .route(
            "/workspaces/{workspace_id}/groups/{group_id}/archive",
            post(workspaces::handle_archive_workspace_group).workspace_api_key(AccessManage),
        )
        .route(
            "/workspaces/{workspace_id}/groups/{group_id}/unarchive",
            post(workspaces::handle_unarchive_workspace_group).workspace_api_key(AccessManage),
        )
        .route(
            "/workspaces/{workspace_id}/edges",
            post(graph::handle_create_edge).workspace_api_key(GraphWrite),
        )
        .route(
            "/workspaces/{workspace_id}/edges/{edge_id}",
            get(graph::handle_get_edge).workspace_api_key(GraphRead),
        )
        .route(
            "/workspaces/{workspace_id}/edges/{edge_id}/revoke",
            post(graph::handle_revoke_edge).workspace_api_key(GraphWrite),
        )
        .route(
            "/workspaces/{workspace_id}/objects",
            get(objects::handle_list_objects)
                .workspace_api_key(ObjectRead)
                .merge(post(objects::handle_create_object).workspace_api_key(ObjectWrite)),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}",
            get(objects::handle_get_object)
                .workspace_api_key(ObjectRead)
                .merge(patch(objects::handle_update_object).workspace_api_key(ObjectWrite)),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/archive",
            post(objects::handle_archive_object).workspace_api_key(ObjectWrite),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/favorite",
            post(favorites::handle_favorite_object).delete(favorites::handle_unfavorite_object),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/pin",
            post(favorites::handle_pin_object).delete(favorites::handle_unpin_object),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/unarchive",
            post(objects::handle_unarchive_object).workspace_api_key(ObjectWrite),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/events",
            get(events::handle_list_object_events).workspace_api_key(EventRead),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/notification-preference",
            get(notifications::handle_get_object_notification_preference)
                .patch(notifications::handle_update_object_notification_preference),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/commentary",
            get(commentary::handle_list_commentary)
                .workspace_api_key(ObjectRead)
                .merge(post(commentary::handle_create_comment).workspace_api_key(ObjectWrite)),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/commentary/mention-candidates",
            get(commentary::handle_list_mention_candidates).workspace_api_key(ObjectWrite),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/commentary/{thread_id}/comments",
            get(commentary::handle_list_thread_comments).workspace_api_key(ObjectRead),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/commentary/{thread_id}/replies",
            post(commentary::handle_reply_to_thread).workspace_api_key(ObjectWrite),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/commentary/{thread_id}/resolve",
            post(commentary::handle_resolve_thread).workspace_api_key(ObjectWrite),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/commentary/{thread_id}/reopen",
            post(commentary::handle_reopen_thread).workspace_api_key(ObjectWrite),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/commentary/comments/{comment_id}",
            patch(commentary::handle_update_comment)
                .workspace_api_key(ObjectWrite)
                .merge(delete(commentary::handle_delete_comment).workspace_api_key(ObjectWrite)),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/backlinks",
            get(graph::handle_get_object_backlinks).workspace_api_key(GraphRead),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/attachments",
            get(objects::handle_list_attachments).workspace_api_key(AttachmentRead),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/attachments/upload",
            post(objects::handle_upload_attachment)
                .layer(DefaultBodyLimit::disable())
                .workspace_api_key(AttachmentWrite),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/attachments/reuse",
            post(objects::handle_reuse_attachment).workspace_api_key(AttachmentWrite),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/attachments/{attachment_id}",
            get(objects::handle_get_attachment).workspace_api_key(AttachmentRead),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/attachments/{attachment_id}/content",
            get(objects::handle_get_attachment_content).workspace_api_key(AttachmentRead),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/edges",
            get(graph::handle_list_object_edges).workspace_api_key(GraphRead),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/graph",
            get(graph::handle_get_object_graph).workspace_api_key(GraphRead),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/grants",
            get(graph::handle_list_object_grants)
                .post(graph::handle_create_object_grant)
                .workspace_api_key(AccessManage),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/grants/{grant_id}",
            patch(graph::handle_update_object_grant).workspace_api_key(AccessManage),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/grants/{grant_id}/revoke",
            post(graph::handle_revoke_object_grant).workspace_api_key(AccessManage),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/versions",
            get(objects::handle_list_versions).workspace_api_key(ObjectRead),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/versions/{version}",
            get(objects::handle_get_version).workspace_api_key(ObjectRead),
        )
        .route(
            "/workspaces/{workspace_id}/objects/{object_id}/versions/{version}/wikilinks",
            get(objects::handle_get_version_wikilinks).workspace_api_key(ObjectRead),
        )
        .route_layer(from_fn_with_state(state.clone(), auth::enforce_csrf))
        .fallback(status::handle_get_fallback)
        .method_not_allowed_fallback(status::handle_method_not_allowed)
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .with_state(state)
}
