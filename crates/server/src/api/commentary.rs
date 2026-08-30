//! Object-scoped commentary handlers.
//!
//! Authorization follows the parent object: viewers may inspect and participate in
//! commentary, authors may edit their own active comments while a thread is open,
//! authors or object admins may delete comments, and thread authors or object admins
//! may resolve and reopen threads.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use axum::{
    Json,
    extract::{Path, State},
};
use chrono::{DateTime, Utc};
use kival_kernel::{
    CommentPageQuery, CommentRow, EventKind, ThreadRow, allowed_mentioned_user_ids,
    comment_mention_ids_in_tx, comment_thread_exists, create_comment, create_comment_thread,
    delete_comment, fetch_comment_for_mutation as fetch_comment_row_for_mutation,
    fetch_comment_mentions, fetch_comment_mentions_for_mutation,
    fetch_comment_thread_for_mutation as fetch_thread_row_for_mutation, fetch_initial_comment_rows,
    fetch_initial_comment_rows_for_mutation, fetch_thread_comment_page_rows, list_comment_threads,
    lock_comment, lock_thread_for_reply, lock_thread_resolution, mention_candidates,
    replace_comment_mentions, resolve_mentioned_usernames, set_thread_resolved,
    touch_comment_thread, update_comment_body,
};
use kival_sdk::{
    COMMENT_BODY_MAX_CHARS, COMMENT_MENTION_MAX_USERS, Comment, CommentAuthor, CommentListResponse,
    CommentMention, CommentMentionCandidate, CommentMentionCandidateListResponse,
    CommentMentionCandidateParams, CommentResponse, CommentThread, CommentThreadListResponse,
    CommentThreadResponse, CreateCommentRequest, ListParams, ListResponse, UpdateCommentRequest,
};
use kival_types::ObjectRole;
use serde_json::json;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

/// Number of initial comments hydrated for each listed thread.
const COMMENT_THREAD_INITIAL_COMMENTS: i64 = 20;

use crate::{
    ServerState,
    api::{
        auth::AuthenticatedUser,
        authz::require_object_role,
        emit::emit_event,
        error::{ApiError, ApiResult},
        json::JsonBody,
        pagination,
        query::QueryParams,
    },
};

#[derive(Clone, Copy, Debug)]
/// Object and commentary identifiers attached to mention events.
struct MentionEventTarget {
    /// Workspace containing the mention.
    workspace_id: Uuid,
    /// Object containing the commentary thread.
    object_id: Uuid,
    /// Commentary thread containing the mention.
    thread_id: Uuid,
    /// Comment that introduced the mention.
    comment_id: Uuid,
}

/// Lists commentary visible through the parent object.
pub(crate) async fn handle_list_commentary(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
    QueryParams(params): QueryParams<ListParams>,
) -> ApiResult<Json<CommentThreadListResponse>> {
    let cursor = pagination::decode_updated_at(&params, "comment_threads", Some(object_id))?;
    let limit = params.checked_limit().map_err(ApiError::bad_request)?;
    let rows = list_comment_threads(
        state.db(),
        workspace_id,
        object_id,
        actor.id,
        cursor.as_ref().map(|value| value.updated_at),
        cursor.as_ref().map(|value| value.id),
        limit + 1,
    )
    .await?;

    let page =
        pagination::updated_at_page(rows, limit, "comment_threads", Some(object_id), |thread| {
            (thread.updated_at, thread.id)
        })?;
    let threads = hydrate_threads(state.db(), actor.id, page.items).await?;

    Ok(Json(ListResponse { items: threads, next_cursor: page.next_cursor }))
}

