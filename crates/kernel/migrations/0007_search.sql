-- =====================================================================
-- Kival migration 0007: search
-- =====================================================================
-- Purpose
--   Build and maintain the denormalized search projection for immutable object
--   versions.
--
-- Depends on
--   * 0002_workspaces.sql for workspace identity and scoping.
--   * 0004_objects.sql for objects and immutable versions.
--
-- Owns
--   * `kival.search_documents`
--   * Search-vector construction and per-version indexing functions.
--   * Triggers that keep the search projection synchronized.
--   * The migration-time full rebuild of the search projection.
--
-- Design notes
--   `search_documents` is derived state and can be rebuilt from authoritative
--   immutable object versions. Every indexed row belongs to one version. Normal
--   search filters these rows through `objects.current_version_id`; historical
--   search can query the same projection without that filter.
-- =====================================================================

-- =====================================================================
-- Search projection
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.search_documents (
    workspace_id uuid NOT NULL,

    category text NOT NULL,
    text text NOT NULL,
    search_vector tsvector NOT NULL,

    object_id uuid NOT NULL,
    version_id uuid NOT NULL,

    CONSTRAINT search_documents_pkey
        PRIMARY KEY (version_id, category),

    CONSTRAINT search_documents_object_fk
        FOREIGN KEY (workspace_id, object_id)
        REFERENCES kival.objects (workspace_id, id)
        ON UPDATE RESTRICT
        ON DELETE CASCADE,

    CONSTRAINT search_documents_version_fk
        FOREIGN KEY (object_id, version_id)
        REFERENCES kival.object_versions (object_id, id)
        ON UPDATE RESTRICT
        ON DELETE CASCADE,

    CONSTRAINT search_documents_category_valid
        CHECK (category IN ('title', 'body', 'metadata'))
);

CREATE INDEX IF NOT EXISTS search_documents_workspace_category_idx
    ON kival.search_documents (workspace_id, category);

CREATE INDEX IF NOT EXISTS search_documents_object_version_idx
    ON kival.search_documents (object_id, version_id);

CREATE INDEX IF NOT EXISTS search_documents_search_vector_idx
    ON kival.search_documents USING GIN (search_vector);

-- =====================================================================
-- Search projection maintenance
-- =====================================================================

-- ---------------------------------------------------------------------
-- Function: kival.search_document_vector(value)
-- Purpose
--   Normalize nullable text into the tsvector representation used by Kival search.
-- Parameters
--   value  Source text; NULL is treated as the empty string.
-- Returns
--   A `simple`-configuration tsvector, intentionally avoiding language-specific
--   stemming so identifiers, titles, and technical content remain predictable.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.search_document_vector(value text)
RETURNS tsvector
LANGUAGE sql
IMMUTABLE
AS $$
    SELECT to_tsvector('simple', COALESCE(value, ''));
$$;

COMMENT ON FUNCTION kival.search_document_vector(text) IS
    'Converts nullable text to a simple-configuration tsvector used by Kival search documents.';

-- ---------------------------------------------------------------------
-- Function: kival.reindex_object_version_search_documents(version_id)
-- Purpose
--   Rebuild every search-document row derived from one immutable object version.
-- Parameters
--   version_id_arg  Version whose search projection should be rebuilt.
-- Behavior
--   Serializes concurrent rebuilds for the same version with a transaction-scoped
--   advisory lock, deletes the prior projection, and recreates title, body, and
--   metadata rows from the authoritative version. If the version no longer exists,
--   only stale projection rows are removed.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.reindex_object_version_search_documents(version_id_arg uuid)
RETURNS void
LANGUAGE plpgsql
AS $$
DECLARE
    version_row RECORD;
BEGIN
    PERFORM pg_advisory_xact_lock(hashtextextended(version_id_arg::text, 0));

    DELETE FROM kival.search_documents WHERE version_id = version_id_arg;

    SELECT
        object.workspace_id,
        version.object_id,
        version.id,
        version.title,
        version.body_text,
        version.metadata
    INTO version_row
    FROM kival.object_versions version
    JOIN kival.objects object
      ON object.id = version.object_id
    WHERE version.id = version_id_arg;

    IF NOT FOUND THEN
        RETURN;
    END IF;

    INSERT INTO kival.search_documents (
        workspace_id,
        category,
        text,
        search_vector,
        object_id,
        version_id
    )
    VALUES
        (
            version_row.workspace_id,
            'title',
            version_row.title,
            kival.search_document_vector(version_row.title),
            version_row.object_id,
            version_row.id
        ),
        (
            version_row.workspace_id,
            'body',
            version_row.body_text,
            kival.search_document_vector(version_row.body_text),
            version_row.object_id,
            version_row.id
        ),
        (
            version_row.workspace_id,
            'metadata',
            version_row.metadata::text,
            kival.search_document_vector(version_row.metadata::text),
            version_row.object_id,
            version_row.id
        );
END;
$$;

COMMENT ON FUNCTION kival.reindex_object_version_search_documents(uuid) IS
    'Rebuilds all search projection rows derived from one immutable object version.';

-- ---------------------------------------------------------------------
-- Function: kival.search_documents_after_version_insert()
-- Purpose
--   Synchronize search state after an immutable object version is created.
-- Trigger contract
--   AFTER INSERT on `kival.object_versions`.
-- Behavior
--   Indexes the newly created version. No update/delete trigger is needed because
--   object versions are immutable.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.search_documents_after_version_insert()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM kival.reindex_object_version_search_documents(NEW.id);
    RETURN NEW;
END;
$$;

COMMENT ON FUNCTION kival.search_documents_after_version_insert() IS
    'Search-maintenance trigger that indexes a newly created immutable object version.';

DROP TRIGGER IF EXISTS search_documents_after_version_insert ON kival.object_versions;
CREATE TRIGGER search_documents_after_version_insert
AFTER INSERT ON kival.object_versions
FOR EACH ROW
EXECUTE FUNCTION kival.search_documents_after_version_insert();

-- Rebuild the complete derived projection after installing the maintenance
-- trigger. TRUNCATE is safe here because `search_documents` contains no
-- authoritative state; every row below is regenerated from object versions.
TRUNCATE kival.search_documents;

DO $$
DECLARE
    version_id_to_index uuid;
BEGIN
    FOR version_id_to_index IN
        SELECT id FROM kival.object_versions
    LOOP
        PERFORM kival.reindex_object_version_search_documents(version_id_to_index);
    END LOOP;
END;
$$;
