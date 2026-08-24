import { jsonRequest, listParams, pathId, requestInit, withParams } from "../internal/utils.js";
import type {
  Comment,
  CommentListResponse,
  CommentMentionCandidateListResponse,
  CommentMentionCandidateParams,
  CommentResponse,
  CommentThread,
  CommentThreadListResponse,
  CommentThreadResponse,
  CreateCommentRequest,
  ListParams,
  UpdateCommentRequest,
  UUID,
} from "../types.js";
import type { KivalClientBase, WithSignal } from "./types.js";

/** Parameters for {@link listObjectCommentary}. */
export type ListObjectCommentaryParameters = WithSignal<
  ListParams & {
    workspaceId: UUID;
    objectId: UUID;
  }
>;

/** Return type for {@link listObjectCommentary}. */
export type ListObjectCommentaryReturnType = CommentThreadListResponse;

/** Lists object commentary ordered by latest thread activity. */
export function listObjectCommentary(
  client: KivalClientBase,
  parameters: ListObjectCommentaryParameters,
): Promise<ListObjectCommentaryReturnType> {
  const { workspaceId, objectId, ...params } = parameters;
  return client.requestJson<ListObjectCommentaryReturnType>(
    withParams(commentaryPath(workspaceId, objectId), listParams(params)),
    requestInit({}, params.signal),
  );
}

/** Parameters for {@link listCommentThreadComments}. */
export type ListCommentThreadCommentsParameters = WithSignal<
  ListParams & {
    workspaceId: UUID;
    objectId: UUID;
    threadId: UUID;
  }
>;

/** Return type for {@link listCommentThreadComments}. */
export type ListCommentThreadCommentsReturnType = CommentListResponse;

/** Lists a page of comments in one commentary thread. */
export function listCommentThreadComments(
  client: KivalClientBase,
  parameters: ListCommentThreadCommentsParameters,
): Promise<ListCommentThreadCommentsReturnType> {
  const { workspaceId, objectId, threadId, ...params } = parameters;
  return client.requestJson<ListCommentThreadCommentsReturnType>(
    withParams(
      `${commentaryPath(workspaceId, objectId)}/${pathId(threadId)}/comments`,
      listParams(params),
    ),
    requestInit({}, params.signal),
  );
}

/** Parameters for {@link listCommentMentionCandidates}. */
export type ListCommentMentionCandidatesParameters = WithSignal<
  CommentMentionCandidateParams & {
    workspaceId: UUID;
    objectId: UUID;
  }
>;

/** Return type for {@link listCommentMentionCandidates}. */
export type ListCommentMentionCandidatesReturnType = CommentMentionCandidateListResponse;

/** Lists active users who can view the object and may be mentioned from commentary. */
export function listCommentMentionCandidates(
  client: KivalClientBase,
  parameters: ListCommentMentionCandidatesParameters,
): Promise<ListCommentMentionCandidatesReturnType> {
  const { workspaceId, objectId, q, limit, signal } = parameters;
  const params = new URLSearchParams();
  if (q) {
    params.set("q", q);
  }
  if (limit != null) {
    params.set("limit", limit.toString());
  }

  return client.requestJson<ListCommentMentionCandidatesReturnType>(
    withParams(`${commentaryPath(workspaceId, objectId)}/mention-candidates`, params),
    requestInit({}, signal),
  );
}

/** Parameters for {@link createCommentThread}. */
export type CreateCommentThreadParameters = WithSignal<{
  workspaceId: UUID;
  objectId: UUID;
  input: CreateCommentRequest;
}>;

/** Return type for {@link createCommentThread}. */
export type CreateCommentThreadReturnType = CommentThread;

/** Creates a top-level commentary thread. */
export function createCommentThread(
  client: KivalClientBase,
  parameters: CreateCommentThreadParameters,
): Promise<CreateCommentThreadReturnType> {
  const { workspaceId, objectId, input, signal } = parameters;
  return client
    .requestJson<CommentThreadResponse>(
      commentaryPath(workspaceId, objectId),
      jsonRequest("POST", input, signal),
    )
    .then((response) => response.thread);
}