/// Lists a bounded page of comments in one commentary thread.
pub(crate) async fn handle_list_thread_comments(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id, thread_id)): Path<(Uuid, Uuid, Uuid)>,
    QueryParams(params): QueryParams<ListParams>,
) -> ApiResult<Json<CommentListResponse>> {
    let cursor = pagination::decode_created_at(&params, "comments", Some(thread_id))?;
    let limit = params.checked_limit().map_err(ApiError::bad_request)?;
    let rows = fetch_existing_thread_comment_page_rows(
        state.db(),
        CommentPageQuery {
            workspace_id,
            object_id,
            actor_id: actor.id,
            thread_id,
            cursor_created_at: cursor.as_ref().map(|value| value.created_at),
            cursor_id: cursor.as_ref().map(|value| value.id),
            limit: limit + 1,
        },
    )
    .await?;
    let comment_ids = rows.iter().map(|row| row.id).collect::<Vec<_>>();
    let mentions =
        fetch_mentions(state.db(), actor.id, workspace_id, object_id, &comment_ids).await?;
    let comments = rows
        .into_iter()
        .map(|row| {
            let comment_id = row.id;
            wire_comment(row, mentions.get(&comment_id).cloned().unwrap_or_default())
        })
        .collect::<Vec<_>>();
    let page =
        pagination::created_at_page(comments, limit, "comments", Some(thread_id), |comment| {
            (comment.created_at, comment.id)
        })?;

    Ok(Json(page))
}

/// Lists users who may currently be mentioned from commentary on an object.
pub(crate) async fn handle_list_mention_candidates(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
    QueryParams(params): QueryParams<CommentMentionCandidateParams>,
) -> ApiResult<Json<CommentMentionCandidateListResponse>> {
    let limit = params.checked_limit().map_err(ApiError::bad_request)?;
    let needle = params.q.trim().to_ascii_lowercase();
    if needle.len() > 30
        || needle.chars().next().is_some_and(|character| !character.is_ascii_alphanumeric())
        || !needle.chars().all(is_username_character)
    {
        return Ok(Json(ListResponse::new(Vec::new())));
    }

    let rows =
        mention_candidates(state.db(), workspace_id, object_id, actor.id, &needle, limit).await?;

    Ok(Json(ListResponse::new(
        rows.into_iter()
            .map(|(user_id, username, display_name)| CommentMentionCandidate {
                user_id,
                username,
                display_name,
            })
            .collect(),
    )))
}

/// Creates a new top-level commentary thread.
pub(crate) async fn handle_create_comment(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id)): Path<(Uuid, Uuid)>,
    JsonBody(request): JsonBody<CreateCommentRequest>,
) -> ApiResult<Json<CommentThreadResponse>> {
    require_object_role(state.db(), actor.id, workspace_id, object_id, ObjectRole::Viewer).await?;
    let body = validate_body(&request.body)?;
    let explicit_mentions = normalize_mentions(request.mentioned_user_ids)?;

    let mut tx = state.db().begin().await?;
    let thread_id = create_comment_thread(&mut tx, workspace_id, object_id, actor.id).await?;
    let mentions =
        resolve_mentions(&mut tx, workspace_id, object_id, &body, &explicit_mentions).await?;

    let comment_id =
        create_comment(&mut tx, workspace_id, object_id, thread_id, None, actor.id, &body).await?;
    replace_comment_mentions(&mut tx, workspace_id, object_id, comment_id, &mentions).await?;

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(
                EventKind::CommentCreated,
                json!({ "thread_id": thread_id, "comment_id": comment_id }),
            )
            .workspace(workspace_id)
            .object(object_id)
            .comment_thread(thread_id)
            .comment(comment_id),
    )
    .await?;
    emit_mention_events(
        &mut tx,
        state.durable_tasks().queue(),
        &actor,
        MentionEventTarget { workspace_id, object_id, thread_id, comment_id },
        &mentions,
    )
    .await?;
    let thread = fetch_thread_for_mutation(&mut tx, workspace_id, object_id, thread_id).await?;
    tx.commit().await?;

    Ok(Json(CommentThreadResponse { thread }))
}

