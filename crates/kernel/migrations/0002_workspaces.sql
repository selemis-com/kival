-- =====================================================================
-- Kival migration 0002: workspaces
-- =====================================================================
-- Purpose
--   Define workspaces as Kival's primary organizational and security boundary,
--   together with direct user membership and the groups attached to a workspace.
--
-- Depends on
--   * 0000_setup.sql for shared trigger helpers.
--   * 0001_identity.sql for users and groups.
--
-- Owns
--   * `kival.workspaces`
--   * `kival.workspace_memberships`
--   * `kival.workspace_groups`
--
-- Design notes
--   Active workspace membership gates ordinary grant-based object access. Direct
--   memberships are revocable lifecycle records. Workspace/group links use an
--   archive lifecycle so a group can be detached and later reactivated by
--   unarchiving the existing link without deleting historical rows.
-- =====================================================================

-- =====================================================================
-- Workspaces
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.workspaces (
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

    CONSTRAINT workspaces_name_not_blank
        CHECK (length(btrim(name)) > 0),

    CONSTRAINT workspaces_description_not_blank_if_present
        CHECK (description IS NULL OR length(btrim(description)) > 0),

    CONSTRAINT workspaces_status_valid
        CHECK (status IN ('active', 'archived')),

    CONSTRAINT workspaces_archive_complete
        CHECK (
            (status = 'active' AND archived_at IS NULL AND archived_by IS NULL)
            OR
            (status = 'archived' AND archived_at IS NOT NULL AND archived_by IS NOT NULL)
        ),

    CONSTRAINT workspaces_archived_at_after_created_at
        CHECK (archived_at IS NULL OR archived_at >= created_at),

    CONSTRAINT workspaces_updated_at_after_created_at
        CHECK (updated_at >= created_at)
);

CREATE INDEX IF NOT EXISTS workspaces_status_idx
    ON kival.workspaces (status);

CREATE INDEX IF NOT EXISTS workspaces_created_by_idx
    ON kival.workspaces (created_by);

CREATE INDEX IF NOT EXISTS workspaces_archived_by_idx
    ON kival.workspaces (archived_by);

DROP TRIGGER IF EXISTS workspaces_before_update ON kival.workspaces;

CREATE TRIGGER workspaces_before_update
BEFORE UPDATE ON kival.workspaces
FOR EACH ROW
EXECUTE FUNCTION kival.before_update();

-- =====================================================================
-- Workspace memberships
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.workspace_memberships (
    id uuid PRIMARY KEY DEFAULT uuidv7(),

    workspace_id uuid NOT NULL REFERENCES kival.workspaces(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    user_id uuid NOT NULL REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    workspace_role text NOT NULL,

    created_by uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    revoked_by uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),

    revoked_at timestamptz,

    CONSTRAINT workspace_memberships_workspace_role_valid
        CHECK (workspace_role IN ('member', 'admin')),

    CONSTRAINT workspace_memberships_revocation_complete
        CHECK (
            (revoked_at IS NULL AND revoked_by IS NULL)
            OR
            (revoked_at IS NOT NULL AND revoked_by IS NOT NULL)
        ),

    CONSTRAINT workspace_memberships_revoked_at_after_created_at
        CHECK (revoked_at IS NULL OR revoked_at >= created_at),

    CONSTRAINT workspace_memberships_updated_at_after_created_at
        CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS workspace_memberships_one_active_membership_per_user_workspace
    ON kival.workspace_memberships (workspace_id, user_id)
    WHERE revoked_at IS NULL;

CREATE INDEX IF NOT EXISTS workspace_memberships_user_active_idx
    ON kival.workspace_memberships (user_id, workspace_id)
    WHERE revoked_at IS NULL;

CREATE INDEX IF NOT EXISTS workspace_memberships_created_by_idx
    ON kival.workspace_memberships (created_by);

CREATE INDEX IF NOT EXISTS workspace_memberships_revoked_by_idx
    ON kival.workspace_memberships (revoked_by);

DROP TRIGGER IF EXISTS workspace_memberships_before_update ON kival.workspace_memberships;

CREATE TRIGGER workspace_memberships_before_update
BEFORE UPDATE ON kival.workspace_memberships
FOR EACH ROW
EXECUTE FUNCTION kival.before_update_lifecycle_only();

-- =====================================================================
-- Workspace group links
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.workspace_groups (
    id uuid PRIMARY KEY DEFAULT uuidv7(),

    workspace_id uuid NOT NULL REFERENCES kival.workspaces(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    group_id uuid NOT NULL REFERENCES kival.groups(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

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

    CONSTRAINT workspace_groups_workspace_group_unique
        UNIQUE (workspace_id, group_id),

    CONSTRAINT workspace_groups_status_valid
        CHECK (status IN ('active', 'archived')),

    CONSTRAINT workspace_groups_archive_complete
        CHECK (
            (status = 'active' AND archived_at IS NULL AND archived_by IS NULL)
            OR
            (status = 'archived' AND archived_at IS NOT NULL AND archived_by IS NOT NULL)
        ),

    CONSTRAINT workspace_groups_archived_at_after_created_at
        CHECK (archived_at IS NULL OR archived_at >= created_at),

    CONSTRAINT workspace_groups_updated_at_after_created_at
        CHECK (updated_at >= created_at)
);

CREATE INDEX IF NOT EXISTS workspace_groups_workspace_active_idx
    ON kival.workspace_groups (workspace_id, group_id)
    WHERE archived_at IS NULL;

CREATE INDEX IF NOT EXISTS workspace_groups_group_active_idx
    ON kival.workspace_groups (group_id, workspace_id)
    WHERE archived_at IS NULL;

CREATE INDEX IF NOT EXISTS workspace_groups_created_by_idx
    ON kival.workspace_groups (created_by);

CREATE INDEX IF NOT EXISTS workspace_groups_archived_by_idx
    ON kival.workspace_groups (archived_by);

DROP TRIGGER IF EXISTS workspace_groups_before_update ON kival.workspace_groups;

CREATE TRIGGER workspace_groups_before_update
BEFORE UPDATE ON kival.workspace_groups
FOR EACH ROW
EXECUTE FUNCTION kival.before_update_archive_only();