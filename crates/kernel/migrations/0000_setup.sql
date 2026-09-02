-- =====================================================================
-- Kival migration 0000: setup
-- =====================================================================
-- Purpose
--   Establish the `kival` schema and reusable database primitives used by
--   later migrations to enforce common capability, lifecycle, and immutability rules.
--
-- Depends on
--   PostgreSQL only. This is the root Kival migration and must run first.
--
-- Owns
--   * The `kival` schema.
--   * Generic capability assertions.
--   * Generic update, lifecycle, archive, and immutability trigger helpers.
--
-- Design notes
--   These primitives deliberately enforce invariants in the database rather than
--   relying only on application code. Capability assertions are available to all
--   later migrations; trigger helpers should be used only when a table satisfies
--   the documented trigger contract.
-- =====================================================================

-- =====================================================================
-- Shared schema and database primitives
-- =====================================================================

CREATE SCHEMA IF NOT EXISTS kival;

-- ---------------------------------------------------------------------
-- Function: kival.require_capability()
-- Purpose
--   Preserve the distinction between a missing resource and a missing capability
--   for any database operation that requires both.
-- Contract
--   Call with the operation-specific resource-existence and authorization results.
-- Behavior
--   Raises stable Kival SQLSTATEs for missing resources or capabilities and
--   returns true when both conditions hold.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.require_capability(
    p_exists boolean,
    p_allowed boolean
)
RETURNS boolean
LANGUAGE plpgsql
STABLE
AS $$
BEGIN
    IF p_exists IS NOT TRUE THEN
        RAISE EXCEPTION USING
            ERRCODE = 'KRNFD',
            MESSAGE = 'resource not found';
    END IF;

    IF p_allowed IS NOT TRUE THEN
        RAISE EXCEPTION USING
            ERRCODE = 'KCAPR',
            MESSAGE = 'required capability not held';
    END IF;

    RETURN TRUE;
END;
$$;

COMMENT ON FUNCTION kival.require_capability(boolean, boolean) IS
    'Requires an existing resource and an allowed capability, raising stable Kival SQLSTATEs otherwise.';

-- ---------------------------------------------------------------------
-- Function: kival.before_update()
-- Purpose
--   Apply the standard mutable-row update policy.
-- Trigger contract
--   BEFORE UPDATE on a table containing `id`, `created_at`, and `updated_at`.
-- Behavior
--   Rejects changes to the row identity or creation timestamp and refreshes
--   `updated_at` for every accepted update. Other columns remain table-defined.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.before_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.id IS DISTINCT FROM OLD.id THEN
        RAISE EXCEPTION 'id is immutable';
    END IF;

    IF NEW.created_at IS DISTINCT FROM OLD.created_at THEN
        RAISE EXCEPTION 'created_at is immutable';
    END IF;

    NEW.updated_at = now();

    RETURN NEW;
END;
$$;

COMMENT ON FUNCTION kival.before_update() IS
    'Standard BEFORE UPDATE trigger: preserves id and created_at and refreshes updated_at.';

-- ---------------------------------------------------------------------
-- Function: kival.before_update_lifecycle_only()
-- Purpose
--   Restrict updates to the revocation fields of an active lifecycle row and make
--   the row immutable once revoked.
-- Trigger contract
--   BEFORE UPDATE on a table containing `updated_at`, `revoked_at`, and
--   `revoked_by`.
-- Behavior
--   Rejects all updates once the old row is revoked. Before revocation, only the
--   revocation fields may change; `updated_at` is maintained automatically.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.before_update_lifecycle_only()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.revoked_at IS NOT NULL THEN
        RAISE EXCEPTION 'revoked rows are immutable';
    END IF;

    NEW.updated_at = now();

    IF (
        to_jsonb(NEW)
        - 'updated_at'
        - 'revoked_at'
        - 'revoked_by'
    ) IS DISTINCT FROM (
        to_jsonb(OLD)
        - 'updated_at'
        - 'revoked_at'
        - 'revoked_by'
    ) THEN
        RAISE EXCEPTION 'only lifecycle revocation fields may be updated';
    END IF;

    RETURN NEW;
END;
$$;

COMMENT ON FUNCTION kival.before_update_lifecycle_only() IS
    'Lifecycle BEFORE UPDATE trigger: permits only revocation fields while active and makes revoked rows immutable.';

-- ---------------------------------------------------------------------
-- Function: kival.before_update_archive_only()
-- Purpose
--   Restrict a row to archive/unarchive lifecycle changes.
-- Trigger contract
--   BEFORE UPDATE on a table containing `updated_at`, `status`, `archived_at`,
--   and `archived_by`, with status values `active` and `archived`.
-- Behavior
--   Rejects changes to non-lifecycle data, refreshes `updated_at`, and enforces
--   complete archive metadata for archived rows and empty archive metadata for
--   active rows.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.before_update_archive_only()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    NEW.updated_at = now();

    IF (
        to_jsonb(NEW)
        - 'updated_at'
        - 'status'
        - 'archived_at'
        - 'archived_by'
    ) IS DISTINCT FROM (
        to_jsonb(OLD)
        - 'updated_at'
        - 'status'
        - 'archived_at'
        - 'archived_by'
    ) THEN
        RAISE EXCEPTION 'only archive lifecycle fields may be updated';
    END IF;

    IF NEW.status = 'active' THEN
        IF NEW.archived_at IS NOT NULL OR NEW.archived_by IS NOT NULL THEN
            RAISE EXCEPTION 'active rows must not have archive fields';
        END IF;
    ELSIF NEW.status = 'archived' THEN
        IF NEW.archived_at IS NULL OR NEW.archived_by IS NULL THEN
            RAISE EXCEPTION 'archived rows require archive fields';
        END IF;
    ELSE
        RAISE EXCEPTION 'invalid archive lifecycle status';
    END IF;

    RETURN NEW;
END;
$$;

COMMENT ON FUNCTION kival.before_update_archive_only() IS
    'Archive BEFORE UPDATE trigger: permits only archive lifecycle fields and enforces complete active/archived state.';

-- ---------------------------------------------------------------------
-- Function: kival.prevent_mutation()
-- Purpose
--   Make a table immutable after insertion.
-- Trigger contract
--   BEFORE UPDATE and/or BEFORE DELETE on any table.
-- Behavior
--   Always raises an exception naming the target table. The NULL return is
--   unreachable in successful execution but satisfies the trigger signature.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.prevent_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION '% is immutable', TG_TABLE_NAME;
    RETURN NULL;
END;
$$;

COMMENT ON FUNCTION kival.prevent_mutation() IS
    'Immutability trigger that rejects every UPDATE or DELETE on the target table.';