/// Replies to an existing comment while retaining one thread root.
pub(crate) async fn handle_reply_to_thread(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id, thread_id)): Path<(Uuid, Uuid, Uuid)>,
    JsonBody(request): JsonBody<CreateCommentRequest>,
) -> ApiResult<Json<CommentResponse>> {
    require_object_role(state.db(), actor.id, workspace_id, object_id, ObjectRole::Viewer).await?;
    let body = validate_body(&request.body)?;
    let explicit_mentions = normalize_mentions(request.mentioned_user_ids)?;

    let mut tx = state.db().begin().await?;
    let thread = lock_thread_for_reply(&mut tx, workspace_id, object_id, thread_id)
        .await?
        .ok_or_else(|| ApiError::not_found("comment thread not found"))?;
    let (root_comment_id, root_author_user_id, resolved_at) = thread;
    if resolved_at.is_some() {
        return Err(ApiError::conflict(
            "resolved comment threads must be reopened before replying",
        ));
    }

    let mentions =
        resolve_mentions(&mut tx, workspace_id, object_id, &body, &explicit_mentions).await?;

    let comment_id = create_comment(
        &mut tx,
        workspace_id,
        object_id,
        thread_id,
        Some(root_comment_id),
        actor.id,
        &body,
    )
    .await?;
    replace_comment_mentions(&mut tx, workspace_id, object_id, comment_id, &mentions).await?;
    touch_comment_thread(&mut tx, thread_id).await?;

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(
                EventKind::CommentReplied,
                json!({
                    "thread_id": thread_id,
                    "comment_id": comment_id,
                    "parent_comment_id": root_comment_id,
                }),
            )
            .workspace(workspace_id)
            .object(object_id)
            .comment_thread(thread_id)
            .comment(comment_id)
            .target_user(root_author_user_id),
    )
    .await?;
    emit_mention_events(
        &mut tx,
        state.durable_tasks().queue(),
        &actor,
        MentionEventTarget { workspace_id, object_id, thread_id, comment_id },
        &mentions,
    )
    .await?;
    let comment = fetch_comment_for_mutation(&mut tx, workspace_id, object_id, comment_id).await?;
    tx.commit().await?;

    Ok(Json(CommentResponse { comment }))
}

/// Edits an active comment in an open thread. Text remains author-controlled.
pub(crate) async fn handle_update_comment(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id, comment_id)): Path<(Uuid, Uuid, Uuid)>,
    JsonBody(request): JsonBody<UpdateCommentRequest>,
) -> ApiResult<Json<CommentResponse>> {
    require_object_role(state.db(), actor.id, workspace_id, object_id, ObjectRole::Viewer).await?;
    let body = validate_body(&request.body)?;
    let explicit_mentions = normalize_mentions(request.mentioned_user_ids)?;

    let mut tx = state.db().begin().await?;
    let (thread_id, author_user_id, deleted_at, expired_at, resolved_at) =
        require_comment_locked(&mut tx, workspace_id, object_id, comment_id).await?;
    if author_user_id != actor.id {
        return Err(ApiError::forbidden("only the comment author may edit commentary"));
    }
    if deleted_at.is_some() || expired_at.is_some() {
        return Err(ApiError::conflict("inactive commentary cannot be edited"));
    }
    if resolved_at.is_some() {
        return Err(ApiError::conflict("resolved comment threads must be reopened before editing"));
    }

    let mentions =
        resolve_mentions(&mut tx, workspace_id, object_id, &body, &explicit_mentions).await?;
    let previous_mentions =
        comment_mention_ids_in_tx(&mut tx, comment_id).await?.into_iter().collect::<BTreeSet<_>>();

    update_comment_body(&mut tx, workspace_id, object_id, comment_id, &body).await?;
    replace_comment_mentions(&mut tx, workspace_id, object_id, comment_id, &mentions).await?;
    touch_comment_thread(&mut tx, thread_id).await?;
    let newly_mentioned = mentions
        .iter()
        .copied()
        .filter(|user_id| !previous_mentions.contains(user_id))
        .collect::<Vec<_>>();

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(
                EventKind::CommentEdited,
                json!({ "thread_id": thread_id, "comment_id": comment_id }),
            )
            .workspace(workspace_id)
            .object(object_id)
            .comment_thread(thread_id)
            .comment(comment_id),
    )
    .await?;
    emit_mention_events(
        &mut tx,
        state.durable_tasks().queue(),
        &actor,
        MentionEventTarget { workspace_id, object_id, thread_id, comment_id },
        &newly_mentioned,
    )
    .await?;
    let comment = fetch_comment_for_mutation(&mut tx, workspace_id, object_id, comment_id).await?;
    tx.commit().await?;

    Ok(Json(CommentResponse { comment }))
}

