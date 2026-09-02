-- =====================================================================
-- Kival migration 0004: objects
-- =====================================================================
-- Purpose
--   Define Kival's core knowledge model: objects, immutable versions,
--   attachments, explicit object edges, and derived textual references.
--
-- Depends on
--   * 0000_setup.sql for lifecycle and immutability trigger helpers.
--   * 0001_identity.sql for actor attribution.
--   * 0002_workspaces.sql for workspace identity and isolation.
--
-- Owns
--   * `kival.objects`
--   * `kival.object_versions`
--   * `kival.object_attachments`
--   * `kival.object_edges`
--   * `kival.object_references`
--
-- Design notes
--   The graph is part of the object model rather than a separate schema domain.
--   Versions are immutable historical records; an object points at its current
--   version. Metadata is intentionally flat: values are JSON scalars or
--   one-dimensional arrays of scalars. Explicit edges are revocable lifecycle
--   records. Textual references
--   are derived data and may transition from resolved to unresolved when a
--   referenced target disappears.
-- =====================================================================

-- =====================================================================
-- Objects and immutable versions
-- =====================================================================

-- ---------------------------------------------------------------------
-- Function: kival.metadata_is_flat(value)
-- Purpose
--   Validate the intentionally flat JSON shape accepted for object metadata.
-- Parameters
--   value  Candidate metadata value.
-- Returns
--   TRUE only for a JSON object whose members are scalars or one-dimensional
--   arrays of scalars; nested objects and nested arrays are rejected.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.metadata_is_flat(value jsonb)
RETURNS boolean
LANGUAGE sql
IMMUTABLE
PARALLEL SAFE
AS $$
    SELECT CASE
        WHEN jsonb_typeof(value) <> 'object' THEN false
        ELSE NOT EXISTS (
            SELECT 1
            FROM jsonb_each(value) AS entry(metadata_key, metadata_value)
            WHERE CASE jsonb_typeof(metadata_value)
                WHEN 'object' THEN true
                WHEN 'array' THEN EXISTS (
                    SELECT 1
                    FROM jsonb_array_elements(metadata_value) AS item(item_value)
                    WHERE jsonb_typeof(item.item_value) IN ('array', 'object')
                )
                ELSE false
            END
        )
    END;
$$;

COMMENT ON FUNCTION kival.metadata_is_flat(jsonb) IS
    'Returns whether metadata is an object containing only JSON scalars or one-dimensional scalar arrays.';

