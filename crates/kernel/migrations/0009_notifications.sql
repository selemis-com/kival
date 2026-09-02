-- =====================================================================
-- Kival migration 0009: notifications, inbox, and realtime invalidation
-- =====================================================================
-- Purpose
--   Add explicit per-object notification preferences, durable event-time
--   notification candidates, a personal inbox projection, and lightweight
--   realtime invalidation delivery.
--
-- Depends on
--   * 0000_setup.sql for shared update helpers.
--   * 0001_identity.sql for notification recipients and actors.
--   * 0002_workspaces.sql for workspace scope and membership.
--   * 0004_objects.sql for object-scoped notification state.
--   * 0005_access.sql for current-visibility predicates.
--   * 0006_events.sql and 0008_commentary.sql for durable event subjects.
--
-- Owns
--   * `kival.object_notification_preferences`
--   * `kival.notification_candidates`
--   * `kival.inbox_notifications`
--   * Event-time recipient capture and durable candidate projection.
--   * Transactional realtime invalidation publication.
--   * Bounded notification retention processing.
--
-- Design notes
--   Visibility remains the default eligibility rule for object updates, while
--   commentary activity is scoped to thread participants, mentioned users,
--   and explicit subscribers. Directly addressed activity such as a mention
--   or reply remains eligible. Inbox rows retain identifiers and routing
--   metadata rather than protected object or commentary content.
--
--   Event-time recipient eligibility is captured transactionally with the
--   durable event. A recoverable worker later claims pending candidates
--   directly and projects them idempotently into inbox state and ephemeral
--   realtime invalidations. Gaining access after an event therefore cannot
--   produce historical notifications.
-- =====================================================================

