-- =====================================================================
-- Kival migration 0008: object commentary
-- =====================================================================
-- Purpose
--   Add mutable discussion threads, comments, mentions, commentary audit subjects,
--   and bounded retention for working context attached to durable objects.
--
-- Depends on
--   * 0000_setup.sql for shared update helpers.
--   * 0001_identity.sql for comment authors, resolvers, deleters, and mentions.
--   * 0004_objects.sql for workspace-scoped object ownership.
--   * 0006_events.sql for the append-only event log extended by this migration.
--
-- Owns
--   * `kival.comment_threads`
--   * `kival.comments`
--   * `kival.comment_mentions`
--   * Commentary subject columns and indexes on `kival.events`.
--   * Commentary-aware event validation and retention processing.
--
-- Design notes
--   Commentary is mutable working context around a durable object. It is kept
--   separate from object versions, graph edges, textual references, and search.
--   Retention may tombstone comment bodies and eventually purge whole threads,
--   while immutable event rows retain stable commentary identifiers without
--   foreign keys to mutable commentary state.
-- =====================================================================

-- =====================================================================
-- Comment threads
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.comment_threads (
    id uuid PRIMARY KEY DEFAULT uuidv7(),

    workspace_id uuid NOT NULL,
    object_id uuid NOT NULL,
    created_by uuid NOT NULL REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    resolved_at timestamptz,
    resolved_by uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    -- A retention policy may assign or shorten this boundary independently of
    -- the parent object's lifetime.
    retention_expires_at timestamptz,

    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),

    CONSTRAINT comment_threads_object_fk
        FOREIGN KEY (workspace_id, object_id)
        REFERENCES kival.objects(workspace_id, id)
        ON UPDATE RESTRICT
        ON DELETE CASCADE,

    CONSTRAINT comment_threads_scoped_id_unique
        UNIQUE (workspace_id, object_id, id),

    CONSTRAINT comment_threads_resolution_complete
        CHECK (
            (resolved_at IS NULL AND resolved_by IS NULL)
            OR (resolved_at IS NOT NULL AND resolved_by IS NOT NULL)
        ),

    CONSTRAINT comment_threads_resolution_after_creation
        CHECK (resolved_at IS NULL OR resolved_at >= created_at),

    CONSTRAINT comment_threads_retention_after_creation
        CHECK (retention_expires_at IS NULL OR retention_expires_at >= created_at),

    CONSTRAINT comment_threads_updated_after_creation
        CHECK (updated_at >= created_at)
);