/// Soft-deletes a comment, preserving its position for existing replies.
pub(crate) async fn handle_delete_comment(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id, comment_id)): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<Json<CommentResponse>> {
    let role =
        require_object_role(state.db(), actor.id, workspace_id, object_id, ObjectRole::Viewer)
            .await?;

    let mut tx = state.db().begin().await?;
    let (thread_id, author_user_id, deleted_at, expired_at, _resolved_at) =
        require_comment_locked(&mut tx, workspace_id, object_id, comment_id).await?;
    if actor.id != author_user_id && role != ObjectRole::Admin {
        return Err(ApiError::forbidden("comment author or object admin access required"));
    }
    if deleted_at.is_some() || expired_at.is_some() {
        return Err(ApiError::conflict("comment body is already unavailable"));
    }

    delete_comment(&mut tx, workspace_id, object_id, comment_id, actor.id).await?;
    touch_comment_thread(&mut tx, thread_id).await?;

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(
                EventKind::CommentDeleted,
                json!({ "thread_id": thread_id, "comment_id": comment_id }),
            )
            .workspace(workspace_id)
            .object(object_id)
            .comment_thread(thread_id)
            .comment(comment_id),
    )
    .await?;
    let comment = fetch_comment_for_mutation(&mut tx, workspace_id, object_id, comment_id).await?;
    tx.commit().await?;

    Ok(Json(CommentResponse { comment }))
}

/// Resolves a thread. Only its root author or an object admin may do so.
pub(crate) async fn handle_resolve_thread(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id, thread_id)): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<Json<CommentThreadResponse>> {
    set_thread_resolution(state, actor, workspace_id, object_id, thread_id, true).await
}

/// Reopens a resolved thread.
pub(crate) async fn handle_reopen_thread(
    State(state): State<Arc<ServerState>>,
    actor: AuthenticatedUser,
    Path((workspace_id, object_id, thread_id)): Path<(Uuid, Uuid, Uuid)>,
) -> ApiResult<Json<CommentThreadResponse>> {
    set_thread_resolution(state, actor, workspace_id, object_id, thread_id, false).await
}

/// Applies a thread resolution transition and emits its activity event.
async fn set_thread_resolution(
    state: Arc<ServerState>,
    actor: AuthenticatedUser,
    workspace_id: Uuid,
    object_id: Uuid,
    thread_id: Uuid,
    resolve: bool,
) -> ApiResult<Json<CommentThreadResponse>> {
    let role =
        require_object_role(state.db(), actor.id, workspace_id, object_id, ObjectRole::Viewer)
            .await?;

    let mut tx = state.db().begin().await?;
    let (root_author, resolved_at) =
        lock_thread_resolution(&mut tx, workspace_id, object_id, thread_id)
            .await?
            .ok_or_else(|| ApiError::not_found("comment thread not found"))?;

    if actor.id != root_author && role != ObjectRole::Admin {
        return Err(ApiError::forbidden("thread author or object admin access required"));
    }
    let already_in_requested_state =
        (resolve && resolved_at.is_some()) || (!resolve && resolved_at.is_none());
    if already_in_requested_state {
        return Err(ApiError::conflict(if resolve {
            "comment thread is already resolved"
        } else {
            "comment thread is already open"
        }));
    }

    set_thread_resolved(&mut tx, workspace_id, object_id, thread_id, actor.id, resolve).await?;

    emit_event(
        &mut tx,
        state.durable_tasks().queue(),
        actor
            .event(
                if resolve {
                    EventKind::CommentThreadResolved
                } else {
                    EventKind::CommentThreadReopened
                },
                json!({ "thread_id": thread_id }),
            )
            .workspace(workspace_id)
            .object(object_id)
            .comment_thread(thread_id),
    )
    .await?;
    let thread = fetch_thread_for_mutation(&mut tx, workspace_id, object_id, thread_id).await?;
    tx.commit().await?;

    Ok(Json(CommentThreadResponse { thread }))
}

/// Normalizes and validates a comment body.
fn validate_body(body: &str) -> ApiResult<String> {
    let body = body.trim().to_owned();
    if body.is_empty() {
        return Err(ApiError::bad_request("comment body must not be blank"));
    }
    if body.chars().count() > COMMENT_BODY_MAX_CHARS {
        return Err(ApiError::bad_request(format!(
            "comment body must not exceed {COMMENT_BODY_MAX_CHARS} characters"
        )));
    }
    Ok(body)
}

/// Deduplicates mention identifiers and enforces the mention limit.
fn normalize_mentions(mentions: Vec<Uuid>) -> ApiResult<Vec<Uuid>> {
    let mentions = mentions.into_iter().collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>();
    if mentions.len() > COMMENT_MENTION_MAX_USERS {
        return Err(ApiError::bad_request(format!(
            "a comment may mention at most {COMMENT_MENTION_MAX_USERS} users"
        )));
    }
    Ok(mentions)
}