CREATE TABLE IF NOT EXISTS kival.objects (
    id uuid PRIMARY KEY DEFAULT uuidv7(),

    workspace_id uuid NOT NULL REFERENCES kival.workspaces(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    current_version_id uuid,

    status text NOT NULL DEFAULT 'active',

    created_by uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    archived_by uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),

    archived_at timestamptz,

    CONSTRAINT objects_status_valid
        CHECK (status IN ('active', 'archived')),

    CONSTRAINT objects_archive_complete
        CHECK (
            (status = 'active' AND archived_at IS NULL AND archived_by IS NULL)
            OR
            (status = 'archived' AND archived_at IS NOT NULL AND archived_by IS NOT NULL)
        ),

    CONSTRAINT objects_archived_at_after_created_at
        CHECK (archived_at IS NULL OR archived_at >= created_at),

    CONSTRAINT objects_updated_at_after_created_at
        CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS objects_workspace_id_id_unique
    ON kival.objects (workspace_id, id);

CREATE INDEX IF NOT EXISTS objects_workspace_active_idx
    ON kival.objects (workspace_id, id)
    WHERE archived_at IS NULL;

CREATE INDEX IF NOT EXISTS objects_created_by_idx
    ON kival.objects (created_by);

CREATE INDEX IF NOT EXISTS objects_archived_by_idx
    ON kival.objects (archived_by);

DROP TRIGGER IF EXISTS objects_before_update ON kival.objects;

CREATE TRIGGER objects_before_update
BEFORE UPDATE ON kival.objects
FOR EACH ROW
EXECUTE FUNCTION kival.before_update();

CREATE TABLE IF NOT EXISTS kival.object_versions (
    id uuid PRIMARY KEY DEFAULT uuidv7(),

    object_id uuid NOT NULL REFERENCES kival.objects(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    version_number bigint NOT NULL,

    title text NOT NULL,

    body_text text NOT NULL DEFAULT '',

    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,

    created_by uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    created_at timestamptz NOT NULL DEFAULT now(),

    CONSTRAINT object_versions_version_number_positive
        CHECK (version_number > 0),

    CONSTRAINT object_versions_title_not_blank
        CHECK (length(btrim(title)) > 0),

    CONSTRAINT object_versions_metadata_is_flat
        CHECK (kival.metadata_is_flat(metadata))
);

CREATE UNIQUE INDEX IF NOT EXISTS object_versions_object_version_number_unique
    ON kival.object_versions (object_id, version_number);

CREATE UNIQUE INDEX IF NOT EXISTS object_versions_object_id_id_unique
    ON kival.object_versions (object_id, id);

CREATE INDEX IF NOT EXISTS object_versions_object_id_created_at_idx
    ON kival.object_versions (object_id, created_at DESC);

CREATE INDEX IF NOT EXISTS object_versions_created_by_idx
    ON kival.object_versions (created_by);

DROP TRIGGER IF EXISTS object_versions_prevent_update ON kival.object_versions;

CREATE TRIGGER object_versions_prevent_update
BEFORE UPDATE ON kival.object_versions
FOR EACH ROW
EXECUTE FUNCTION kival.prevent_mutation();

DROP TRIGGER IF EXISTS object_versions_prevent_delete ON kival.object_versions;

CREATE TRIGGER object_versions_prevent_delete
BEFORE DELETE ON kival.object_versions
FOR EACH ROW
EXECUTE FUNCTION kival.prevent_mutation();

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint c
        JOIN pg_class r ON r.oid = c.conrelid
        JOIN pg_namespace n ON n.oid = r.relnamespace
        WHERE n.nspname = 'kival'
          AND r.relname = 'objects'
          AND c.conname = 'objects_current_version_belongs_to_object_fk'
    ) THEN
        ALTER TABLE kival.objects
        ADD CONSTRAINT objects_current_version_belongs_to_object_fk
        FOREIGN KEY (id, current_version_id)
        REFERENCES kival.object_versions (object_id, id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT;
    END IF;
END;
$$;

-- =====================================================================
-- Object attachments
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.object_attachments (
    id uuid PRIMARY KEY DEFAULT uuidv7(),

    workspace_id uuid NOT NULL,
    object_id uuid NOT NULL,
    version_id uuid,

    blob_ref text NOT NULL,
    size_bytes bigint NOT NULL,
    source_attachment_id uuid REFERENCES kival.object_attachments(id)
        ON UPDATE RESTRICT
        ON DELETE SET NULL,

    name text,
    media_type text,
    metadata jsonb NOT NULL DEFAULT '{}'::jsonb,

    created_by uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    created_at timestamptz NOT NULL DEFAULT now(),

    CONSTRAINT object_attachments_object_fk
        FOREIGN KEY (workspace_id, object_id)
        REFERENCES kival.objects (workspace_id, id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    CONSTRAINT object_attachments_version_fk
        FOREIGN KEY (object_id, version_id)
        REFERENCES kival.object_versions (object_id, id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    CONSTRAINT object_attachments_blob_ref_valid
        CHECK (blob_ref ~ '^[0-9a-f]{64}$'),

    CONSTRAINT object_attachments_size_bytes_non_negative
        CHECK (size_bytes >= 0),

    CONSTRAINT object_attachments_name_not_blank
        CHECK (name IS NULL OR length(btrim(name)) > 0),

    CONSTRAINT object_attachments_media_type_not_blank
        CHECK (media_type IS NULL OR length(btrim(media_type)) > 0),

    CONSTRAINT object_attachments_metadata_is_flat
        CHECK (kival.metadata_is_flat(metadata))
);

CREATE INDEX IF NOT EXISTS object_attachments_object_idx
    ON kival.object_attachments (workspace_id, object_id, created_at DESC);

CREATE INDEX IF NOT EXISTS object_attachments_version_idx
    ON kival.object_attachments (object_id, version_id, created_at DESC)
    WHERE version_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS object_attachments_blob_ref_idx
    ON kival.object_attachments (blob_ref);

CREATE INDEX IF NOT EXISTS object_attachments_source_attachment_id_idx
    ON kival.object_attachments (source_attachment_id)
    WHERE source_attachment_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS object_attachments_created_by_idx
    ON kival.object_attachments (created_by);


-- =====================================================================
-- Object edges
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.object_edges (
    id uuid PRIMARY KEY DEFAULT uuidv7(),

    workspace_id uuid NOT NULL REFERENCES kival.workspaces(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    source_object_id uuid NOT NULL,
    target_object_id uuid NOT NULL,

    created_by uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    revoked_by uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),

    revoked_at timestamptz,

    CONSTRAINT object_edges_source_object_fk
        FOREIGN KEY (workspace_id, source_object_id)
        REFERENCES kival.objects (workspace_id, id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    CONSTRAINT object_edges_target_object_fk
        FOREIGN KEY (workspace_id, target_object_id)
        REFERENCES kival.objects (workspace_id, id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    CONSTRAINT object_edges_no_self_edge
        CHECK (source_object_id <> target_object_id),

    CONSTRAINT object_edges_revocation_complete
        CHECK (
            (revoked_at IS NULL AND revoked_by IS NULL)
            OR
            (revoked_at IS NOT NULL AND revoked_by IS NOT NULL)
        ),

    CONSTRAINT object_edges_revoked_at_after_created_at
        CHECK (revoked_at IS NULL OR revoked_at >= created_at),

    CONSTRAINT object_edges_updated_at_after_created_at
        CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS object_edges_one_active_edge
    ON kival.object_edges (
        workspace_id,
        source_object_id,
        target_object_id
    )
    WHERE revoked_at IS NULL;

CREATE INDEX IF NOT EXISTS object_edges_target_active_idx
    ON kival.object_edges (workspace_id, target_object_id, source_object_id)
    WHERE revoked_at IS NULL;

CREATE INDEX IF NOT EXISTS object_edges_workspace_active_idx
    ON kival.object_edges (workspace_id)
    WHERE revoked_at IS NULL;

CREATE INDEX IF NOT EXISTS object_edges_created_by_idx
    ON kival.object_edges (created_by);

CREATE INDEX IF NOT EXISTS object_edges_revoked_by_idx
    ON kival.object_edges (revoked_by);

DROP TRIGGER IF EXISTS object_edges_before_update ON kival.object_edges;

CREATE TRIGGER object_edges_before_update
BEFORE UPDATE ON kival.object_edges
FOR EACH ROW
EXECUTE FUNCTION kival.before_update_lifecycle_only();

-- =====================================================================
-- Derived object references
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.object_references (
    id uuid PRIMARY KEY DEFAULT uuidv7(),

    workspace_id uuid NOT NULL REFERENCES kival.workspaces(id)
        ON UPDATE RESTRICT
        ON DELETE CASCADE,

    source_object_id uuid NOT NULL,
    source_version_id uuid NOT NULL,

    target_object_id uuid REFERENCES kival.objects(id)
        ON UPDATE RESTRICT
        ON DELETE SET NULL,

    raw_target text NOT NULL,
    display_text text,
    reference_kind text NOT NULL,

    span_start integer NOT NULL,
    span_end integer NOT NULL,

    status text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),

    CONSTRAINT object_references_source_object_fk
        FOREIGN KEY (workspace_id, source_object_id)
        REFERENCES kival.objects (workspace_id, id)
        ON UPDATE RESTRICT
        ON DELETE CASCADE,

    CONSTRAINT object_references_source_version_fk
        FOREIGN KEY (source_object_id, source_version_id)
        REFERENCES kival.object_versions (object_id, id)
        ON UPDATE RESTRICT
        ON DELETE CASCADE,

    CONSTRAINT object_references_raw_target_not_blank
        CHECK (length(btrim(raw_target)) > 0),

    CONSTRAINT object_references_display_text_not_blank
        CHECK (display_text IS NULL OR length(btrim(display_text)) > 0),

    CONSTRAINT object_references_kind_valid
        CHECK (reference_kind IN ('wikilink', 'kival_object_link')),

    CONSTRAINT object_references_span_valid
        CHECK (span_start >= 0 AND span_end > span_start),

    CONSTRAINT object_references_status_valid
        CHECK (status IN ('resolved', 'unresolved', 'ambiguous', 'stale')),

    CONSTRAINT object_references_resolution_complete
        CHECK (
            (status = 'resolved' AND target_object_id IS NOT NULL)
            OR status = 'stale'
            OR (status IN ('unresolved', 'ambiguous') AND target_object_id IS NULL)
        )
);

CREATE INDEX IF NOT EXISTS object_references_target_idx
    ON kival.object_references (workspace_id, target_object_id, created_at DESC)
    WHERE target_object_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS object_references_source_version_idx
    ON kival.object_references (source_object_id, source_version_id, span_start);

CREATE INDEX IF NOT EXISTS object_references_current_wikilink_target_idx
    ON kival.object_references (workspace_id, raw_target)
    WHERE reference_kind = 'wikilink'
      AND status <> 'stale';

-- ---------------------------------------------------------------------
-- Function: kival.clear_deleted_reference_target()
-- Purpose
--   Keep a derived object reference semantically valid when its target is cleared.
-- Trigger contract
--   BEFORE UPDATE OF `target_object_id` on `kival.object_references`.
-- Behavior
--   When a previously resolved target becomes NULL, changes a still-`resolved`
--   reference to `unresolved` and refreshes `updated_at`. Other target changes are
--   left untouched for the caller or reference-resolution pipeline to manage.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.clear_deleted_reference_target()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.target_object_id IS NOT NULL
       AND NEW.target_object_id IS NULL
       AND NEW.status = 'resolved' THEN
        NEW.status := 'unresolved';
        NEW.updated_at := now();
    END IF;

    RETURN NEW;
END;
$$;

COMMENT ON FUNCTION kival.clear_deleted_reference_target() IS
    'Marks a resolved derived object reference unresolved when its target_object_id is cleared.';

DROP TRIGGER IF EXISTS object_references_clear_deleted_target
ON kival.object_references;

CREATE TRIGGER object_references_clear_deleted_target
BEFORE UPDATE OF target_object_id ON kival.object_references
FOR EACH ROW
EXECUTE FUNCTION kival.clear_deleted_reference_target();
