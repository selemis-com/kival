import { KivalTransportError } from "kival-sdk";
import {
  type FormEvent,
  type KeyboardEvent,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";
import {
  createCommentThread,
  deleteComment,
  listCommentMentionCandidates,
  listCommentThreadComments,
  listObjectCommentary,
  reopenCommentThread,
  replyToCommentThread,
  resolveCommentThread,
  updateComment,
} from "../../../shared/api";
import { submitFormOnEnter } from "../../../shared/forms";
import { KIVAL_REALTIME_EVENT } from "../../../shared/realtime";
import { styles } from "../../../shared/styles";
import type {
  Comment,
  CommentMention,
  CommentMentionCandidate,
  CommentThread,
  ObjectRole,
  RealtimeMessage,
} from "../../../shared/types";
import { LoadingIndicator } from "../../../shared/ui/LoadingIndicator";

type Props = {
  workspaceId: string;
  objectId: string;
  currentUserId: string;
  effectiveRole: ObjectRole;
  archived: boolean;
  targetCommentId?: string | null;
  targetThreadId?: string | null;
};

type MentionToken = {
  start: number;
  end: number;
  query: string;
};

type ComposerProps = {
  workspaceId: string;
  objectId: string;
  initialBody?: string;
  initialMentions?: CommentMention[];
  label: string;
  submitLabel: string;
  onCancel?: () => void;
  onSubmitted?: () => void;
  onSubmit: (body: string, mentionedUserIds: string[]) => Promise<void>;
};

const USERNAME_CHARACTER = /[A-Za-z0-9._-]/;
const MENTION_PATTERN = /(?:^|[^A-Za-z0-9._-])@([A-Za-z0-9](?:[A-Za-z0-9._-]*[A-Za-z0-9])?)/g;

function formatCommentDate(value: string) {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : "Commentary action failed.";
}

function compareComments(left: Comment, right: Comment) {
  const timestamp = new Date(left.created_at).getTime() - new Date(right.created_at).getTime();
  return timestamp || left.id.localeCompare(right.id);
}

function isAbortError(error: unknown) {
  return error instanceof KivalTransportError && error.kind === "abort";
}

function activeMentionToken(body: string, caret: number): MentionToken | null {
  const beforeCaret = body.slice(0, caret);
  const match = /(?:^|[^A-Za-z0-9._-])@([A-Za-z0-9._-]*)$/.exec(beforeCaret);
  if (!match) {
    return null;
  }

  const query = match[1] ?? "";
  if (
    query.length > 30 ||
    (query.length > 0 && !/[A-Za-z0-9]/.test(query[0] ?? "")) ||
    [...query].some((character) => !USERNAME_CHARACTER.test(character))
  ) {
    return null;
  }

  const matchStart = match.index ?? 0;
  const atOffset = match[0].lastIndexOf("@");
  return {
    start: matchStart + atOffset,
    end: caret,
    query: query.toLowerCase(),
  };
}

function mentionedUsernames(body: string) {
  const usernames = new Set<string>();
  for (const match of body.matchAll(MENTION_PATTERN)) {
    if (match[1]) {
      usernames.add(match[1].toLowerCase());
    }
  }
  return usernames;
}

function Composer({
  workspaceId,
  objectId,
  initialBody = "",
  initialMentions = [],
  label,
  submitLabel,
  onCancel,
  onSubmitted,
  onSubmit,
}: ComposerProps) {
  const [body, setBody] = useState(initialBody);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [mentionToken, setMentionToken] = useState<MentionToken | null>(null);
  const [mentionCandidates, setMentionCandidates] = useState<CommentMentionCandidate[]>([]);
  const [mentionCandidatesLoading, setMentionCandidatesLoading] = useState(false);
  const [mentionCandidatesError, setMentionCandidatesError] = useState<string | null>(null);
  const [highlightedCandidateIndex, setHighlightedCandidateIndex] = useState(0);
  const [selectedMentions, setSelectedMentions] = useState<Map<string, CommentMentionCandidate>>(
    () =>
      new Map(
        initialMentions.map((mention) => [
          mention.username.toLowerCase(),
          {
            user_id: mention.user_id,
            username: mention.username,
            display_name: mention.display_name,
          },
        ]),
      ),
  );
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const mentionListId = useId();

  useEffect(() => {
    if (!mentionToken || submitting) {
      setMentionCandidates([]);
      setMentionCandidatesLoading(false);
      setMentionCandidatesError(null);
      return;
    }

    const controller = new AbortController();
    setMentionCandidates([]);
    setMentionCandidatesLoading(true);
    setMentionCandidatesError(null);
    const timeout = window.setTimeout(() => {
      void listCommentMentionCandidates(
        workspaceId,
        objectId,
        mentionToken.query,
        controller.signal,
      )
        .then((response) => {
          setMentionCandidates(response.items);
          setHighlightedCandidateIndex(0);
        })
        .catch((cause: unknown) => {
          if (!isAbortError(cause)) {
            setMentionCandidates([]);
            setMentionCandidatesError(errorMessage(cause));
          }
        })
        .finally(() => {
          if (!controller.signal.aborted) {
            setMentionCandidatesLoading(false);
          }
        });
    }, 120);

    return () => {
      window.clearTimeout(timeout);
      controller.abort();
    };
  }, [mentionToken, objectId, submitting, workspaceId]);

  function updateMentionAtCaret(nextBody: string, caret: number | null) {
    setMentionToken(activeMentionToken(nextBody, caret ?? nextBody.length));
  }

  function chooseMention(candidate: CommentMentionCandidate) {
    if (!mentionToken || submitting) {
      return;
    }

    const replacement = `@${candidate.username} `;
    const nextBody = `${body.slice(0, mentionToken.start)}${replacement}${body.slice(
      mentionToken.end,
    )}`;
    const nextCaret = mentionToken.start + replacement.length;

    setBody(nextBody);
    setSelectedMentions((current) => {
      const next = new Map(current);
      next.set(candidate.username.toLowerCase(), candidate);
      return next;
    });
    setMentionToken(null);
    setMentionCandidates([]);
    setMentionCandidatesError(null);

    window.requestAnimationFrame(() => {
      textareaRef.current?.focus();
      textareaRef.current?.setSelectionRange(nextCaret, nextCaret);
    });
  }

  function handleComposerKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (!mentionToken || mentionCandidates.length === 0) {
      if (event.key === "Escape") {
        setMentionToken(null);
      }
      submitFormOnEnter(event);
      return;
    }

    if (event.key === "ArrowDown") {
      event.preventDefault();
      setHighlightedCandidateIndex((current) => (current + 1) % mentionCandidates.length);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setHighlightedCandidateIndex(
        (current) => (current - 1 + mentionCandidates.length) % mentionCandidates.length,
      );
    } else if ((event.key === "Enter" && !event.shiftKey) || event.key === "Tab") {
      event.preventDefault();
      const candidate = mentionCandidates[highlightedCandidateIndex];
      if (candidate) {
        chooseMention(candidate);
      }
    } else if (event.key === "Escape") {
      event.preventDefault();
      setMentionToken(null);
      setMentionCandidates([]);
    }

    if (!event.defaultPrevented) {
      submitFormOnEnter(event);
    }
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    const normalizedBody = body.trim();

    if (!normalizedBody || submitting) {
      return;
    }

    const bodyMentions = mentionedUsernames(normalizedBody);
    const mentionedUserIds = [...selectedMentions.entries()]
      .filter(([username]) => bodyMentions.has(username))
      .map(([, candidate]) => candidate.user_id);

    setSubmitting(true);
    setError(null);
    setMentionToken(null);
    setMentionCandidates([]);

    try {
      await onSubmit(normalizedBody, mentionedUserIds);
      setBody("");
      setSelectedMentions(new Map());
      onSubmitted?.();
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setSubmitting(false);
    }
  }

  const showMentionMenu = mentionToken !== null;

  return (
    <form style={styles.commentaryComposer} onSubmit={(event) => void submit(event)}>
      <label style={styles.commentaryComposerLabel}>
        <span>{label}</span>
        <div style={styles.commentaryTextareaWrap}>
          <textarea
            ref={textareaRef}
            name="comment_body"
            value={body}
            rows={initialBody ? 4 : 3}
            maxLength={20_000}
            inputMode="text"
            autoComplete="off"
            data-1p-ignore="true"
            placeholder="Discuss this object. Mention someone with @username."
            style={styles.commentaryTextarea}
            disabled={submitting}
            role="combobox"
            aria-autocomplete="list"
            aria-haspopup="listbox"
            aria-controls={showMentionMenu ? mentionListId : undefined}
            aria-expanded={showMentionMenu}
            aria-activedescendant={
              showMentionMenu && mentionCandidates[highlightedCandidateIndex]
                ? `${mentionListId}-${mentionCandidates[highlightedCandidateIndex].user_id}`
                : undefined
            }
            onChange={(event) => {
              const nextBody = event.target.value;
              setBody(nextBody);
              updateMentionAtCaret(nextBody, event.target.selectionStart);
            }}
            onClick={(event) => updateMentionAtCaret(body, event.currentTarget.selectionStart)}
            onBlur={() => {
              setMentionToken(null);
              setMentionCandidates([]);
            }}
            onKeyUp={(event) => {
              if (!["ArrowDown", "ArrowUp", "Enter", "Tab", "Escape"].includes(event.key)) {
                updateMentionAtCaret(body, event.currentTarget.selectionStart);
              }
            }}
            onKeyDown={handleComposerKeyDown}
          />

          {showMentionMenu ? (
            <div
              id={mentionListId}
              role="listbox"
              aria-label="Mention a user"
              style={styles.commentaryMentionMenu}
            >
              {mentionCandidatesLoading ? (
                <span style={styles.commentaryMentionMenuStatus}>Finding people…</span>
              ) : mentionCandidatesError ? (
                <span style={styles.inlineError}>{mentionCandidatesError}</span>
              ) : mentionCandidates.length === 0 ? (
                <span style={styles.commentaryMentionMenuStatus}>No matching people.</span>
              ) : (
                mentionCandidates.map((candidate, index) => (
                  <button
                    key={candidate.user_id}
                    id={`${mentionListId}-${candidate.user_id}`}
                    type="button"
                    role="option"
                    tabIndex={-1}
                    aria-selected={index === highlightedCandidateIndex}
                    style={
                      index === highlightedCandidateIndex
                        ? styles.commentaryMentionOptionActive
                        : styles.commentaryMentionOption
                    }
                    onPointerMove={() => setHighlightedCandidateIndex(index)}
                    onPointerDown={(event) => event.preventDefault()}
                    onClick={() => chooseMention(candidate)}
                  >
                    <strong>@{candidate.username}</strong>
                    <span style={styles.commentaryMeta}>{candidate.display_name}</span>
                  </button>
                ))
              )}
            </div>
          ) : null}
        </div>
      </label>

      <div style={styles.commentaryComposerFooter}>
        <span style={styles.commentaryHint}>
          Mentions are limited to people who can currently access this object.
        </span>
        <div style={styles.commentaryActions}>
          {onCancel ? (
            <button
              type="button"
              style={styles.tertiaryButton}
              disabled={submitting}
              onClick={onCancel}
            >
              Cancel
            </button>
          ) : null}
          <button
            type="submit"
            style={styles.secondaryButton}
            disabled={!body.trim() || submitting}
          >
            {submitting ? "Saving…" : submitLabel}
          </button>
        </div>
      </div>

      {error ? <p style={styles.inlineError}>{error}</p> : null}
    </form>
  );
}