/// Resolves and authorizes explicit and username-derived mentions.
async fn resolve_mentions(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    body: &str,
    explicit_mentions: &[Uuid],
) -> ApiResult<Vec<Uuid>> {
    let usernames = mentioned_usernames(body);
    let normalized_usernames = usernames.into_iter().collect::<Vec<_>>();

    let resolved = if normalized_usernames.is_empty() {
        Vec::new()
    } else {
        resolve_mentioned_usernames(tx, workspace_id, &normalized_usernames, object_id).await?
    };

    if resolved.len() != normalized_usernames.len() {
        return Err(ApiError::bad_request(
            "one or more mentioned users do not exist or cannot access this object",
        ));
    }

    let mut mentions = explicit_mentions.iter().copied().collect::<BTreeSet<_>>();
    mentions.extend(resolved.into_iter().map(|(user_id, _)| user_id));

    if mentions.len() > COMMENT_MENTION_MAX_USERS {
        return Err(ApiError::bad_request(format!(
            "a comment may mention at most {COMMENT_MENTION_MAX_USERS} users"
        )));
    }

    if !explicit_mentions.is_empty() {
        let allowed = allowed_mentioned_user_ids(tx, workspace_id, explicit_mentions, object_id)
            .await?
            .into_iter()
            .collect::<BTreeSet<_>>();

        if explicit_mentions.iter().any(|user_id| !allowed.contains(user_id)) {
            return Err(ApiError::bad_request(
                "one or more mentioned users do not exist or cannot access this object",
            ));
        }
    }

    Ok(mentions.into_iter().collect())
}

/// Extracts normalized `@username` references from comment text.
fn mentioned_usernames(body: &str) -> BTreeSet<String> {
    let mut usernames = BTreeSet::new();
    let mut characters = body.char_indices().peekable();
    let mut previous = None;

    while let Some((_, character)) = characters.next() {
        if character != '@' {
            previous = Some(character);
            continue;
        }
        if previous.is_some_and(is_username_character) {
            previous = Some(character);
            continue;
        }

        let mut username = String::new();
        while let Some(&(_, next)) = characters.peek() {
            if (username.is_empty() && next.is_ascii_alphanumeric())
                || (!username.is_empty() && is_username_character(next))
            {
                username.push(next.to_ascii_lowercase());
                characters.next();
            } else {
                break;
            }
        }

        while username.ends_with(['.', '_', '-']) {
            username.pop();
        }

        let last_username_character = username.chars().last();
        if !username.is_empty() && username.len() <= 30 {
            usernames.insert(username);
        }
        previous = last_username_character.or(Some(character));
    }

    usernames
}

/// Returns whether a character may appear in a mentioned username.
const fn is_username_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
}

/// Emits mention events for users newly mentioned by a comment.
async fn emit_mention_events(
    tx: &mut Transaction<'_, Postgres>,
    durable_queue: &steda::Queue,
    actor: &AuthenticatedUser,
    target: MentionEventTarget,
    mentions: &[Uuid],
) -> ApiResult<()> {
    for user_id in mentions {
        if *user_id == actor.id {
            continue;
        }
        emit_event(
            tx,
            durable_queue,
            actor
                .event(
                    EventKind::CommentMentioned,
                    json!({ "thread_id": target.thread_id, "comment_id": target.comment_id }),
                )
                .workspace(target.workspace_id)
                .object(target.object_id)
                .comment_thread(target.thread_id)
                .comment(target.comment_id)
                .target_user(*user_id),
        )
        .await?;
    }
    Ok(())
}

/// Locks and loads a comment before mutation.
async fn require_comment_locked(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    comment_id: Uuid,
) -> ApiResult<(Uuid, Uuid, Option<DateTime<Utc>>, Option<DateTime<Utc>>, Option<DateTime<Utc>>)> {
    lock_comment(tx, workspace_id, object_id, comment_id)
        .await?
        .ok_or_else(|| ApiError::not_found("comment not found"))
}