-- =====================================================================
-- Object notification preferences
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.object_notification_preferences (
    user_id uuid NOT NULL REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    workspace_id uuid NOT NULL REFERENCES kival.workspaces(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    object_id uuid NOT NULL,
    ordinary_notifications_enabled boolean NOT NULL,
    updated_by uuid NOT NULL REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),

    PRIMARY KEY (user_id, object_id),

    CONSTRAINT object_notification_preferences_object_fk
        FOREIGN KEY (workspace_id, object_id)
        REFERENCES kival.objects (workspace_id, id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    CONSTRAINT object_notification_preferences_updated_at_after_created_at
        CHECK (updated_at >= created_at)
);

CREATE INDEX IF NOT EXISTS object_notification_preferences_object_idx
    ON kival.object_notification_preferences (workspace_id, object_id, user_id);

-- ---------------------------------------------------------------------
-- Function: kival.before_update_object_notification_preference()
-- Purpose
--   Preserve notification-preference identity while allowing the preference value
--   and actor attribution to change.
-- Trigger contract
--   BEFORE UPDATE on `kival.object_notification_preferences`.
-- Behavior
--   Rejects changes to user, workspace, object, or creation timestamp and refreshes
--   `updated_at` for accepted preference updates.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.before_update_object_notification_preference()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.user_id IS DISTINCT FROM OLD.user_id THEN
        RAISE EXCEPTION 'notification preference user_id is immutable';
    END IF;

    IF NEW.workspace_id IS DISTINCT FROM OLD.workspace_id THEN
        RAISE EXCEPTION 'notification preference workspace_id is immutable';
    END IF;

    IF NEW.object_id IS DISTINCT FROM OLD.object_id THEN
        RAISE EXCEPTION 'notification preference object_id is immutable';
    END IF;

    IF NEW.created_at IS DISTINCT FROM OLD.created_at THEN
        RAISE EXCEPTION 'notification preference created_at is immutable';
    END IF;

    NEW.updated_at = now();

    RETURN NEW;
END;
$$;

COMMENT ON FUNCTION kival.before_update_object_notification_preference() IS
    'Notification preference BEFORE UPDATE trigger: preserves its composite identity and created_at and refreshes updated_at.';

DROP TRIGGER IF EXISTS object_notification_preferences_before_update
    ON kival.object_notification_preferences;
CREATE TRIGGER object_notification_preferences_before_update
BEFORE UPDATE ON kival.object_notification_preferences
FOR EACH ROW
EXECUTE FUNCTION kival.before_update_object_notification_preference();

-- =====================================================================
-- Durable notification candidates
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.notification_candidates (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    sequence_number bigint GENERATED ALWAYS AS IDENTITY UNIQUE,

    event_id uuid NOT NULL REFERENCES kival.events(id)
        ON UPDATE RESTRICT
        ON DELETE CASCADE,
    recipient_user_id uuid NOT NULL REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    workspace_id uuid NOT NULL REFERENCES kival.workspaces(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    object_id uuid,
    actor_user_id uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    delivery_kind text NOT NULL,
    notification_type text NOT NULL,
    reason text NOT NULL,
    deduplication_key text NOT NULL,

    thread_id uuid,
    comment_id uuid,

    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL DEFAULT now() + interval '30 days',
    projected_at timestamptz,

    CONSTRAINT notification_candidates_object_fk
        FOREIGN KEY (workspace_id, object_id)
        REFERENCES kival.objects (workspace_id, id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    CONSTRAINT notification_candidates_delivery_kind_valid
        CHECK (delivery_kind IN ('inbox', 'realtime')),
    CONSTRAINT notification_candidates_type_not_blank
        CHECK (length(btrim(notification_type)) > 0),
    CONSTRAINT notification_candidates_reason_not_blank
        CHECK (length(btrim(reason)) > 0),
    CONSTRAINT notification_candidates_workspace_subject_valid
        CHECK (object_id IS NOT NULL OR reason = 'workspace_access_granted'),
    CONSTRAINT notification_candidates_deduplication_key_not_blank
        CHECK (length(btrim(deduplication_key)) > 0),
    CONSTRAINT notification_candidates_expires_at_after_created_at
        CHECK (expires_at >= created_at),
    CONSTRAINT notification_candidates_projected_at_after_created_at
        CHECK (projected_at IS NULL OR projected_at >= created_at),

    UNIQUE (event_id, recipient_user_id, delivery_kind, reason)
);

CREATE INDEX IF NOT EXISTS notification_candidates_pending_idx
    ON kival.notification_candidates (sequence_number)
    WHERE projected_at IS NULL;

CREATE INDEX IF NOT EXISTS notification_candidates_expiry_idx
    ON kival.notification_candidates (expires_at);

-- =====================================================================
-- Personal inbox projection
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.inbox_notifications (
    id uuid PRIMARY KEY DEFAULT uuidv7(),
    -- The ordering key is the newest durable candidate folded into this row.
    -- Using the candidate sequence makes grouped projection independent of
    -- which worker happens to acquire conflicting candidates first.
    sequence_number bigint NOT NULL UNIQUE,
    source_candidate_sequence_number bigint NOT NULL,
    directed_candidate_sequence_number bigint,

    recipient_user_id uuid NOT NULL REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    workspace_id uuid NOT NULL REFERENCES kival.workspaces(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    object_id uuid,
    source_event_id uuid NOT NULL,
    latest_event_id uuid NOT NULL,
    actor_user_id uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    notification_type text NOT NULL,
    reason text NOT NULL,
    deduplication_key text NOT NULL,
    event_count integer NOT NULL DEFAULT 1,

    thread_id uuid,
    comment_id uuid,

    read_at timestamptz,
    archived_at timestamptz,
    expires_at timestamptz NOT NULL DEFAULT now() + interval '180 days',
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),

    CONSTRAINT inbox_notifications_object_fk
        FOREIGN KEY (workspace_id, object_id)
        REFERENCES kival.objects (workspace_id, id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    CONSTRAINT inbox_notifications_type_not_blank
        CHECK (length(btrim(notification_type)) > 0),
    CONSTRAINT inbox_notifications_reason_not_blank
        CHECK (length(btrim(reason)) > 0),
    CONSTRAINT inbox_notifications_workspace_subject_valid
        CHECK (object_id IS NOT NULL OR reason = 'workspace_access_granted'),
    CONSTRAINT inbox_notifications_deduplication_key_not_blank
        CHECK (length(btrim(deduplication_key)) > 0),
    CONSTRAINT inbox_notifications_event_count_positive
        CHECK (event_count > 0),
    CONSTRAINT inbox_notifications_candidate_sequences_positive
        CHECK (
            sequence_number > 0
            AND source_candidate_sequence_number > 0
            AND (directed_candidate_sequence_number IS NULL OR directed_candidate_sequence_number > 0)
        ),
    CONSTRAINT inbox_notifications_candidate_sequence_order_valid
        CHECK (
            source_candidate_sequence_number <= sequence_number
            AND (
                directed_candidate_sequence_number IS NULL
                OR directed_candidate_sequence_number BETWEEN source_candidate_sequence_number AND sequence_number
            )
        ),
    CONSTRAINT inbox_notifications_read_at_after_created_at
        CHECK (read_at IS NULL OR read_at >= created_at),
    CONSTRAINT inbox_notifications_archived_at_after_created_at
        CHECK (archived_at IS NULL OR archived_at >= created_at),
    CONSTRAINT inbox_notifications_expires_at_after_created_at
        CHECK (expires_at >= created_at),
    CONSTRAINT inbox_notifications_updated_at_after_created_at
        CHECK (updated_at >= created_at),

    UNIQUE (recipient_user_id, deduplication_key)
);

CREATE INDEX IF NOT EXISTS inbox_notifications_recipient_sequence_idx
    ON kival.inbox_notifications (recipient_user_id, sequence_number DESC)
    WHERE archived_at IS NULL;

CREATE INDEX IF NOT EXISTS inbox_notifications_recipient_unread_idx
    ON kival.inbox_notifications (recipient_user_id, sequence_number DESC)
    WHERE archived_at IS NULL AND read_at IS NULL;

CREATE INDEX IF NOT EXISTS inbox_notifications_object_idx
    ON kival.inbox_notifications (workspace_id, object_id, sequence_number DESC);

CREATE INDEX IF NOT EXISTS inbox_notifications_expiry_idx
    ON kival.inbox_notifications (expires_at);

-- ---------------------------------------------------------------------
-- Function: kival.inbox_notification_is_visible(recipient_user_id, workspace_id, object_id, reason)
-- Purpose
--   Re-evaluate current authorization before exposing a durable inbox entry.
-- Parameters
--   p_recipient_user_id  User whose inbox entry is being evaluated.
--   p_workspace_id       Workspace associated with the entry.
--   p_object_id          Object subject, or NULL for workspace-access grants.
--   p_reason             Notification reason used to validate subject shape.
-- Returns
--   TRUE only when the workspace is active and the recipient still has the
--   corresponding workspace/object visibility. Workspace grants intentionally have
--   no object subject; every other notification remains object-scoped.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.inbox_notification_is_visible(
    p_recipient_user_id uuid,
    p_workspace_id uuid,
    p_object_id uuid,
    p_reason text
)
RETURNS boolean
LANGUAGE sql
STABLE
AS $$
    SELECT EXISTS (
        SELECT 1
        FROM kival.workspaces workspace
        WHERE workspace.id = p_workspace_id
          AND workspace.archived_at IS NULL
    )
    AND CASE
        WHEN p_object_id IS NULL THEN
            p_reason = 'workspace_access_granted'
            AND (
                EXISTS (
                    SELECT 1
                    FROM kival.global_admins global_admin
                    WHERE global_admin.user_id = p_recipient_user_id
                      AND global_admin.revoked_at IS NULL
                )
                OR EXISTS (
                    SELECT 1
                    FROM kival.workspace_memberships membership
                    WHERE membership.workspace_id = p_workspace_id
                      AND membership.user_id = p_recipient_user_id
                      AND membership.revoked_at IS NULL
                )
            )
        ELSE
            EXISTS (
                SELECT 1
                FROM kival.objects object
                WHERE object.workspace_id = p_workspace_id
                  AND object.id = p_object_id
                  AND object.archived_at IS NULL
            )
            AND kival.has_object_permission(
                p_workspace_id,
                p_object_id,
                p_recipient_user_id,
                'viewer'::kival.object_role
            )
    END;
$$;

COMMENT ON FUNCTION kival.inbox_notification_is_visible(uuid, uuid, uuid, text) IS
    'Checks current recipient access for workspace- or object-scoped inbox entries.';

DROP TRIGGER IF EXISTS inbox_notifications_before_update ON kival.inbox_notifications;
CREATE TRIGGER inbox_notifications_before_update
BEFORE UPDATE ON kival.inbox_notifications
FOR EACH ROW
EXECUTE FUNCTION kival.before_update();

-- =====================================================================
-- Realtime invalidation
-- =====================================================================

-- ---------------------------------------------------------------------
-- Function: kival.publish_realtime_invalidation(recipient_user_id, type, workspace_id, object_id, event_id, inbox_entry_id)
-- Purpose
--   Publish one lightweight recipient-scoped realtime invalidation.
-- Parameters
--   p_recipient_user_id  Recipient that may consume the invalidation.
--   p_type               Invalidation type.
--   p_workspace_id       Optional workspace routing identifier.
--   p_object_id          Optional object routing identifier.
--   p_event_id           Optional durable source-event identifier.
--   p_inbox_entry_id     Optional inbox projection identifier.
-- Behavior
--   Emits JSON on `kival_realtime` through PostgreSQL NOTIFY. NOTIFY is
--   transactional, so listeners observe only committed projection state.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.publish_realtime_invalidation(
    p_recipient_user_id uuid,
    p_type text,
    p_workspace_id uuid DEFAULT NULL,
    p_object_id uuid DEFAULT NULL,
    p_event_id uuid DEFAULT NULL,
    p_inbox_entry_id uuid DEFAULT NULL
)
RETURNS void
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM pg_notify(
        'kival_realtime',
        json_build_object(
            'recipient_user_id', p_recipient_user_id,
            'type', p_type,
            'workspace_id', p_workspace_id,
            'object_id', p_object_id,
            'event_id', p_event_id,
            'inbox_entry_id', p_inbox_entry_id
        )::text
    );
END;
$$;

COMMENT ON FUNCTION kival.publish_realtime_invalidation(uuid, text, uuid, uuid, uuid, uuid) IS
    'Publishes a transaction-safe, recipient-scoped realtime invalidation.';

-- ---------------------------------------------------------------------
-- Function: kival.notify_inbox_projection_changed()
-- Purpose
--   Notify realtime listeners that a recipient's inbox projection changed.
-- Trigger contract
--   AFTER INSERT or selected projection-field UPDATE on `kival.inbox_notifications`.
-- Behavior
--   Publishes an `inbox.updated` invalidation carrying routing identifiers from
--   the committed inbox row.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.notify_inbox_projection_changed()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM kival.publish_realtime_invalidation(
        NEW.recipient_user_id,
        'inbox.updated',
        NEW.workspace_id,
        NEW.object_id,
        NEW.latest_event_id,
        NEW.id
    );
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS inbox_notifications_notify_projection_change
    ON kival.inbox_notifications;
CREATE TRIGGER inbox_notifications_notify_projection_change
AFTER INSERT OR UPDATE OF latest_event_id, event_count, notification_type, reason, archived_at
ON kival.inbox_notifications
FOR EACH ROW
EXECUTE FUNCTION kival.notify_inbox_projection_changed();

-- =====================================================================
-- Event-time candidate capture
-- =====================================================================

-- ---------------------------------------------------------------------
-- Function: kival.capture_notification_candidates()
-- Purpose
--   Capture notification eligibility transactionally at the durable event boundary.
-- Trigger contract
--   AFTER INSERT on `kival.events`.
-- Behavior
--   Derives notification type, reason, recipients, and deduplication keys from the
--   inserted event. Commentary uses stable thread/comment subjects; mentions and
--   replies use `target_user_id` for directed delivery. Candidate rows become the
--   durable work queue consumed asynchronously by projection workers.
-- Security semantics
--   Eligibility is frozen when the event commits, preventing users who gain access
--   later from receiving historical notifications. Subsequent inbox reads still
--   apply current visibility.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.capture_notification_candidates()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    v_reason text;
    v_notification_type text;
    v_directed boolean := FALSE;
    v_thread_id uuid := NEW.comment_thread_id;
    v_comment_id uuid := NEW.comment_id;
    v_deduplication_key text;
BEGIN
    -- Authorization and lifecycle transitions can invalidate state that the
    -- affected user is no longer allowed to identify by resource ID. Publish
    -- an identifier-free resync request transactionally with those events so
    -- clients re-read their authoritative HTTP state after commit.
    IF NEW.target_user_id IS NOT NULL
       AND NEW.event_kind IN (
            'workspace.membership_created',
            'workspace.membership_updated',
            'workspace.membership_revoked',
            'group.membership_created',
            'group.membership_updated',
            'group.membership_revoked',
            'object_grant.created',
            'object_grant.updated',
            'object_grant.revoked'
       )
    THEN
        PERFORM kival.publish_realtime_invalidation(
            NEW.target_user_id,
            'realtime.resync_required'
        );
    END IF;

    IF NEW.group_id IS NOT NULL
       AND NEW.event_kind IN (
            'group.archived',
            'group.unarchived',
            'workspace.group_linked',
            'workspace.group_archived',
            'workspace.group_unarchived',
            'object_grant.created',
            'object_grant.updated',
            'object_grant.revoked'
       )
    THEN
        PERFORM kival.publish_realtime_invalidation(
            membership.user_id,
            'realtime.resync_required'
        )
        FROM kival.group_memberships membership
        JOIN kival.users user_account ON user_account.id = membership.user_id
        WHERE membership.group_id = NEW.group_id
          AND membership.revoked_at IS NULL
          AND user_account.disabled_at IS NULL
          AND (
                NEW.workspace_id IS NULL
                OR EXISTS (
                    SELECT 1
                    FROM kival.workspace_memberships workspace_membership
                    WHERE workspace_membership.workspace_id = NEW.workspace_id
                      AND workspace_membership.user_id = membership.user_id
                      AND workspace_membership.revoked_at IS NULL
                )
              );
    END IF;

    IF NEW.workspace_id IS NOT NULL
       AND NEW.event_kind IN ('workspace.archived', 'workspace.unarchived')
    THEN
        PERFORM kival.publish_realtime_invalidation(
            candidate.user_id,
            'realtime.resync_required'
        )
        FROM (
            SELECT membership.user_id
            FROM kival.workspace_memberships membership
            JOIN kival.users user_account ON user_account.id = membership.user_id
            WHERE membership.workspace_id = NEW.workspace_id
              AND membership.revoked_at IS NULL
              AND user_account.disabled_at IS NULL

            UNION

            SELECT global_admin.user_id
            FROM kival.global_admins global_admin
            JOIN kival.users user_account ON user_account.id = global_admin.user_id
            WHERE global_admin.revoked_at IS NULL
              AND user_account.disabled_at IS NULL
        ) AS candidate;
    END IF;

    IF NEW.workspace_id IS NOT NULL
       AND NEW.object_id IS NOT NULL
       AND NEW.event_kind IN ('object.archived', 'object.unarchived')
    THEN
        PERFORM kival.publish_realtime_invalidation(
            candidate.user_id,
            'realtime.resync_required'
        )
        FROM (
            SELECT membership.user_id
            FROM kival.workspace_memberships membership
            JOIN kival.users user_account ON user_account.id = membership.user_id
            WHERE membership.workspace_id = NEW.workspace_id
              AND membership.revoked_at IS NULL
              AND user_account.disabled_at IS NULL

            UNION

            SELECT global_admin.user_id
            FROM kival.global_admins global_admin
            JOIN kival.users user_account ON user_account.id = global_admin.user_id
            WHERE global_admin.revoked_at IS NULL
              AND user_account.disabled_at IS NULL
        ) AS candidate
        WHERE kival.has_object_permission(
                NEW.workspace_id,
                NEW.object_id,
                candidate.user_id,
                'viewer'::kival.object_role
              );
    END IF;

    IF NEW.workspace_id IS NULL THEN
        RETURN NEW;
    END IF;

    v_deduplication_key := CASE
        WHEN v_comment_id IS NOT NULL THEN 'comment:' || v_comment_id::text
        ELSE 'event:' || NEW.id::text
    END;

    IF NEW.event_kind = 'workspace.membership_created'
       AND NEW.target_user_id IS NOT NULL
       AND NEW.target_user_id IS DISTINCT FROM NEW.actor_user_id
    THEN
        INSERT INTO kival.notification_candidates (
            event_id,
            recipient_user_id,
            workspace_id,
            object_id,
            actor_user_id,
            delivery_kind,
            notification_type,
            reason,
            deduplication_key
        )
        VALUES (
            NEW.id,
            NEW.target_user_id,
            NEW.workspace_id,
            NULL,
            NEW.actor_user_id,
            'inbox',
            NEW.event_kind,
            'workspace_access_granted',
            v_deduplication_key
        )
        ON CONFLICT (event_id, recipient_user_id, delivery_kind, reason) DO NOTHING;
    END IF;

    IF NEW.event_kind = 'object_grant.created'
       AND NEW.target_user_id IS NOT NULL
       AND NEW.target_user_id IS DISTINCT FROM NEW.actor_user_id
       AND kival.has_object_permission(
            NEW.workspace_id,
            NEW.object_id,
            NEW.target_user_id,
            'viewer'::kival.object_role
       )
    THEN
        INSERT INTO kival.notification_candidates (
            event_id,
            recipient_user_id,
            workspace_id,
            object_id,
            actor_user_id,
            delivery_kind,
            notification_type,
            reason,
            deduplication_key
        )
        VALUES (
            NEW.id,
            NEW.target_user_id,
            NEW.workspace_id,
            NEW.object_id,
            NEW.actor_user_id,
            'inbox',
            NEW.event_kind,
            'object_access_granted',
            v_deduplication_key
        )
        ON CONFLICT (event_id, recipient_user_id, delivery_kind, reason) DO NOTHING;
    END IF;

    CASE
        WHEN NEW.event_kind = 'comment.mentioned' THEN
            v_reason := 'mention';
            v_notification_type := 'mention';
            v_directed := TRUE;
        WHEN NEW.event_kind = 'comment.replied' THEN
            v_reason := 'reply';
            v_notification_type := 'reply';
            v_directed := TRUE;
        WHEN NEW.event_kind = 'review.requested' THEN
            v_reason := 'review_requested';
            v_notification_type := 'review_requested';
            v_directed := TRUE;
        WHEN NEW.event_kind = 'watcher.added' THEN
            v_reason := 'watcher_added';
            v_notification_type := 'watcher_added';
            v_directed := TRUE;
        ELSE
            NULL;
    END CASE;

    IF v_directed
       AND NEW.target_user_id IS NOT NULL
       AND NEW.target_user_id IS DISTINCT FROM NEW.actor_user_id
       AND kival.has_object_permission(
            NEW.workspace_id,
            NEW.object_id,
            NEW.target_user_id,
            'viewer'::kival.object_role
       )
    THEN
        INSERT INTO kival.notification_candidates (
            event_id,
            recipient_user_id,
            workspace_id,
            object_id,
            actor_user_id,
            delivery_kind,
            notification_type,
            reason,
            deduplication_key,
            thread_id,
            comment_id
        )
        VALUES (
            NEW.id,
            NEW.target_user_id,
            NEW.workspace_id,
            NEW.object_id,
            NEW.actor_user_id,
            'inbox',
            v_notification_type,
            v_reason,
            v_deduplication_key,
            v_thread_id,
            v_comment_id
        )
        ON CONFLICT (event_id, recipient_user_id, delivery_kind, reason) DO NOTHING;
    END IF;

    IF NEW.event_kind IN (
        'comment.created',
        'comment.replied',
        'comment_thread.resolved',
        'comment_thread.reopened'
    ) THEN
        INSERT INTO kival.notification_candidates (
            event_id,
            recipient_user_id,
            workspace_id,
            object_id,
            actor_user_id,
            delivery_kind,
            notification_type,
            reason,
            deduplication_key,
            thread_id,
            comment_id
        )
        SELECT
            NEW.id,
            candidate.user_id,
            NEW.workspace_id,
            NEW.object_id,
            NEW.actor_user_id,
            'inbox',
            NEW.event_kind,
            'object_activity',
            v_deduplication_key,
            v_thread_id,
            v_comment_id
        FROM (
            SELECT thread_comment.author_user_id AS user_id
            FROM kival.comments thread_comment
            WHERE thread_comment.workspace_id = NEW.workspace_id
              AND thread_comment.object_id = NEW.object_id
              AND thread_comment.thread_id = v_thread_id

            UNION

            SELECT mention.mentioned_user_id
            FROM kival.comment_mentions mention
            JOIN kival.comments thread_comment
              ON thread_comment.workspace_id = mention.workspace_id
             AND thread_comment.object_id = mention.object_id
             AND thread_comment.id = mention.comment_id
            WHERE thread_comment.workspace_id = NEW.workspace_id
              AND thread_comment.object_id = NEW.object_id
              AND thread_comment.thread_id = v_thread_id

            UNION

            SELECT preference.user_id
            FROM kival.object_notification_preferences preference
            WHERE preference.workspace_id = NEW.workspace_id
              AND preference.object_id = NEW.object_id
              AND preference.ordinary_notifications_enabled
        ) AS participant
        JOIN (
            SELECT membership.user_id
            FROM kival.workspace_memberships membership
            JOIN kival.users user_account ON user_account.id = membership.user_id
            WHERE membership.workspace_id = NEW.workspace_id
              AND membership.revoked_at IS NULL
              AND user_account.disabled_at IS NULL

            UNION

            SELECT global_admin.user_id
            FROM kival.global_admins global_admin
            JOIN kival.users user_account ON user_account.id = global_admin.user_id
            WHERE global_admin.revoked_at IS NULL
              AND user_account.disabled_at IS NULL
        ) AS candidate ON candidate.user_id = participant.user_id
        WHERE candidate.user_id IS DISTINCT FROM NEW.actor_user_id
          AND candidate.user_id IS DISTINCT FROM CASE
                WHEN v_directed THEN NEW.target_user_id
                ELSE NULL::uuid
              END
          AND kival.has_object_permission(
                NEW.workspace_id,
                NEW.object_id,
                candidate.user_id,
                'viewer'::kival.object_role
              )
          AND COALESCE(
                (
                    SELECT preference.ordinary_notifications_enabled
                    FROM kival.object_notification_preferences preference
                    WHERE preference.user_id = candidate.user_id
                      AND preference.workspace_id = NEW.workspace_id
                      AND preference.object_id = NEW.object_id
                ),
                TRUE
              )
        ON CONFLICT (event_id, recipient_user_id, delivery_kind, reason) DO NOTHING;
    END IF;

    IF NEW.event_kind = 'object.updated' THEN
        INSERT INTO kival.notification_candidates (
            event_id,
            recipient_user_id,
            workspace_id,
            object_id,
            actor_user_id,
            delivery_kind,
            notification_type,
            reason,
            deduplication_key,
            thread_id,
            comment_id
        )
        SELECT
            NEW.id,
            candidate.user_id,
            NEW.workspace_id,
            NEW.object_id,
            NEW.actor_user_id,
            'inbox',
            'object_activity',
            'object_activity',
            v_deduplication_key,
            v_thread_id,
            v_comment_id
        FROM (
            SELECT membership.user_id
            FROM kival.workspace_memberships membership
            JOIN kival.users user_account ON user_account.id = membership.user_id
            WHERE membership.workspace_id = NEW.workspace_id
              AND membership.revoked_at IS NULL
              AND user_account.disabled_at IS NULL

            UNION

            SELECT global_admin.user_id
            FROM kival.global_admins global_admin
            JOIN kival.users user_account ON user_account.id = global_admin.user_id
            WHERE global_admin.revoked_at IS NULL
              AND user_account.disabled_at IS NULL
        ) AS candidate
        WHERE candidate.user_id IS DISTINCT FROM NEW.actor_user_id
          AND kival.has_object_permission(
                NEW.workspace_id,
                NEW.object_id,
                candidate.user_id,
                'viewer'::kival.object_role
              )
          AND COALESCE(
                (
                    SELECT preference.ordinary_notifications_enabled
                    FROM kival.object_notification_preferences preference
                    WHERE preference.user_id = candidate.user_id
                      AND preference.workspace_id = NEW.workspace_id
                      AND preference.object_id = NEW.object_id
                ),
                TRUE
              )
        ON CONFLICT (event_id, recipient_user_id, delivery_kind, reason) DO NOTHING;
    END IF;

    -- Realtime is an authorization-scoped state invalidation path, not an
    -- attention preference. Mention audit events do not need their own broad
    -- invalidation because the corresponding create/edit event already carries
    -- the commentary state change and inbox projection notifies the target.
    IF NEW.event_kind IN (
        'comment.created',
        'comment.replied',
        'comment.edited',
        'comment.deleted',
        'comment_thread.resolved',
        'comment_thread.reopened',
        'object.updated'
    ) THEN
        INSERT INTO kival.notification_candidates (
            event_id,
            recipient_user_id,
            workspace_id,
            object_id,
            actor_user_id,
            delivery_kind,
            notification_type,
            reason,
            deduplication_key,
            thread_id,
            comment_id
        )
        SELECT
            NEW.id,
            candidate.user_id,
            NEW.workspace_id,
            NEW.object_id,
            NEW.actor_user_id,
            'realtime',
            CASE
                WHEN NEW.event_kind IN (
                    'comment.created',
                    'comment.replied',
                    'comment.edited',
                    'comment.deleted',
                    'comment_thread.resolved',
                    'comment_thread.reopened'
                ) THEN NEW.event_kind
                ELSE 'object.activity'
            END,
            'invalidation',
            'event:' || NEW.id::text,
            v_thread_id,
            v_comment_id
        FROM (
            SELECT membership.user_id
            FROM kival.workspace_memberships membership
            JOIN kival.users user_account ON user_account.id = membership.user_id
            WHERE membership.workspace_id = NEW.workspace_id
              AND membership.revoked_at IS NULL
              AND user_account.disabled_at IS NULL

            UNION

            SELECT global_admin.user_id
            FROM kival.global_admins global_admin
            JOIN kival.users user_account ON user_account.id = global_admin.user_id
            WHERE global_admin.revoked_at IS NULL
              AND user_account.disabled_at IS NULL
        ) AS candidate
        WHERE kival.has_object_permission(
                NEW.workspace_id,
                NEW.object_id,
                candidate.user_id,
                'viewer'::kival.object_role
              )
        ON CONFLICT (event_id, recipient_user_id, delivery_kind, reason) DO NOTHING;
    END IF;

    RETURN NEW;
END;
$$;

COMMENT ON FUNCTION kival.capture_notification_candidates() IS
    'Captures directed and participation-scoped inbox candidates plus authorization-scoped realtime invalidations.';

DROP TRIGGER IF EXISTS events_capture_notification_candidates ON kival.events;
CREATE TRIGGER events_capture_notification_candidates
AFTER INSERT ON kival.events
FOR EACH ROW
EXECUTE FUNCTION kival.capture_notification_candidates();

-- =====================================================================
-- Notification retention
-- =====================================================================

-- ---------------------------------------------------------------------
-- Function: kival.apply_notification_retention(limit)
-- Purpose
--   Delete expired candidate and inbox state in bounded batches.
-- Parameters
--   p_limit  Maximum rows deleted from each retained relation; constrained to the
--            function's accepted batch range.
-- Returns
--   Counts of deleted notification-candidate and inbox rows.
-- Concurrency semantics
--   Uses ordered `FOR UPDATE SKIP LOCKED` batches so multiple server instances can
--   clean up concurrently without blocking active projection or inbox mutations.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.apply_notification_retention(p_limit integer DEFAULT 256)
RETURNS TABLE (
    candidates_deleted integer,
    inbox_deleted integer
)
LANGUAGE plpgsql
AS $$
BEGIN
    IF p_limit < 1 OR p_limit > 1000 THEN
        RAISE EXCEPTION 'notification retention batch limit must be between 1 and 1000';
    END IF;

    WITH expired AS (
        SELECT candidate.id
        FROM kival.notification_candidates candidate
        WHERE candidate.expires_at <= now()
        ORDER BY candidate.expires_at ASC, candidate.id ASC
        FOR UPDATE SKIP LOCKED
        LIMIT p_limit
    )
    DELETE FROM kival.notification_candidates candidate
    USING expired
    WHERE candidate.id = expired.id;
    GET DIAGNOSTICS candidates_deleted = ROW_COUNT;

    WITH expired AS (
        SELECT inbox.id
        FROM kival.inbox_notifications inbox
        WHERE inbox.expires_at <= now()
        ORDER BY inbox.expires_at ASC, inbox.id ASC
        FOR UPDATE SKIP LOCKED
        LIMIT p_limit
    )
    DELETE FROM kival.inbox_notifications inbox
    USING expired
    WHERE inbox.id = expired.id;
    GET DIAGNOSTICS inbox_deleted = ROW_COUNT;

    RETURN NEXT;
END;
$$;

COMMENT ON FUNCTION kival.apply_notification_retention(integer) IS
    'Deletes bounded batches of expired notification candidates and inbox rows without blocking live work.';

-- =====================================================================
-- Candidate projection
-- =====================================================================

-- ---------------------------------------------------------------------
-- Function: kival.process_notification_candidate_batch(limit)
-- Purpose
--   Claim and project a bounded batch of durable notification candidates.
-- Parameters
--   p_limit  Maximum pending candidates claimed by this invocation.
-- Returns
--   Counts of processed candidates and changed inbox rows plus the remaining
--   unprojected candidate count.
-- Behavior
--   Projects inbox deliveries idempotently, publishes realtime-only deliveries,
--   marks handled candidates as projected, and preserves deterministic grouped
--   inbox state using candidate sequence numbers rather than worker acquisition order.
-- Concurrency semantics
--   Uses `FOR UPDATE SKIP LOCKED`, allowing multiple server instances to consume
--   the durable candidate queue concurrently without assuming event commit order.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.process_notification_candidate_batch(p_limit integer DEFAULT 100)
RETURNS TABLE (
    candidates_processed integer,
    notifications_changed integer,
    remaining_candidate_lag bigint
)
LANGUAGE plpgsql
AS $$
DECLARE
    v_candidate kival.notification_candidates%ROWTYPE;
    v_rows integer;
BEGIN
    IF p_limit < 1 OR p_limit > 1000 THEN
        RAISE EXCEPTION 'notification candidate batch limit must be between 1 and 1000';
    END IF;

    candidates_processed := 0;
    notifications_changed := 0;

    FOR v_candidate IN
        SELECT candidate.*
        FROM kival.notification_candidates candidate
        WHERE candidate.projected_at IS NULL
        ORDER BY candidate.sequence_number ASC
        FOR UPDATE SKIP LOCKED
        LIMIT p_limit
    LOOP
        IF v_candidate.expires_at <= now() THEN
            UPDATE kival.notification_candidates
            SET projected_at = now()
            WHERE id = v_candidate.id
              AND projected_at IS NULL;
        ELSIF v_candidate.delivery_kind = 'inbox' THEN
            INSERT INTO kival.inbox_notifications (
                sequence_number,
                source_candidate_sequence_number,
                directed_candidate_sequence_number,
                recipient_user_id,
                workspace_id,
                object_id,
                source_event_id,
                latest_event_id,
                actor_user_id,
                notification_type,
                reason,
                deduplication_key,
                thread_id,
                comment_id,
                expires_at
            )
            VALUES (
                v_candidate.sequence_number,
                v_candidate.sequence_number,
                CASE
                    WHEN v_candidate.reason IN ('mention', 'reply', 'review_requested', 'watcher_added')
                    THEN v_candidate.sequence_number
                    ELSE NULL
                END,
                v_candidate.recipient_user_id,
                v_candidate.workspace_id,
                v_candidate.object_id,
                v_candidate.event_id,
                v_candidate.event_id,
                v_candidate.actor_user_id,
                v_candidate.notification_type,
                v_candidate.reason,
                v_candidate.deduplication_key,
                v_candidate.thread_id,
                v_candidate.comment_id,
                v_candidate.created_at + interval '180 days'
            )
            ON CONFLICT (recipient_user_id, deduplication_key) DO UPDATE
            SET source_event_id = CASE
                    WHEN EXCLUDED.source_candidate_sequence_number
                         < kival.inbox_notifications.source_candidate_sequence_number
                    THEN EXCLUDED.source_event_id
                    ELSE kival.inbox_notifications.source_event_id
                END,
                latest_event_id = CASE
                    WHEN EXCLUDED.sequence_number > kival.inbox_notifications.sequence_number
                    THEN EXCLUDED.latest_event_id
                    ELSE kival.inbox_notifications.latest_event_id
                END,
                actor_user_id = CASE
                    WHEN EXCLUDED.sequence_number > kival.inbox_notifications.sequence_number
                    THEN EXCLUDED.actor_user_id
                    ELSE kival.inbox_notifications.actor_user_id
                END,
                notification_type = CASE
                    WHEN EXCLUDED.directed_candidate_sequence_number IS NOT NULL
                         AND (
                             kival.inbox_notifications.directed_candidate_sequence_number IS NULL
                             OR EXCLUDED.directed_candidate_sequence_number
                                > kival.inbox_notifications.directed_candidate_sequence_number
                         )
                    THEN EXCLUDED.notification_type
                    WHEN kival.inbox_notifications.directed_candidate_sequence_number IS NULL
                         AND EXCLUDED.source_candidate_sequence_number
                             < kival.inbox_notifications.source_candidate_sequence_number
                    THEN EXCLUDED.notification_type
                    ELSE kival.inbox_notifications.notification_type
                END,
                reason = CASE
                    WHEN EXCLUDED.directed_candidate_sequence_number IS NOT NULL
                         AND (
                             kival.inbox_notifications.directed_candidate_sequence_number IS NULL
                             OR EXCLUDED.directed_candidate_sequence_number
                                > kival.inbox_notifications.directed_candidate_sequence_number
                         )
                    THEN EXCLUDED.reason
                    WHEN kival.inbox_notifications.directed_candidate_sequence_number IS NULL
                         AND EXCLUDED.source_candidate_sequence_number
                             < kival.inbox_notifications.source_candidate_sequence_number
                    THEN EXCLUDED.reason
                    ELSE kival.inbox_notifications.reason
                END,
                event_count = kival.inbox_notifications.event_count + 1,
                thread_id = CASE
                    WHEN EXCLUDED.sequence_number > kival.inbox_notifications.sequence_number
                    THEN COALESCE(EXCLUDED.thread_id, kival.inbox_notifications.thread_id)
                    ELSE kival.inbox_notifications.thread_id
                END,
                comment_id = CASE
                    WHEN EXCLUDED.sequence_number > kival.inbox_notifications.sequence_number
                    THEN COALESCE(EXCLUDED.comment_id, kival.inbox_notifications.comment_id)
                    ELSE kival.inbox_notifications.comment_id
                END,
                read_at = CASE
                    WHEN EXCLUDED.sequence_number > kival.inbox_notifications.sequence_number
                    THEN NULL
                    ELSE kival.inbox_notifications.read_at
                END,
                archived_at = CASE
                    WHEN EXCLUDED.sequence_number > kival.inbox_notifications.sequence_number
                    THEN NULL
                    ELSE kival.inbox_notifications.archived_at
                END,
                expires_at = GREATEST(
                    kival.inbox_notifications.expires_at,
                    EXCLUDED.expires_at
                ),
                source_candidate_sequence_number = LEAST(
                    kival.inbox_notifications.source_candidate_sequence_number,
                    EXCLUDED.source_candidate_sequence_number
                ),
                directed_candidate_sequence_number = CASE
                    WHEN kival.inbox_notifications.directed_candidate_sequence_number IS NULL
                    THEN EXCLUDED.directed_candidate_sequence_number
                    WHEN EXCLUDED.directed_candidate_sequence_number IS NULL
                    THEN kival.inbox_notifications.directed_candidate_sequence_number
                    ELSE GREATEST(
                        kival.inbox_notifications.directed_candidate_sequence_number,
                        EXCLUDED.directed_candidate_sequence_number
                    )
                END,
                sequence_number = GREATEST(
                    kival.inbox_notifications.sequence_number,
                    EXCLUDED.sequence_number
                );

            GET DIAGNOSTICS v_rows = ROW_COUNT;
            notifications_changed := notifications_changed + v_rows;

            UPDATE kival.notification_candidates
            SET projected_at = now()
            WHERE id = v_candidate.id
              AND projected_at IS NULL;
        ELSE
            PERFORM kival.publish_realtime_invalidation(
                v_candidate.recipient_user_id,
                v_candidate.notification_type,
                v_candidate.workspace_id,
                v_candidate.object_id,
                v_candidate.event_id,
                NULL
            );

            UPDATE kival.notification_candidates
            SET projected_at = now()
            WHERE id = v_candidate.id
              AND projected_at IS NULL;
        END IF;

        candidates_processed := candidates_processed + 1;
    END LOOP;

    SELECT count(*)
    INTO remaining_candidate_lag
    FROM kival.notification_candidates candidate
    WHERE candidate.projected_at IS NULL;

    RETURN NEXT;
END;
$$;

COMMENT ON FUNCTION kival.process_notification_candidate_batch(integer) IS
    'Claims and projects a bounded batch of durable notification candidates without event commit-order assumptions.';
