-- =====================================================================
-- Kival migration 0001: identity
-- =====================================================================
-- Purpose
--   Define Kival's human identity and reusable group model: users, groups, and
--   revocable group memberships.
--
-- Depends on
--   0000_setup.sql for the `kival` schema and shared trigger helpers.
--
-- Owns
--   * `kival.users`
--   * `kival.groups`
--   * `kival.group_memberships`
--
-- Design notes
--   Users and groups are global identities. Workspace participation is modeled
--   separately in 0002_workspaces.sql. Membership rows are lifecycle records:
--   an active membership is revoked in place, after which the row is immutable.
--   Partial unique indexes ensure at most one active membership per user/group.
-- =====================================================================

-- =====================================================================
-- Users
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.users (
    id uuid PRIMARY KEY DEFAULT uuidv7(),

    username text NOT NULL,
    username_normalized text GENERATED ALWAYS AS (lower(username)) STORED,

    display_name text NOT NULL,

    status text NOT NULL DEFAULT 'active',

    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),

    disabled_at timestamptz,
    disabled_by uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    disabled_by_operator boolean NOT NULL DEFAULT false,

    CONSTRAINT users_username_length
        CHECK (char_length(username) BETWEEN 1 AND 30),

    CONSTRAINT users_username_format
        CHECK (username ~ '^[a-z0-9]([a-z0-9._-]*[a-z0-9])?$'),

    CONSTRAINT users_display_name_not_blank
        CHECK (length(btrim(display_name)) > 0),

    CONSTRAINT users_status_valid
        CHECK (status IN ('active', 'disabled')),

    CONSTRAINT users_disabled_complete
        CHECK (
            (
                status = 'active'
                AND disabled_at IS NULL
                AND disabled_by IS NULL
                AND NOT disabled_by_operator
            )
            OR
            (
                status = 'disabled'
                AND disabled_at IS NOT NULL
                AND (disabled_by IS NOT NULL) <> disabled_by_operator
            )
        ),

    CONSTRAINT users_disabled_at_after_created_at
        CHECK (disabled_at IS NULL OR disabled_at >= created_at),

    CONSTRAINT users_updated_at_after_created_at
        CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS users_username_normalized_unique
    ON kival.users (username_normalized);

COMMENT ON COLUMN kival.users.disabled_by_operator IS
    'True when the current disabled state was imposed by a deployment operator rather than a Kival user.';

DROP TRIGGER IF EXISTS users_before_update ON kival.users;

-- ---------------------------------------------------------------------
-- Function: kival.users_before_update()
-- Purpose
--   Apply the mutable-user update policy while preserving durable identity.
-- Trigger contract
--   BEFORE UPDATE on `kival.users`.
-- Behavior
--   Rejects changes to `id`, immutable `username`, or `created_at`, then refreshes
--   `updated_at`. Mutable profile and account-lifecycle fields remain table-defined.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.users_before_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.id IS DISTINCT FROM OLD.id THEN
        RAISE EXCEPTION 'user id is immutable';
    END IF;

    IF NEW.username IS DISTINCT FROM OLD.username THEN
        RAISE EXCEPTION 'username is immutable';
    END IF;

    IF NEW.created_at IS DISTINCT FROM OLD.created_at THEN
        RAISE EXCEPTION 'user created_at is immutable';
    END IF;

    NEW.updated_at = now();
    RETURN NEW;
END;
$$;

COMMENT ON FUNCTION kival.users_before_update() IS
    'User BEFORE UPDATE trigger: preserves id, username, and created_at and refreshes updated_at.';

CREATE TRIGGER users_before_update
BEFORE UPDATE ON kival.users
FOR EACH ROW
EXECUTE FUNCTION kival.users_before_update();

-- =====================================================================
-- Groups
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.groups (
    id uuid PRIMARY KEY DEFAULT uuidv7(),

    name text NOT NULL,
    description text,

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

    CONSTRAINT groups_name_not_blank
        CHECK (length(btrim(name)) > 0),

    CONSTRAINT groups_description_not_blank_if_present
        CHECK (description IS NULL OR length(btrim(description)) > 0),

    CONSTRAINT groups_status_valid
        CHECK (status IN ('active', 'archived')),

    CONSTRAINT groups_archive_complete
        CHECK (
            (status = 'active' AND archived_at IS NULL AND archived_by IS NULL)
            OR
            (status = 'archived' AND archived_at IS NOT NULL AND archived_by IS NOT NULL)
        ),

    CONSTRAINT groups_archived_at_after_created_at
        CHECK (archived_at IS NULL OR archived_at >= created_at),

    CONSTRAINT groups_updated_at_after_created_at
        CHECK (updated_at >= created_at)
);

CREATE INDEX IF NOT EXISTS groups_status_idx
    ON kival.groups (status);

CREATE INDEX IF NOT EXISTS groups_created_by_idx
    ON kival.groups (created_by);

CREATE INDEX IF NOT EXISTS groups_archived_by_idx
    ON kival.groups (archived_by);

DROP TRIGGER IF EXISTS groups_before_update ON kival.groups;

CREATE TRIGGER groups_before_update
BEFORE UPDATE ON kival.groups
FOR EACH ROW
EXECUTE FUNCTION kival.before_update();

-- =====================================================================
-- Group memberships
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.group_memberships (
    id uuid PRIMARY KEY DEFAULT uuidv7(),

    group_id uuid NOT NULL REFERENCES kival.groups(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    user_id uuid NOT NULL REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    group_role text NOT NULL,

    created_by uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    revoked_by uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),

    revoked_at timestamptz,

    CONSTRAINT group_memberships_group_role_valid
        CHECK (group_role IN ('member', 'admin')),

    CONSTRAINT group_memberships_revocation_complete
        CHECK (
            (revoked_at IS NULL AND revoked_by IS NULL)
            OR
            (revoked_at IS NOT NULL AND revoked_by IS NOT NULL)
        ),

    CONSTRAINT group_memberships_revoked_at_after_created_at
        CHECK (revoked_at IS NULL OR revoked_at >= created_at),

    CONSTRAINT group_memberships_updated_at_after_created_at
        CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS group_memberships_one_active_membership_per_user_group
    ON kival.group_memberships (group_id, user_id)
    WHERE revoked_at IS NULL;

CREATE INDEX IF NOT EXISTS group_memberships_user_active_idx
    ON kival.group_memberships (user_id, group_id)
    WHERE revoked_at IS NULL;

CREATE INDEX IF NOT EXISTS group_memberships_created_by_idx
    ON kival.group_memberships (created_by);

CREATE INDEX IF NOT EXISTS group_memberships_revoked_by_idx
    ON kival.group_memberships (revoked_by);

DROP TRIGGER IF EXISTS group_memberships_before_update ON kival.group_memberships;

CREATE TRIGGER group_memberships_before_update
BEFORE UPDATE ON kival.group_memberships
FOR EACH ROW
EXECUTE FUNCTION kival.before_update_lifecycle_only();