/// Hydrates a thread response for a mutation that already passed object admission.
async fn fetch_thread_for_mutation(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    thread_id: Uuid,
) -> ApiResult<CommentThread> {
    let row = fetch_thread_row_for_mutation(tx, workspace_id, object_id, thread_id)
        .await?
        .ok_or_else(|| ApiError::not_found("comment thread not found"))?;
    let rows = fetch_initial_comment_rows_for_mutation(
        tx,
        workspace_id,
        object_id,
        thread_id,
        COMMENT_THREAD_INITIAL_COMMENTS + 1,
    )
    .await?;
    let comment_ids = rows.iter().map(|row| row.id).collect::<Vec<_>>();
    let mentions = fetch_mentions_for_mutation(tx, workspace_id, object_id, &comment_ids).await?;
    let comments = rows
        .into_iter()
        .map(|row| {
            let comment_id = row.id;
            wire_comment(row, mentions.get(&comment_id).cloned().unwrap_or_default())
        })
        .collect::<Vec<_>>();
    let page = pagination::created_at_page(
        comments,
        COMMENT_THREAD_INITIAL_COMMENTS,
        "comments",
        Some(row.id),
        |comment| (comment.created_at, comment.id),
    )?;

    Ok(CommentThread {
        id: row.id,
        workspace_id: row.workspace_id,
        object_id: row.object_id,
        created_by: row.created_by,
        created_at: row.created_at,
        updated_at: row.updated_at,
        resolved_at: row.resolved_at,
        resolved_by: row.resolved_by,
        retention_expires_at: row.retention_expires_at,
        comments: page.items,
        comments_next_cursor: page.next_cursor,
    })
}

/// Hydrates a comment response for a mutation that already passed object admission.
async fn fetch_comment_for_mutation(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    comment_id: Uuid,
) -> ApiResult<Comment> {
    let row = fetch_comment_row_for_mutation(tx, workspace_id, object_id, comment_id)
        .await?
        .ok_or_else(|| ApiError::not_found("comment not found"))?;
    let mentions = fetch_mentions_for_mutation(tx, workspace_id, object_id, &[comment_id]).await?;
    Ok(wire_comment(row, mentions.get(&comment_id).cloned().unwrap_or_default()))
}

/// Loads mentions for an already-admitted mutation response.
async fn fetch_mentions_for_mutation(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    comment_ids: &[Uuid],
) -> ApiResult<BTreeMap<Uuid, Vec<CommentMention>>> {
    let rows =
        fetch_comment_mentions_for_mutation(tx, workspace_id, object_id, comment_ids).await?;
    let mut result = BTreeMap::<Uuid, Vec<CommentMention>>::new();
    for row in rows {
        result.entry(row.comment_id).or_default().push(CommentMention {
            user_id: row.user_id,
            username: row.username,
            display_name: row.display_name,
        });
    }
    Ok(result)
}

/// Hydrates thread rows with their initial comments and mentions.
async fn hydrate_threads(
    pool: &PgPool,
    actor_id: Uuid,
    rows: Vec<ThreadRow>,
) -> ApiResult<Vec<CommentThread>> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let thread_ids = rows.iter().map(|row| row.id).collect::<Vec<_>>();
    let comments = fetch_initial_comment_rows(
        pool,
        rows[0].workspace_id,
        rows[0].object_id,
        actor_id,
        &thread_ids,
        COMMENT_THREAD_INITIAL_COMMENTS + 1,
    )
    .await?;
    let comment_ids = comments.iter().map(|row| row.id).collect::<Vec<_>>();
    let mentions =
        fetch_mentions(pool, actor_id, rows[0].workspace_id, rows[0].object_id, &comment_ids)
            .await?;
    let mut comments_by_thread = BTreeMap::<Uuid, Vec<Comment>>::new();
    for row in comments {
        let comment_id = row.id;
        comments_by_thread
            .entry(row.thread_id)
            .or_default()
            .push(wire_comment(row, mentions.get(&comment_id).cloned().unwrap_or_default()));
    }

    rows.into_iter()
        .map(|row| {
            let page = pagination::created_at_page(
                comments_by_thread.remove(&row.id).unwrap_or_default(),
                COMMENT_THREAD_INITIAL_COMMENTS,
                "comments",
                Some(row.id),
                |comment| (comment.created_at, comment.id),
            )?;

            Ok(CommentThread {
                id: row.id,
                workspace_id: row.workspace_id,
                object_id: row.object_id,
                created_by: row.created_by,
                created_at: row.created_at,
                updated_at: row.updated_at,
                resolved_at: row.resolved_at,
                resolved_by: row.resolved_by,
                retention_expires_at: row.retention_expires_at,
                comments: page.items,
                comments_next_cursor: page.next_cursor,
            })
        })
        .collect()
}

