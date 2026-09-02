-- =====================================================================
-- Kival migration 0006: events
-- =====================================================================
-- Purpose
--   Define Kival's append-only event log for auditable domain activity, including
--   optional workspace, object, group, target-user, and API-key attribution.
--
-- Depends on
--   All preceding migrations. Event foreign keys may refer to users,
--   workspaces, objects, versions, edges, grants, groups, and API keys.
--
-- Owns
--   * `kival.events`
--   * Event workspace-consistency validation.
--   * Database-level event immutability triggers.
--
-- Design notes
--   Events are immutable after insertion. Object-related event subjects must
--   belong to the event's declared workspace. API-key attribution is captured as
--   both a foreign key and label snapshot tuple so the actor remains explicit.
-- =====================================================================

-- =====================================================================
-- Append-only event log
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.events (
    id uuid PRIMARY KEY DEFAULT uuidv7(),

    sequence_number bigint GENERATED ALWAYS AS IDENTITY UNIQUE,

    workspace_id uuid REFERENCES kival.workspaces(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    actor_user_id uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    api_key_id uuid,
    api_key_label text,

    event_kind text NOT NULL,

    object_id uuid REFERENCES kival.objects(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    object_version_id uuid REFERENCES kival.object_versions(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    object_edge_id uuid REFERENCES kival.object_edges(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    object_grant_id uuid REFERENCES kival.object_grants(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    group_id uuid REFERENCES kival.groups(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    target_user_id uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    payload jsonb NOT NULL DEFAULT '{}'::jsonb,

    created_at timestamptz NOT NULL DEFAULT now(),

    CONSTRAINT events_event_kind_not_blank
        CHECK (length(btrim(event_kind)) > 0),

    CONSTRAINT events_payload_is_object
        CHECK (jsonb_typeof(payload) = 'object'),

    CONSTRAINT events_api_key_attribution_complete
        CHECK (
            (api_key_id IS NULL AND api_key_label IS NULL)
            OR
            (api_key_id IS NOT NULL AND actor_user_id IS NOT NULL AND api_key_label IS NOT NULL)
        ),

    CONSTRAINT events_api_key_attribution_matches_key
        FOREIGN KEY (api_key_id, actor_user_id, api_key_label)
        REFERENCES kival.api_keys (id, user_id, label)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS events_workspace_sequence_idx
    ON kival.events (workspace_id, sequence_number);

CREATE INDEX IF NOT EXISTS events_created_at_idx
    ON kival.events (created_at DESC);

CREATE INDEX IF NOT EXISTS events_actor_user_idx
    ON kival.events (actor_user_id, sequence_number);

CREATE INDEX IF NOT EXISTS events_target_user_idx
    ON kival.events (target_user_id, sequence_number)
    WHERE target_user_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS events_object_idx
    ON kival.events (object_id, sequence_number)
    WHERE object_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS events_group_idx
    ON kival.events (group_id, sequence_number)
    WHERE group_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS events_event_kind_idx
    ON kival.events (event_kind, sequence_number);

CREATE INDEX IF NOT EXISTS events_api_key_idx
    ON kival.events (api_key_id, sequence_number)
    WHERE api_key_id IS NOT NULL;

-- ---------------------------------------------------------------------
-- Function: kival.events_before_insert()
-- Purpose
--   Validate workspace attribution for object-related events before persistence.
-- Trigger contract
--   BEFORE INSERT on `kival.events`.
-- Behavior
--   For each populated object, edge, grant, or object-version foreign key, requires
--   `workspace_id` and verifies that the referenced subject belongs to that same
--   workspace. Events without those object-related subjects may remain global.
--   Foreign-key constraints separately guarantee that referenced rows exist.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.events_before_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    v_workspace_id uuid;
BEGIN
    IF NEW.object_id IS NOT NULL THEN
        SELECT o.workspace_id
        INTO v_workspace_id
        FROM kival.objects o
        WHERE o.id = NEW.object_id;

        IF NEW.workspace_id IS NULL THEN
            RAISE EXCEPTION 'workspace_id is required for object event';
        END IF;

        IF v_workspace_id IS DISTINCT FROM NEW.workspace_id THEN
            RAISE EXCEPTION 'event workspace_id does not match object workspace';
        END IF;
    END IF;

    IF NEW.object_edge_id IS NOT NULL THEN
        SELECT oe.workspace_id
        INTO v_workspace_id
        FROM kival.object_edges oe
        WHERE oe.id = NEW.object_edge_id;

        IF NEW.workspace_id IS NULL THEN
            RAISE EXCEPTION 'workspace_id is required for object_edge event';
        END IF;

        IF v_workspace_id IS DISTINCT FROM NEW.workspace_id THEN
            RAISE EXCEPTION 'event workspace_id does not match object_edge workspace';
        END IF;
    END IF;

    IF NEW.object_grant_id IS NOT NULL THEN
        SELECT og.workspace_id
        INTO v_workspace_id
        FROM kival.object_grants og
        WHERE og.id = NEW.object_grant_id;

        IF NEW.workspace_id IS NULL THEN
            RAISE EXCEPTION 'workspace_id is required for object_grant event';
        END IF;

        IF v_workspace_id IS DISTINCT FROM NEW.workspace_id THEN
            RAISE EXCEPTION 'event workspace_id does not match object_grant workspace';
        END IF;
    END IF;

    IF NEW.object_version_id IS NOT NULL THEN
        SELECT o.workspace_id
        INTO v_workspace_id
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

    RETURN NEW;
END;
$$;

COMMENT ON FUNCTION kival.events_before_insert() IS
    'Validates that object-related event subjects belong to the event workspace before insertion.';

DROP TRIGGER IF EXISTS events_before_insert ON kival.events;

CREATE TRIGGER events_before_insert
BEFORE INSERT ON kival.events
FOR EACH ROW
EXECUTE FUNCTION kival.events_before_insert();

DROP TRIGGER IF EXISTS events_prevent_update ON kival.events;

CREATE TRIGGER events_prevent_update
BEFORE UPDATE ON kival.events
FOR EACH ROW
EXECUTE FUNCTION kival.prevent_mutation();

DROP TRIGGER IF EXISTS events_prevent_delete ON kival.events;

CREATE TRIGGER events_prevent_delete
BEFORE DELETE ON kival.events
FOR EACH ROW
EXECUTE FUNCTION kival.prevent_mutation();