CREATE INDEX IF NOT EXISTS comment_threads_object_activity_idx
    ON kival.comment_threads (workspace_id, object_id, updated_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS comment_threads_object_open_idx
    ON kival.comment_threads (workspace_id, object_id, updated_at DESC, id DESC)
    WHERE resolved_at IS NULL;

CREATE INDEX IF NOT EXISTS comment_threads_retention_due_idx
    ON kival.comment_threads (retention_expires_at, id)
    WHERE retention_expires_at IS NOT NULL;

DROP TRIGGER IF EXISTS comment_threads_before_update ON kival.comment_threads;

CREATE TRIGGER comment_threads_before_update
BEFORE UPDATE ON kival.comment_threads
FOR EACH ROW
EXECUTE FUNCTION kival.before_update();

-- =====================================================================
-- Comments
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.comments (
    id uuid PRIMARY KEY DEFAULT uuidv7(),

    workspace_id uuid NOT NULL,
    object_id uuid NOT NULL,
    thread_id uuid NOT NULL,
    parent_comment_id uuid,

    author_user_id uuid NOT NULL REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    body text,
    edited_at timestamptz,

    deleted_at timestamptz,
    deleted_by uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    -- Retention can remove the body without presenting the action as a user
    -- deletion. Replies remain structurally valid until the whole thread is
    -- eventually purged.
    expired_at timestamptz,
    retention_expires_at timestamptz,

    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),

    CONSTRAINT comments_thread_fk
        FOREIGN KEY (workspace_id, object_id, thread_id)
        REFERENCES kival.comment_threads(workspace_id, object_id, id)
        ON UPDATE RESTRICT
        ON DELETE CASCADE,

    CONSTRAINT comments_scoped_id_unique
        UNIQUE (workspace_id, object_id, thread_id, id),

    CONSTRAINT comments_object_id_unique
        UNIQUE (workspace_id, object_id, id),

    CONSTRAINT comments_parent_fk
        FOREIGN KEY (workspace_id, object_id, thread_id, parent_comment_id)
        REFERENCES kival.comments(workspace_id, object_id, thread_id, id)
        ON UPDATE RESTRICT
        ON DELETE CASCADE,

    CONSTRAINT comments_parent_not_self
        CHECK (parent_comment_id IS NULL OR parent_comment_id <> id),

    CONSTRAINT comments_body_size
        CHECK (body IS NULL OR char_length(body) <= 20000),

    CONSTRAINT comments_state_valid
        CHECK (
            (
                body IS NOT NULL
                AND length(btrim(body)) > 0
                AND deleted_at IS NULL
                AND deleted_by IS NULL
                AND expired_at IS NULL
            )
            OR (
                body IS NULL
                AND deleted_at IS NOT NULL
                AND deleted_by IS NOT NULL
                AND expired_at IS NULL
            )
            OR (
                body IS NULL
                AND deleted_at IS NULL
                AND deleted_by IS NULL
                AND expired_at IS NOT NULL
            )
        ),

    CONSTRAINT comments_edited_after_creation
        CHECK (edited_at IS NULL OR edited_at >= created_at),

    CONSTRAINT comments_deleted_after_creation
        CHECK (deleted_at IS NULL OR deleted_at >= created_at),

    CONSTRAINT comments_expired_after_creation
        CHECK (expired_at IS NULL OR expired_at >= created_at),

    CONSTRAINT comments_retention_after_creation
        CHECK (retention_expires_at IS NULL OR retention_expires_at >= created_at),

    CONSTRAINT comments_updated_after_creation
        CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS comments_one_root_per_thread
    ON kival.comments (thread_id)
    WHERE parent_comment_id IS NULL;

CREATE INDEX IF NOT EXISTS comments_thread_created_idx
    ON kival.comments (workspace_id, object_id, thread_id, created_at, id);

CREATE INDEX IF NOT EXISTS comments_parent_created_idx
    ON kival.comments (parent_comment_id, created_at, id)
    WHERE parent_comment_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS comments_author_idx
    ON kival.comments (author_user_id, created_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS comments_retention_due_idx
    ON kival.comments (retention_expires_at, id)
    WHERE retention_expires_at IS NOT NULL
      AND expired_at IS NULL
      AND deleted_at IS NULL;

DROP TRIGGER IF EXISTS comments_before_update ON kival.comments;

CREATE TRIGGER comments_before_update
BEFORE UPDATE ON kival.comments
FOR EACH ROW
EXECUTE FUNCTION kival.before_update();

-- =====================================================================
-- Mentions
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.comment_mentions (
    workspace_id uuid NOT NULL,
    object_id uuid NOT NULL,
    comment_id uuid NOT NULL,
    mentioned_user_id uuid NOT NULL REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),

    PRIMARY KEY (comment_id, mentioned_user_id),

    CONSTRAINT comment_mentions_comment_fk
        FOREIGN KEY (workspace_id, object_id, comment_id)
        REFERENCES kival.comments(workspace_id, object_id, id)
        ON UPDATE RESTRICT
        ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS comment_mentions_target_idx
    ON kival.comment_mentions (mentioned_user_id, created_at DESC, comment_id);

-- =====================================================================
-- Commentary event attribution
-- =====================================================================

-- Commentary event subjects intentionally do not have foreign keys. Retention may
-- purge mutable working context while immutable audit events retain stable IDs.
-- The insert trigger validates their workspace ownership while the rows exist.
ALTER TABLE kival.events
    ADD COLUMN IF NOT EXISTS comment_thread_id uuid,
    ADD COLUMN IF NOT EXISTS comment_id uuid;

CREATE INDEX IF NOT EXISTS events_comment_thread_idx
    ON kival.events (comment_thread_id, sequence_number)
    WHERE comment_thread_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS events_comment_idx
    ON kival.events (comment_id, sequence_number)
    WHERE comment_id IS NOT NULL;

-- ---------------------------------------------------------------------
-- Function: kival.events_before_insert()
-- Purpose
--   Extend canonical event-scope validation to commentary event subjects.
-- Trigger contract
--   BEFORE INSERT on `kival.events`; replaces the function introduced by
--   0006_events.sql after commentary subject columns are added.
-- Behavior
--   Retains validation for object, edge, grant, and version subjects and adds
--   workspace/object/thread consistency checks for comment threads and comments.
--   Commentary rows need not remain present after insertion because retention may
--   purge them; validation is performed while the referenced working context exists.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.events_before_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    v_workspace_id uuid;
    v_object_id uuid;
    v_thread_id uuid;
BEGIN
    IF NEW.object_id IS NOT NULL THEN
        SELECT o.workspace_id INTO v_workspace_id
        FROM kival.objects o WHERE o.id = NEW.object_id;
        IF NEW.workspace_id IS NULL THEN
            RAISE EXCEPTION 'workspace_id is required for object event';
        END IF;
        IF v_workspace_id IS DISTINCT FROM NEW.workspace_id THEN
            RAISE EXCEPTION 'event workspace_id does not match object workspace';
        END IF;
    END IF;

    IF NEW.object_edge_id IS NOT NULL THEN
        SELECT oe.workspace_id INTO v_workspace_id
        FROM kival.object_edges oe WHERE oe.id = NEW.object_edge_id;
        IF NEW.workspace_id IS NULL THEN
            RAISE EXCEPTION 'workspace_id is required for object_edge event';
        END IF;
        IF v_workspace_id IS DISTINCT FROM NEW.workspace_id THEN
            RAISE EXCEPTION 'event workspace_id does not match object_edge workspace';
        END IF;
    END IF;

    IF NEW.object_grant_id IS NOT NULL THEN
        SELECT og.workspace_id INTO v_workspace_id
        FROM kival.object_grants og WHERE og.id = NEW.object_grant_id;
        IF NEW.workspace_id IS NULL THEN
            RAISE EXCEPTION 'workspace_id is required for object_grant event';
        END IF;
        IF v_workspace_id IS DISTINCT FROM NEW.workspace_id THEN
            RAISE EXCEPTION 'event workspace_id does not match object_grant workspace';
        END IF;
    END IF;

    IF NEW.object_version_id IS NOT NULL THEN
        SELECT o.workspace_id INTO v_workspace_id
        FROM kival.object_versions ov
        JOIN kival.objects o ON o.id = ov.object_id
        WHERE ov.id = NEW.object_version_id;
        IF NEW.workspace_id IS NULL THEN
            RAISE EXCEPTION 'workspace_id is required for object_version event';
        END IF;
        IF v_workspace_id IS DISTINCT FROM NEW.workspace_id THEN
            RAISE EXCEPTION 'event workspace_id does not match object_version workspace';
        END IF;
    END IF;

    IF NEW.comment_thread_id IS NOT NULL THEN
        SELECT t.workspace_id, t.object_id
        INTO v_workspace_id, v_object_id
        FROM kival.comment_threads t
        WHERE t.id = NEW.comment_thread_id;

        IF NEW.workspace_id IS NULL OR NEW.object_id IS NULL THEN
            RAISE EXCEPTION 'workspace_id and object_id are required for comment_thread event';
        END IF;

        IF v_workspace_id IS DISTINCT FROM NEW.workspace_id
           OR v_object_id IS DISTINCT FROM NEW.object_id THEN
            RAISE EXCEPTION 'event subject does not match comment_thread scope';
        END IF;
    END IF;

    IF NEW.comment_id IS NOT NULL THEN
        SELECT c.workspace_id, c.object_id, c.thread_id
        INTO v_workspace_id, v_object_id, v_thread_id
        FROM kival.comments c
        WHERE c.id = NEW.comment_id;

        IF NEW.workspace_id IS NULL OR NEW.object_id IS NULL THEN
            RAISE EXCEPTION 'workspace_id and object_id are required for comment event';
        END IF;

        IF NEW.comment_thread_id IS NULL THEN
            RAISE EXCEPTION 'comment_thread_id is required for comment event';
        END IF;

        IF v_workspace_id IS DISTINCT FROM NEW.workspace_id
           OR v_object_id IS DISTINCT FROM NEW.object_id THEN
            RAISE EXCEPTION 'event subject does not match comment scope';
        END IF;

        IF v_thread_id IS DISTINCT FROM NEW.comment_thread_id THEN
            RAISE EXCEPTION 'event comment does not belong to comment_thread';
        END IF;
    END IF;

    RETURN NEW;
END;
$$;

COMMENT ON FUNCTION kival.events_before_insert() IS
    'Validates workspace and object scope for durable and commentary event subjects.';

-- =====================================================================
-- Commentary retention
-- =====================================================================

-- ---------------------------------------------------------------------
-- Function: kival.apply_commentary_retention(batch_size)
-- Purpose
--   Apply commentary retention in bounded, scheduler-independent batches.
-- Parameters
--   p_batch_size  Maximum number of due comments/threads considered per cleanup
--                 phase; must be positive.
-- Returns
--   Counts of comments tombstoned by retention and threads purged as a unit.
-- Concurrency semantics
--   Due rows are claimed with `FOR UPDATE SKIP LOCKED`, so cleanup may run in
--   bounded concurrent batches. Due threads are purged as units; independently
--   due comments in surviving threads are tombstoned so reply structure remains.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.apply_commentary_retention(p_batch_size integer DEFAULT 500)
RETURNS TABLE(expired_comments integer, purged_threads integer)
LANGUAGE plpgsql
AS $$
DECLARE
    v_expired_comments integer := 0;
    v_purged_threads integer := 0;
BEGIN
    IF p_batch_size < 1 THEN
        RAISE EXCEPTION 'batch size must be positive';
    END IF;

    WITH due AS (
        SELECT id
        FROM kival.comment_threads
        WHERE retention_expires_at <= now()
        ORDER BY retention_expires_at, id
        LIMIT p_batch_size
        FOR UPDATE SKIP LOCKED
    ), purged AS (
        DELETE FROM kival.comment_threads t
        USING due
        WHERE t.id = due.id
        RETURNING t.id
    )
    SELECT count(*)::integer INTO v_purged_threads FROM purged;

    WITH due AS (
        SELECT id
        FROM kival.comments
        WHERE retention_expires_at <= now()
          AND expired_at IS NULL
          AND deleted_at IS NULL
        ORDER BY retention_expires_at, id
        LIMIT p_batch_size
        FOR UPDATE SKIP LOCKED
    ), removed_mentions AS (
        DELETE FROM kival.comment_mentions m
        USING due
        WHERE m.comment_id = due.id
    ), expired AS (
        UPDATE kival.comments c
        SET body = NULL,
            edited_at = NULL,
            expired_at = now()
        FROM due
        WHERE c.id = due.id
        RETURNING c.id
    )
    SELECT count(*)::integer INTO v_expired_comments FROM expired;

    RETURN QUERY SELECT v_expired_comments, v_purged_threads;
END;
$$;

COMMENT ON FUNCTION kival.apply_commentary_retention(integer) IS
    'Applies commentary retention in bounded batches without assuming indefinite storage or a scheduler.';

-- Commentary is deliberately absent from object_versions, object_edges,
-- object_references, and all search projections.