/** Parameters for {@link replyToCommentThread}. */
export type ReplyToCommentThreadParameters = WithSignal<{
  workspaceId: UUID;
  objectId: UUID;
  threadId: UUID;
  input: CreateCommentRequest;
}>;

/** Return type for {@link replyToCommentThread}. */
export type ReplyToCommentThreadReturnType = Comment;

/** Replies to an open commentary thread. */
export function replyToCommentThread(
  client: KivalClientBase,
  parameters: ReplyToCommentThreadParameters,
): Promise<ReplyToCommentThreadReturnType> {
  const { workspaceId, objectId, threadId, input, signal } = parameters;
  return client
    .requestJson<CommentResponse>(
      `${commentaryPath(workspaceId, objectId)}/${pathId(threadId)}/replies`,
      jsonRequest("POST", input, signal),
    )
    .then((response) => response.comment);
}

/** Parameters for {@link updateComment}. */
export type UpdateCommentParameters = WithSignal<{
  workspaceId: UUID;
  objectId: UUID;
  commentId: UUID;
  input: UpdateCommentRequest;
}>;

/** Return type for {@link updateComment}. */
export type UpdateCommentReturnType = Comment;

/** Edits an active comment authored by the current user in an open thread. */
export function updateComment(
  client: KivalClientBase,
  parameters: UpdateCommentParameters,
): Promise<UpdateCommentReturnType> {
  const { workspaceId, objectId, commentId, input, signal } = parameters;
  return client
    .requestJson<CommentResponse>(
      `${commentaryPath(workspaceId, objectId)}/comments/${pathId(commentId)}`,
      jsonRequest("PATCH", input, signal),
    )
    .then((response) => response.comment);
}

/** Parameters for {@link deleteComment}. */
export type DeleteCommentParameters = WithSignal<{
  workspaceId: UUID;
  objectId: UUID;
  commentId: UUID;
}>;

/** Return type for {@link deleteComment}. */
export type DeleteCommentReturnType = Comment;

/** Soft-deletes a comment as its author or an object administrator. */
export function deleteComment(
  client: KivalClientBase,
  parameters: DeleteCommentParameters,
): Promise<DeleteCommentReturnType> {
  const { workspaceId, objectId, commentId, signal } = parameters;
  return client
    .requestJson<CommentResponse>(
      `${commentaryPath(workspaceId, objectId)}/comments/${pathId(commentId)}`,
      requestInit({ method: "DELETE" }, signal),
    )
    .then((response) => response.comment);
}

/** Parameters for {@link resolveCommentThread}. */
export type ResolveCommentThreadParameters = WithSignal<{
  workspaceId: UUID;
  objectId: UUID;
  threadId: UUID;
}>;

/** Return type for {@link resolveCommentThread}. */
export type ResolveCommentThreadReturnType = CommentThread;

/** Resolves an open commentary thread. */
export function resolveCommentThread(
  client: KivalClientBase,
  parameters: ResolveCommentThreadParameters,
): Promise<ResolveCommentThreadReturnType> {
  return setCommentThreadResolution(client, parameters, "resolve");
}

/** Parameters for {@link reopenCommentThread}. */
export type ReopenCommentThreadParameters = ResolveCommentThreadParameters;

/** Return type for {@link reopenCommentThread}. */
export type ReopenCommentThreadReturnType = CommentThread;

/** Reopens a resolved commentary thread. */
export function reopenCommentThread(
  client: KivalClientBase,
  parameters: ReopenCommentThreadParameters,
): Promise<ReopenCommentThreadReturnType> {
  return setCommentThreadResolution(client, parameters, "reopen");
}

function setCommentThreadResolution(
  client: KivalClientBase,
  parameters: ResolveCommentThreadParameters,
  action: "resolve" | "reopen",
): Promise<CommentThread> {
  const { workspaceId, objectId, threadId, signal } = parameters;
  return client
    .requestJson<CommentThreadResponse>(
      `${commentaryPath(workspaceId, objectId)}/${pathId(threadId)}/${action}`,
      requestInit({ method: "POST" }, signal),
    )
    .then((response) => response.thread);
}

function commentaryPath(workspaceId: UUID, objectId: UUID) {
  return `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/commentary`;
}