/// Loads one paginated comment window for a thread.
async fn fetch_existing_thread_comment_page_rows(
    pool: &PgPool,
    query: CommentPageQuery,
) -> ApiResult<Vec<CommentRow>> {
    if !comment_thread_exists(
        pool,
        query.workspace_id,
        query.object_id,
        query.actor_id,
        query.thread_id,
    )
    .await?
    {
        return Err(ApiError::not_found("comment thread not found"));
    }
    Ok(fetch_thread_comment_page_rows(pool, query).await?)
}

/// Loads and groups mentions for the supplied comment identifiers.
async fn fetch_mentions(
    pool: &PgPool,
    actor_id: Uuid,
    workspace_id: Uuid,
    object_id: Uuid,
    comment_ids: &[Uuid],
) -> ApiResult<BTreeMap<Uuid, Vec<CommentMention>>> {
    let rows = fetch_comment_mentions(pool, actor_id, workspace_id, object_id, comment_ids).await?;
    let mut result = BTreeMap::<Uuid, Vec<CommentMention>>::new();
    for row in rows {
        result.entry(row.comment_id).or_default().push(CommentMention {
            user_id: row.user_id,
            username: row.username,
            display_name: row.display_name,
        });
    }
    Ok(result)
}

/// Converts a kernel comment row and its mentions into the wire representation.
fn wire_comment(row: CommentRow, mentions: Vec<CommentMention>) -> Comment {
    let status = row.status();

    Comment {
        id: row.id,
        workspace_id: row.workspace_id,
        object_id: row.object_id,
        thread_id: row.thread_id,
        parent_comment_id: row.parent_comment_id,
        author: CommentAuthor {
            id: row.author_user_id,
            username: row.author_username,
            display_name: row.author_display_name,
        },
        status,
        body: row.body,
        mentions,
        created_at: row.created_at,
        updated_at: row.updated_at,
        edited_at: row.edited_at,
        deleted_at: row.deleted_at,
        deleted_by: row.deleted_by,
        expired_at: row.expired_at,
        retention_expires_at: row.retention_expires_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_is_trimmed() {
        assert_eq!(validate_body("  discussion  ").unwrap(), "discussion");
    }

    #[test]
    fn mention_usernames_are_extracted() {
        assert_eq!(
            mentioned_usernames("Ask @Alice and @ops-team; ignore @. and alice@example.com"),
            BTreeSet::from(["alice".to_owned(), "ops-team".to_owned()]),
        );
    }

    #[test]
    fn mention_usernames_exclude_trailing_punctuation() {
        assert_eq!(
            mentioned_usernames("Ask @Alice. Then notify @ops_team- and @release.captain."),
            BTreeSet::from([
                "alice".to_owned(),
                "ops_team".to_owned(),
                "release.captain".to_owned(),
            ]),
        );
    }

    #[test]
    fn maximum_length_mentions_allow_sentence_punctuation() {
        let username = "a".repeat(30);
        assert_eq!(
            mentioned_usernames(&format!("Please review this, @{username}.")),
            BTreeSet::from([username]),
        );
    }

    #[test]
    fn body_limit_counts_unicode_scalar_values() {
        assert!(validate_body(&"é".repeat(COMMENT_BODY_MAX_CHARS)).is_ok());
        assert!(validate_body(&"é".repeat(COMMENT_BODY_MAX_CHARS + 1)).is_err());
    }

    #[test]
    fn mention_limit_applies_after_deduplication() {
        let ids = (0..COMMENT_MENTION_MAX_USERS).map(|_| Uuid::now_v7()).collect::<Vec<_>>();
        assert!(normalize_mentions(ids.clone()).is_ok());

        let mut too_many = ids;
        too_many.push(Uuid::now_v7());
        assert!(normalize_mentions(too_many).is_err());

        let id = Uuid::now_v7();
        assert_eq!(normalize_mentions(vec![id, id]).unwrap(), vec![id]);
    }
}
