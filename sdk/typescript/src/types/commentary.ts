import type { Timestamp, UUID } from "./common.js";
import type { ListResponse } from "./pagination.js";

/** Commentary lifecycle state. */
export type CommentStatus = "active" | "deleted" | "expired";

/** Public identity attached to a comment. */
export type CommentAuthor = {
  id: UUID;
  username: string;
  display_name: string;
};

/** Stable user mention resolved by the server. */
export type CommentMention = {
  user_id: UUID;
  username: string;
  display_name: string;
};

/** Mutable working comment attached to an object, outside object version history. */
export type Comment = {
  id: UUID;
  workspace_id: UUID;
  object_id: UUID;
  thread_id: UUID;
  parent_comment_id: UUID | null;
  author: CommentAuthor;
  status: CommentStatus;
  body: string | null;
  mentions: CommentMention[];
  created_at: Timestamp;
  updated_at: Timestamp;
  edited_at: Timestamp | null;
  deleted_at: Timestamp | null;
  deleted_by: UUID | null;
  expired_at: Timestamp | null;
  retention_expires_at: Timestamp | null;
};

/** Top-level commentary thread with a bounded page of comments. */
export type CommentThread = {
  id: UUID;
  workspace_id: UUID;
  object_id: UUID;
  created_by: UUID;
  created_at: Timestamp;
  updated_at: Timestamp;
  resolved_at: Timestamp | null;
  resolved_by: UUID | null;
  retention_expires_at: Timestamp | null;
  comments: Comment[];
  /** Opaque cursor for loading more comments in this thread. */
  comments_next_cursor?: string;
};

/** User who may currently be mentioned from commentary on an object. */
export type CommentMentionCandidate = {
  user_id: UUID;
  username: string;
  display_name: string;
};

/** Parameters for object-scoped mention autocomplete. */
export type CommentMentionCandidateParams = {
  /** Username prefix or display-name fragment. */
  q?: string;
  /** Maximum candidates to return. Defaults to 8 and is capped at 20. */
  limit?: number | null;
};

/** Request for a top-level comment or reply. */
export type CreateCommentRequest = {
  body: string;
  mentioned_user_ids?: UUID[];
};

/** Complete replacement for an active comment body and its mentions. */
export type UpdateCommentRequest = CreateCommentRequest;

export type CommentResponse = { comment: Comment };
export type CommentThreadResponse = { thread: CommentThread };
export type CommentThreadListResponse = ListResponse<CommentThread>;
export type CommentListResponse = ListResponse<Comment>;
export type CommentMentionCandidateListResponse = ListResponse<CommentMentionCandidate>;