function CommentItem({
  comment,
  targeted,
  currentUserId,
  canComment,
  canAdmin,
  threadResolved,
  onCommentChanged,
  onChanged,
}: {
  comment: Comment;
  targeted: boolean;
  currentUserId: string;
  canComment: boolean;
  canAdmin: boolean;
  threadResolved: boolean;
  onCommentChanged: (comment: Comment) => void;
  onChanged: () => Promise<void>;
}) {
  const [editing, setEditing] = useState(false);
  const [deleteConfirmOpen, setDeleteConfirmOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const isAuthor = comment.author.id === currentUserId;
  const isActive = comment.status === "active";
  const canEdit = canComment && !threadResolved && isAuthor && isActive;
  const canDelete = canComment && (isAuthor || canAdmin) && isActive;

  async function remove() {
    if (!canDelete || deleting) {
      return;
    }

    setDeleting(true);
    setError(null);

    try {
      const deleted = await deleteComment(comment.workspace_id, comment.object_id, comment.id);
      onCommentChanged(deleted);
      await onChanged();
      setDeleteConfirmOpen(false);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setDeleting(false);
    }
  }

  useEffect(() => {
    if (!deleteConfirmOpen || deleting) {
      return;
    }

    function handleKeyDown(event: globalThis.KeyboardEvent) {
      if (event.key === "Escape") {
        setDeleteConfirmOpen(false);
        setError(null);
      }
    }

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [deleteConfirmOpen, deleting]);

  return (
    <article
      id={`commentary-comment-${comment.id}`}
      tabIndex={targeted ? -1 : undefined}
      style={
        targeted
          ? {
              ...(comment.parent_comment_id
                ? styles.commentaryReply
                : styles.commentaryRootComment),
              ...styles.commentaryTargetComment,
            }
          : comment.parent_comment_id
            ? styles.commentaryReply
            : styles.commentaryRootComment
      }
    >
      <div style={styles.commentaryCommentHeader}>
        <div style={styles.commentaryAuthorBlock}>
          <strong>{comment.author.display_name}</strong>
          <span style={styles.commentaryMeta}>@{comment.author.username}</span>
        </div>
        <span style={styles.commentaryMeta}>
          {formatCommentDate(comment.created_at)}
          {comment.edited_at ? " · edited" : ""}
        </span>
      </div>

      {editing ? (
        <Composer
          workspaceId={comment.workspace_id}
          objectId={comment.object_id}
          initialBody={comment.body ?? ""}
          initialMentions={comment.mentions}
          label="Edit comment"
          submitLabel="Save"
          onCancel={() => setEditing(false)}
          onSubmitted={() => setEditing(false)}
          onSubmit={async (body, mentionedUserIds) => {
            const updated = await updateComment(
              comment.workspace_id,
              comment.object_id,
              comment.id,
              {
                body,
                mentioned_user_ids: mentionedUserIds,
              },
            );
            onCommentChanged(updated);
            await onChanged();
          }}
        />
      ) : comment.status === "deleted" ? (
        <p style={styles.commentaryTombstone}>Comment deleted.</p>
      ) : comment.status === "expired" ? (
        <p style={styles.commentaryTombstone}>Comment removed by retention.</p>
      ) : (
        <p style={styles.commentaryBody}>{comment.body}</p>
      )}

      {isActive && comment.mentions.length > 0 ? (
        <div style={styles.commentaryMentions}>
          {comment.mentions.map((mention) => (
            <span key={mention.user_id} style={styles.commentaryMention}>
              @{mention.username}
            </span>
          ))}
        </div>
      ) : null}

      {!editing && (canEdit || canDelete) ? (
        <div style={styles.commentaryActions}>
          {canEdit ? (
            <button type="button" style={styles.tertiaryButton} onClick={() => setEditing(true)}>
              Edit
            </button>
          ) : null}
          {canDelete ? (
            <button
              type="button"
              style={styles.tertiaryDangerButton}
              onClick={() => {
                setDeleteConfirmOpen(true);
                setError(null);
              }}
            >
              Delete
            </button>
          ) : null}
        </div>
      ) : null}
      {deleteConfirmOpen && canDelete ? (
        <div style={styles.modalBackdrop}>
          <button
            type="button"
            style={styles.modalBackdropDismiss}
            aria-label="Close delete comment confirmation"
            disabled={deleting}
            onClick={() => {
              setDeleteConfirmOpen(false);
              setError(null);
            }}
          />
          <div
            role="dialog"
            aria-modal="true"
            aria-labelledby={`delete-comment-dialog-title-${comment.id}`}
            aria-describedby={`delete-comment-dialog-description-${comment.id}`}
            style={styles.modalDialog}
          >
            <div style={styles.modalCopy}>
              <h2 id={`delete-comment-dialog-title-${comment.id}`} style={styles.modalTitle}>
                Delete this comment?
              </h2>
              <p id={`delete-comment-dialog-description-${comment.id}`} style={styles.muted}>
                The comment will be replaced with a deletion marker. This cannot be undone.
              </p>
            </div>

            {error ? (
              <div style={styles.errorBox} role="alert">
                <strong>Could not delete comment</strong>
                <span>{error}</span>
              </div>
            ) : null}

            <div style={styles.modalActions}>
              <button
                type="button"
                style={styles.secondaryButton}
                disabled={deleting}
                onClick={() => {
                  setDeleteConfirmOpen(false);
                  setError(null);
                }}
              >
                Cancel
              </button>
              <button
                type="button"
                style={styles.dangerButton}
                disabled={deleting}
                onClick={() => void remove()}
              >
                {deleting ? "Deleting…" : "Delete"}
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </article>
  );
}

function ThreadItem({
  thread,
  targetCommentId,
  highlightedTargetCommentId,
  currentUserId,
  canComment,
  canAdmin,
  loadingMoreComments,
  onLoadMoreComments,
  onThreadChanged,
  onCommentChanged,
  onChanged,
}: {
  thread: CommentThread;
  targetCommentId?: string | null;
  highlightedTargetCommentId?: string | null;
  currentUserId: string;
  canComment: boolean;
  canAdmin: boolean;
  loadingMoreComments: boolean;
  onLoadMoreComments: () => Promise<void>;
  onThreadChanged: (thread: CommentThread) => void;
  onCommentChanged: (comment: Comment) => void;
  onChanged: () => Promise<void>;
}) {
  const [replying, setReplying] = useState(false);
  const [transitioning, setTransitioning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [resolvedExpanded, setResolvedExpanded] = useState(() =>
    thread.comments.some((comment) => comment.id === targetCommentId),
  );
  const isResolved = thread.resolved_at !== null;
  const canTransition = canComment && (thread.created_by === currentUserId || canAdmin);
  const firstComment = thread.comments[0];
  const resolvedSummary =
    firstComment?.body?.replaceAll(/\s+/g, " ").trim() ??
    (firstComment?.status === "deleted"
      ? "Comment deleted"
      : firstComment?.status === "expired"
        ? "Comment removed by retention"
        : "Resolved discussion");

  async function transition() {
    if (!canTransition || transitioning) {
      return;
    }

    setTransitioning(true);
    setError(null);

    try {
      const updatedThread = isResolved
        ? await reopenCommentThread(thread.workspace_id, thread.object_id, thread.id)
        : await resolveCommentThread(thread.workspace_id, thread.object_id, thread.id);
      onThreadChanged(updatedThread);
      if (!isResolved) {
        setReplying(false);
        setResolvedExpanded(false);
      }
      await onChanged();
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setTransitioning(false);
    }
  }

  if (isResolved && !resolvedExpanded) {
    return (
      <section style={styles.commentaryThreadCollapsed}>
        <button
          type="button"
          style={styles.commentaryThreadSummary}
          aria-expanded="false"
          onClick={() => setResolvedExpanded(true)}
        >
          <span style={styles.commentaryResolvedBadge}>Resolved</span>
          <span style={styles.commentaryThreadSummaryText}>
            {firstComment
              ? `${firstComment.author.display_name}: ${resolvedSummary}`
              : resolvedSummary}
          </span>
        </button>
        {canTransition ? (
          <button
            type="button"
            style={styles.tertiaryButton}
            disabled={transitioning}
            onClick={() => void transition()}
          >
            {transitioning ? "Saving…" : "Reopen"}
          </button>
        ) : null}
      </section>
    );
  }

  return (
    <section style={styles.commentaryThread}>
      <div style={styles.commentaryThreadHeader}>
        <span style={isResolved ? styles.commentaryResolvedBadge : styles.commentaryOpenBadge}>
          {isResolved ? "Resolved" : "Open"}
        </span>
        <div style={styles.commentaryActions}>
          {isResolved ? (
            <button
              type="button"
              style={styles.tertiaryButton}
              onClick={() => setResolvedExpanded(false)}
            >
              Collapse
            </button>
          ) : null}
          {!isResolved && canComment ? (
            <button
              type="button"
              style={styles.tertiaryButton}
              onClick={() => setReplying((value) => !value)}
            >
              Reply
            </button>
          ) : null}
          {canTransition ? (
            <button
              type="button"
              style={styles.tertiaryButton}
              disabled={transitioning}
              onClick={() => void transition()}
            >
              {transitioning ? "Saving…" : isResolved ? "Reopen" : "Resolve"}
            </button>
          ) : null}
        </div>
      </div>

      <div style={styles.commentaryComments}>
        {thread.comments.map((comment) => (
          <CommentItem
            key={comment.id}
            comment={comment}
            targeted={comment.id === highlightedTargetCommentId}
            currentUserId={currentUserId}
            canComment={canComment}
            canAdmin={canAdmin}
            threadResolved={isResolved}
            onCommentChanged={onCommentChanged}
            onChanged={onChanged}
          />
        ))}
      </div>

      {thread.comments_next_cursor ? (
        <button
          type="button"
          style={styles.tertiaryButton}
          disabled={loadingMoreComments}
          onClick={() => void onLoadMoreComments()}
        >
          {loadingMoreComments ? "Loading replies…" : "Load more replies"}
        </button>
      ) : null}

      {replying ? (
        <Composer
          workspaceId={thread.workspace_id}
          objectId={thread.object_id}
          label="Reply"
          submitLabel="Reply"
          onCancel={() => setReplying(false)}
          onSubmitted={() => setReplying(false)}
          onSubmit={async (body, mentionedUserIds) => {
            const reply = await replyToCommentThread(
              thread.workspace_id,
              thread.object_id,
              thread.id,
              {
                body,
                mentioned_user_ids: mentionedUserIds,
              },
            );
            onCommentChanged(reply);
            await onChanged();
          }}
        />
      ) : null}

      {error ? <p style={styles.inlineError}>{error}</p> : null}
    </section>
  );
}

export function CommentaryPanel({
  workspaceId,
  objectId,
  currentUserId,
  effectiveRole,
  archived,
  targetCommentId,
  targetThreadId,
}: Props) {
  const [threads, setThreads] = useState<CommentThread[]>([]);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [loadingCommentThreadIds, setLoadingCommentThreadIds] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [dismissedTargetId, setDismissedTargetId] = useState<string | null>(null);
  const reloadGenerationRef = useRef(0);
  const scrolledTargetRef = useRef<string | null>(null);
  const canComment = !archived;
  const canAdmin = effectiveRole === "admin";
  const highlightedTargetId = dismissedTargetId === targetCommentId ? null : targetCommentId;

  const reload = useCallback(
    async (signal?: AbortSignal) => {
      const generation = ++reloadGenerationRef.current;
      const response = await listObjectCommentary(workspaceId, objectId, null, signal);
      if (signal?.aborted || generation !== reloadGenerationRef.current) {
        return;
      }
      const loadedThreads = [...response.items];
      let commentaryCursor = response.next_cursor ?? null;

      while (
        targetThreadId &&
        !loadedThreads.some((thread) => thread.id === targetThreadId) &&
        commentaryCursor
      ) {
        const page = await listObjectCommentary(workspaceId, objectId, commentaryCursor, signal);
        loadedThreads.push(...page.items);
        commentaryCursor = page.next_cursor ?? null;
      }

      const targetThread = targetThreadId
        ? loadedThreads.find((thread) => thread.id === targetThreadId)
        : null;

      if (
        targetThread &&
        targetCommentId &&
        !targetThread.comments.some((comment) => comment.id === targetCommentId)
      ) {
        let commentsCursor = targetThread.comments_next_cursor;

        while (
          !targetThread.comments.some((comment) => comment.id === targetCommentId) &&
          commentsCursor
        ) {
          const page = await listCommentThreadComments(
            workspaceId,
            objectId,
            targetThread.id,
            commentsCursor,
            signal,
          );
          const known = new Set(targetThread.comments.map((comment) => comment.id));
          targetThread.comments.push(...page.items.filter((comment) => !known.has(comment.id)));
          targetThread.comments.sort(compareComments);
          commentsCursor = page.next_cursor;
          targetThread.comments_next_cursor = commentsCursor;
        }
      }

      if (signal?.aborted || generation !== reloadGenerationRef.current) {
        return;
      }
      setThreads(loadedThreads);
      setNextCursor(commentaryCursor);
    },
    [workspaceId, objectId, targetCommentId, targetThreadId],
  );

  const refreshAfterMutation = useCallback(async () => {
    setError(null);
    try {
      await reload();
    } catch (cause) {
      setError(
        `Commentary changed, but the latest state could not be refreshed: ${errorMessage(cause)}`,
      );
    }
  }, [reload]);

  const applyThread = useCallback((thread: CommentThread) => {
    setThreads((current) => [thread, ...current.filter((candidate) => candidate.id !== thread.id)]);
  }, []);

  const applyComment = useCallback((comment: Comment) => {
    setThreads((current) => {
      const updated = current.map((thread) => {
        if (thread.id !== comment.thread_id) {
          return thread;
        }

        const comments = thread.comments.some((candidate) => candidate.id === comment.id)
          ? thread.comments.map((candidate) => (candidate.id === comment.id ? comment : candidate))
          : [...thread.comments, comment];
        comments.sort(compareComments);
        return { ...thread, comments, updated_at: comment.updated_at };
      });
      const changed = updated.find((thread) => thread.id === comment.thread_id);
      return changed
        ? [changed, ...updated.filter((thread) => thread.id !== comment.thread_id)]
        : updated;
    });
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    setLoading(true);
    setError(null);

    void reload(controller.signal)
      .catch((cause: unknown) => {
        if (!isAbortError(cause)) {
          setError(errorMessage(cause));
        }
      })
      .finally(() => {
        if (!controller.signal.aborted) {
          setLoading(false);
        }
      });

    return () => controller.abort();
  }, [reload]);

  useEffect(() => {
    if (loading || !targetCommentId || scrolledTargetRef.current === targetCommentId) {
      return;
    }

    const target = document.getElementById(`commentary-comment-${targetCommentId}`);
    if (!target) {
      return;
    }

    scrolledTargetRef.current = targetCommentId;
    target.scrollIntoView({ block: "center" });
    target.focus({ preventScroll: true });
  }, [loading, targetCommentId]);

  useEffect(() => {
    if (!targetCommentId || dismissedTargetId === targetCommentId) {
      return;
    }

    const dismissOnOutsidePointer = (event: PointerEvent) => {
      const target = document.getElementById(`commentary-comment-${targetCommentId}`);
      if (target && event.target instanceof Node && !target.contains(event.target)) {
        setDismissedTargetId(targetCommentId);
      }
    };

    document.addEventListener("pointerdown", dismissOnOutsidePointer);
    return () => document.removeEventListener("pointerdown", dismissOnOutsidePointer);
  }, [dismissedTargetId, targetCommentId]);

  useEffect(() => {
    let refreshController: AbortController | null = null;

    const handleRealtime = (event: Event) => {
      const message = (event as CustomEvent<RealtimeMessage>).detail;
      const commentaryChanged =
        message.workspace_id === workspaceId &&
        message.object_id === objectId &&
        (message.type.startsWith("comment.") || message.type.startsWith("comment_thread."));

      if (message.type !== "realtime.resync_required" && !commentaryChanged) {
        return;
      }

      refreshController?.abort();
      refreshController = new AbortController();
      void reload(refreshController.signal).catch((cause: unknown) => {
        if (!isAbortError(cause)) {
          setError(`Realtime commentary refresh failed: ${errorMessage(cause)}`);
        }
      });
    };

    window.addEventListener(KIVAL_REALTIME_EVENT, handleRealtime);
    return () => {
      window.removeEventListener(KIVAL_REALTIME_EVENT, handleRealtime);
      refreshController?.abort();
    };
  }, [objectId, reload, workspaceId]);

  async function loadMore() {
    if (!nextCursor || loadingMore) {
      return;
    }

    setLoadingMore(true);
    setError(null);

    try {
      const response = await listObjectCommentary(workspaceId, objectId, nextCursor);
      setThreads((current) => {
        const known = new Set(current.map((thread) => thread.id));
        return [...current, ...response.items.filter((thread) => !known.has(thread.id))];
      });
      setNextCursor(response.next_cursor ?? null);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setLoadingMore(false);
    }
  }

  async function loadMoreThreadComments(thread: CommentThread) {
    if (!thread.comments_next_cursor || loadingCommentThreadIds.has(thread.id)) {
      return;
    }

    setLoadingCommentThreadIds((current) => new Set(current).add(thread.id));
    setError(null);

    try {
      const response = await listCommentThreadComments(
        workspaceId,
        objectId,
        thread.id,
        thread.comments_next_cursor,
      );
      setThreads((current) =>
        current.map((candidate) => {
          if (candidate.id !== thread.id) {
            return candidate;
          }
          const known = new Set(candidate.comments.map((comment) => comment.id));
          const comments = [
            ...candidate.comments,
            ...response.items.filter((comment) => !known.has(comment.id)),
          ];
          comments.sort(compareComments);
          return {
            ...candidate,
            comments,
            comments_next_cursor: response.next_cursor,
          };
        }),
      );
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setLoadingCommentThreadIds((current) => {
        const next = new Set(current);
        next.delete(thread.id);
        return next;
      });
    }
  }

  return (
    <section style={styles.commentaryPanel} aria-labelledby="object-commentary-heading">
      <div style={styles.commentaryHeadingRow}>
        <div>
          <h2 id="object-commentary-heading" style={styles.sectionTitle}>
            Commentary
          </h2>
          <p style={styles.commentaryDescription}>
            Working discussion around this object, separate from its durable version history.
          </p>
        </div>
        <span style={styles.commentaryCount}>
          {threads.length}
          {nextCursor ? "+" : ""} {threads.length === 1 && !nextCursor ? "thread" : "threads"}
        </span>
      </div>

      {canComment ? (
        <Composer
          workspaceId={workspaceId}
          objectId={objectId}
          label="Start a discussion"
          submitLabel="Comment"
          onSubmit={async (body, mentionedUserIds) => {
            const thread = await createCommentThread(workspaceId, objectId, {
              body,
              mentioned_user_ids: mentionedUserIds,
            });
            applyThread(thread);
            await refreshAfterMutation();
          }}
        />
      ) : archived ? (
        <p style={styles.commentaryHint}>
          Archived objects keep their commentary inspectable but do not accept new discussion.
        </p>
      ) : null}

      {loading ? <LoadingIndicator label="Loading commentary…" compact /> : null}
      {error ? <p style={styles.inlineError}>{error}</p> : null}
      {!loading && !error && threads.length === 0 ? (
        <p style={styles.muted}>No commentary yet.</p>
      ) : null}

      <div style={styles.commentaryThreadList}>
        {threads.map((thread) => (
          <ThreadItem
            key={thread.id}
            thread={thread}
            targetCommentId={targetCommentId}
            highlightedTargetCommentId={highlightedTargetId}
            currentUserId={currentUserId}
            canComment={canComment}
            canAdmin={canAdmin}
            loadingMoreComments={loadingCommentThreadIds.has(thread.id)}
            onLoadMoreComments={() => loadMoreThreadComments(thread)}
            onThreadChanged={applyThread}
            onCommentChanged={applyComment}
            onChanged={refreshAfterMutation}
          />
        ))}
      </div>

      {nextCursor ? (
        <button
          type="button"
          style={styles.tertiaryButton}
          disabled={loadingMore}
          onClick={() => void loadMore()}
        >
          {loadingMore ? "Loading…" : "Load more commentary"}
        </button>
      ) : null}
    </section>
  );
}
