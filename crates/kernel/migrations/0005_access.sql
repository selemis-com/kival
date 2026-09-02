-- =====================================================================
-- Kival migration 0005: access control
-- =====================================================================
-- Purpose
--   Define object-level roles, administrative authority, explicit object grants,
--   and the canonical database functions for resolving effective object access.
--
-- Depends on
--   * 0000_setup.sql for shared lifecycle trigger helpers.
--   * 0001_identity.sql for users, groups, and group memberships.
--   * 0002_workspaces.sql for workspace membership and workspace/group links.
--   * 0004_objects.sql for objects receiving grants.
--
-- Owns
--   * `kival.object_role`
--   * `kival.global_admins`
--   * `kival.object_grants`
--   * Effective object-role, permission-resolution, and resource-capability functions.
--
-- Design notes
--   Within active workspaces, global and workspace administrators receive implicit
--   object administration. Ordinary direct and group grants are effective only for
--   active workspace members. Group grants additionally require active group
--   membership, an active group, and an active link between that group and the
--   workspace.
-- =====================================================================

-- =====================================================================
-- Object role
-- =====================================================================

-- PostgreSQL does not support `CREATE DOMAIN IF NOT EXISTS`, so create the
-- role domain idempotently by checking the schema catalog first. The domain is
-- deliberately used in function signatures as well as table columns so invalid
-- role strings fail at the database boundary.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_type t
        JOIN pg_namespace n ON n.oid = t.typnamespace
        WHERE n.nspname = 'kival'
          AND t.typname = 'object_role'
    ) THEN
        CREATE DOMAIN kival.object_role AS text
            CHECK (VALUE IN ('viewer', 'editor', 'admin'));
    END IF;
END;
$$;

-- =====================================================================
-- Global admins
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.global_admins (
    id uuid PRIMARY KEY DEFAULT uuidv7(),

    user_id uuid NOT NULL REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    created_by uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    revoked_by uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),

    revoked_at timestamptz,

    CONSTRAINT global_admins_revocation_complete
        CHECK (
            (revoked_at IS NULL AND revoked_by IS NULL)
            OR
            (revoked_at IS NOT NULL AND revoked_by IS NOT NULL)
        ),

    CONSTRAINT global_admins_revoked_at_after_created_at
        CHECK (revoked_at IS NULL OR revoked_at >= created_at),

    CONSTRAINT global_admins_updated_at_after_created_at
        CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS global_admins_one_active_per_user
    ON kival.global_admins (user_id)
    WHERE revoked_at IS NULL;

CREATE INDEX IF NOT EXISTS global_admins_created_by_idx
    ON kival.global_admins (created_by);

CREATE INDEX IF NOT EXISTS global_admins_revoked_by_idx
    ON kival.global_admins (revoked_by);

DROP TRIGGER IF EXISTS global_admins_before_update ON kival.global_admins;

CREATE TRIGGER global_admins_before_update
BEFORE UPDATE ON kival.global_admins
FOR EACH ROW
EXECUTE FUNCTION kival.before_update_lifecycle_only();

-- =====================================================================
-- Object grants
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.object_grants (
    id uuid PRIMARY KEY DEFAULT uuidv7(),

    workspace_id uuid NOT NULL REFERENCES kival.workspaces(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    object_id uuid NOT NULL,

    principal_user_id uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    principal_group_id uuid,

    object_role kival.object_role NOT NULL,

    created_by uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    revoked_by uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),

    revoked_at timestamptz,

    CONSTRAINT object_grants_object_fk
        FOREIGN KEY (workspace_id, object_id)
        REFERENCES kival.objects (workspace_id, id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    CONSTRAINT object_grants_principal_group_workspace_fk
        FOREIGN KEY (workspace_id, principal_group_id)
        REFERENCES kival.workspace_groups (workspace_id, group_id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    CONSTRAINT object_grants_exactly_one_principal
        CHECK (
            (principal_user_id IS NOT NULL AND principal_group_id IS NULL)
            OR
            (principal_user_id IS NULL AND principal_group_id IS NOT NULL)
        ),

    CONSTRAINT object_grants_revocation_complete
        CHECK (
            (revoked_at IS NULL AND revoked_by IS NULL)
            OR
            (revoked_at IS NOT NULL AND revoked_by IS NOT NULL)
        ),

    CONSTRAINT object_grants_revoked_at_after_created_at
        CHECK (revoked_at IS NULL OR revoked_at >= created_at),

    CONSTRAINT object_grants_updated_at_after_created_at
        CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS object_grants_one_active_user_grant_per_object
    ON kival.object_grants (object_id, principal_user_id)
    WHERE revoked_at IS NULL
      AND principal_user_id IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS object_grants_one_active_group_grant_per_object
    ON kival.object_grants (object_id, principal_group_id)
    WHERE revoked_at IS NULL
      AND principal_group_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS object_grants_object_active_idx
    ON kival.object_grants (workspace_id, object_id, object_role)
    WHERE revoked_at IS NULL;

CREATE INDEX IF NOT EXISTS object_grants_user_active_idx
    ON kival.object_grants (principal_user_id, object_id, object_role)
    WHERE revoked_at IS NULL
      AND principal_user_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS object_grants_group_active_idx
    ON kival.object_grants (workspace_id, principal_group_id, object_id, object_role)
    WHERE revoked_at IS NULL
      AND principal_group_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS object_grants_created_by_idx
    ON kival.object_grants (created_by);

CREATE INDEX IF NOT EXISTS object_grants_revoked_by_idx
    ON kival.object_grants (revoked_by);

DROP TRIGGER IF EXISTS object_grants_before_update ON kival.object_grants;

CREATE TRIGGER object_grants_before_update
BEFORE UPDATE ON kival.object_grants
FOR EACH ROW
EXECUTE FUNCTION kival.before_update_lifecycle_only();

-- =====================================================================
-- Effective object access
-- =====================================================================

-- Returns the effective object role for a user, or NULL when no access applies.
--
-- Active global and workspace administrators receive admin implicitly. Ordinary
-- direct and group grants are effective only while the user is an active workspace
-- member. Group-derived grants additionally require an active membership, active
-- group, and active workspace-group link.
-- ---------------------------------------------------------------------
-- Function: kival.object_access_role(workspace_id, object_id, user_id)
-- Purpose
--   Resolve the highest effective object role granted to a user.
-- Parameters
--   p_workspace_id  Workspace in which access is being evaluated.
--   p_object_id     Object whose effective role should be resolved.
--   p_user_id       User whose access should be evaluated.
-- Returns
--   `viewer`, `editor`, or `admin`; NULL means no effective access.
-- Security semantics
--   Fails closed for a missing object, a workspace/object mismatch, or an archived
--   workspace. Active global and workspace administrators receive `admin`.
--   Everyone else must be an active workspace member before direct or group grants
--   can apply. Group grants additionally require active group membership, an
--   unarchived group, and an unarchived workspace/group link. Multiple grants are
--   collapsed to the strongest role: admin > editor > viewer.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.object_access_role(
    p_workspace_id uuid,
    p_object_id uuid,
    p_user_id uuid
)
RETURNS kival.object_role
LANGUAGE sql
STABLE
AS $$
    SELECT CASE
        -- Fail closed when the object does not exist, belongs to another
        -- workspace, or its workspace is archived.
        WHEN NOT EXISTS (
            SELECT 1
            FROM kival.objects o
            JOIN kival.workspaces w
              ON w.id = o.workspace_id
            WHERE o.workspace_id = p_workspace_id
              AND o.id = p_object_id
              AND w.archived_at IS NULL
        ) THEN NULL::kival.object_role

        WHEN EXISTS (
            SELECT 1
            FROM kival.global_admins ga
            WHERE ga.user_id = p_user_id
              AND ga.revoked_at IS NULL
        ) THEN 'admin'::kival.object_role

        WHEN EXISTS (
            SELECT 1
            FROM kival.workspace_memberships wm
            WHERE wm.workspace_id = p_workspace_id
              AND wm.user_id = p_user_id
              AND wm.workspace_role = 'admin'
              AND wm.revoked_at IS NULL
        ) THEN 'admin'::kival.object_role

        WHEN NOT EXISTS (
            SELECT 1
            FROM kival.workspace_memberships wm
            WHERE wm.workspace_id = p_workspace_id
              AND wm.user_id = p_user_id
              AND wm.revoked_at IS NULL
        ) THEN NULL::kival.object_role

        ELSE (
            SELECT CASE
                WHEN BOOL_OR(og.object_role = 'admin')
                    THEN 'admin'::kival.object_role
                WHEN BOOL_OR(og.object_role = 'editor')
                    THEN 'editor'::kival.object_role
                WHEN BOOL_OR(og.object_role = 'viewer')
                    THEN 'viewer'::kival.object_role
                ELSE NULL::kival.object_role
            END
            FROM kival.object_grants og
            LEFT JOIN kival.group_memberships gm
              ON gm.group_id = og.principal_group_id
             AND gm.user_id = p_user_id
             AND gm.revoked_at IS NULL
            LEFT JOIN kival.groups g
              ON g.id = og.principal_group_id
             AND g.archived_at IS NULL
            LEFT JOIN kival.workspace_groups wg
              ON wg.workspace_id = og.workspace_id
             AND wg.group_id = og.principal_group_id
             AND wg.archived_at IS NULL
            WHERE og.workspace_id = p_workspace_id
              AND og.object_id = p_object_id
              AND og.revoked_at IS NULL
              AND (
                  og.principal_user_id = p_user_id
                  OR (
                      og.principal_group_id IS NOT NULL
                      AND gm.id IS NOT NULL
                      AND g.id IS NOT NULL
                      AND wg.id IS NOT NULL
                  )
              )
        )
    END
$$;

COMMENT ON FUNCTION kival.object_access_role(uuid, uuid, uuid) IS
    'Returns a user''s highest effective object role, applying admin overrides, workspace membership, and active direct/group grants.';

-- Returns whether the user's effective object role satisfies the required role.
-- ---------------------------------------------------------------------
-- Function: kival.has_object_permission(workspace_id, object_id, user_id, role)
-- Purpose
--   Convert the effective object role into a boolean authorization decision.
-- Parameters
--   p_workspace_id  Workspace in which access is being evaluated.
--   p_object_id     Object being authorized.
--   p_user_id       User being authorized.
--   p_required_role Minimum role required by the operation.
-- Returns
--   TRUE when the effective role meets or exceeds the required role; otherwise
--   FALSE, including when no effective role exists.
-- Role hierarchy
--   admin >= editor >= viewer.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.has_object_permission(
    p_workspace_id uuid,
    p_object_id uuid,
    p_user_id uuid,
    p_required_role kival.object_role
)
RETURNS boolean
LANGUAGE sql
STABLE
AS $$
    WITH access AS (
        SELECT kival.object_access_role(
            p_workspace_id,
            p_object_id,
            p_user_id
        ) AS effective_role
    )
    SELECT COALESCE(
        CASE p_required_role
            WHEN 'viewer' THEN
                effective_role IS NOT NULL

            WHEN 'editor' THEN
                effective_role IN (
                    'editor'::kival.object_role,
                    'admin'::kival.object_role
                )

            WHEN 'admin' THEN
                effective_role = 'admin'::kival.object_role
        END,
        FALSE
    )
    FROM access
$$;

COMMENT ON FUNCTION kival.has_object_permission(uuid, uuid, uuid, kival.object_role) IS
    'Returns whether a user''s effective object role satisfies the required viewer/editor/admin role.';


-- =====================================================================
-- Read authorization predicates
-- =====================================================================
-- These helpers extend the canonical access policy above so protected state reads
-- evaluate current authorization in the same SQL statement that returns data.

-- ---------------------------------------------------------------------
-- Function: kival.user_can_read_workspace(workspace_id, user_id)
-- Purpose
--   Evaluate whether a user may currently read an active workspace.
-- Parameters
--   p_workspace_id  Workspace being authorized.
--   p_user_id       User being authorized.
-- Returns
--   TRUE for an active global administrator or active member of the active
--   workspace; FALSE for missing, archived, or unauthorized workspaces.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.user_can_read_workspace(
    p_workspace_id uuid,
    p_user_id uuid
)
RETURNS boolean
LANGUAGE sql
STABLE
AS $$
    SELECT EXISTS (
        SELECT 1
        FROM kival.workspaces w
        WHERE w.id = p_workspace_id
          AND w.archived_at IS NULL
          AND (
              EXISTS (
                  SELECT 1
                  FROM kival.global_admins ga
                  WHERE ga.user_id = p_user_id
                    AND ga.revoked_at IS NULL
              )
              OR EXISTS (
                  SELECT 1
                  FROM kival.workspace_memberships wm
                  WHERE wm.workspace_id = p_workspace_id
                    AND wm.user_id = p_user_id
                    AND wm.revoked_at IS NULL
              )
          )
    )
$$;

COMMENT ON FUNCTION kival.user_can_read_workspace(uuid, uuid) IS
    'Returns whether a user may currently read an active workspace.';

-- ---------------------------------------------------------------------
-- Function: kival.user_can_read_group(group_id, user_id)
-- Purpose
--   Evaluate whether a user may read group metadata.
-- Parameters
--   p_group_id  Group being authorized.
--   p_user_id   User being authorized.
-- Returns
--   TRUE for an active global administrator or active administrator membership
--   in the group. Group metadata remains readable across the group's lifecycle.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.user_can_read_group(
    p_group_id uuid,
    p_user_id uuid
)
RETURNS boolean
LANGUAGE sql
STABLE
AS $$
    SELECT EXISTS (
        SELECT 1
        FROM kival.groups g
        WHERE g.id = p_group_id
          AND (
              EXISTS (
                  SELECT 1
                  FROM kival.global_admins ga
                  WHERE ga.user_id = p_user_id
                    AND ga.revoked_at IS NULL
              )
              OR EXISTS (
                  SELECT 1
                  FROM kival.group_memberships gm
                  WHERE gm.group_id = p_group_id
                    AND gm.user_id = p_user_id
                    AND gm.group_role = 'admin'
                    AND gm.revoked_at IS NULL
              )
          )
    )
$$;

COMMENT ON FUNCTION kival.user_can_read_group(uuid, uuid) IS
    'Returns whether a user may currently read group metadata in any lifecycle state.';

-- ---------------------------------------------------------------------
-- Function: kival.user_can_read_object(workspace_id, object_id, user_id)
-- Purpose
--   Evaluate current read access to an object while preserving administrative
--   visibility of archived objects.
-- Parameters
--   p_workspace_id  Workspace expected to own the object.
--   p_object_id     Object being authorized.
--   p_user_id       User being authorized.
-- Returns
--   TRUE for ordinary viewer-or-stronger access to an active object, or for
--   administrative access to an archived object; FALSE otherwise.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.user_can_read_object(
    p_workspace_id uuid,
    p_object_id uuid,
    p_user_id uuid
)
RETURNS boolean
LANGUAGE sql
STABLE
AS $$
    SELECT COALESCE((
        SELECT CASE
            WHEN o.archived_at IS NULL THEN
                kival.has_object_permission(
                    p_workspace_id,
                    p_object_id,
                    p_user_id,
                    'viewer'::kival.object_role
                )
            ELSE
                kival.has_object_permission(
                    p_workspace_id,
                    p_object_id,
                    p_user_id,
                    'admin'::kival.object_role
                )
        END
        FROM kival.objects o
        WHERE o.workspace_id = p_workspace_id
          AND o.id = p_object_id
    ), FALSE)
$$;

COMMENT ON FUNCTION kival.user_can_read_object(uuid, uuid, uuid) IS
    'Returns whether a user may currently read an active object or administratively read an archived object.';

-- ---------------------------------------------------------------------
-- Function: kival.user_can_access_active_object(workspace_id, object_id, user_id, required_role)
-- Purpose
--   Evaluate a minimum effective role against an active object.
-- Parameters
--   p_workspace_id  Workspace expected to own the object.
--   p_object_id     Object being authorized.
--   p_user_id       User being authorized.
--   p_required_role Minimum viewer/editor/admin role required by the operation.
-- Returns
--   TRUE only when the object is active and `has_object_permission` satisfies
--   the requested role.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.user_can_access_active_object(
    p_workspace_id uuid,
    p_object_id uuid,
    p_user_id uuid,
    p_required_role kival.object_role
)
RETURNS boolean
LANGUAGE sql
STABLE
AS $$
    SELECT EXISTS (
        SELECT 1
        FROM kival.objects o
        JOIN kival.workspaces w
          ON w.id = o.workspace_id
        WHERE o.workspace_id = p_workspace_id
          AND o.id = p_object_id
          AND o.archived_at IS NULL
          AND w.archived_at IS NULL
          AND kival.has_object_permission(
              p_workspace_id,
              p_object_id,
              p_user_id,
              p_required_role
          )
    )
$$;

COMMENT ON FUNCTION kival.user_can_access_active_object(uuid, uuid, uuid, kival.object_role) IS
    'Returns whether a user currently satisfies a minimum role on an active object.';

-- ---------------------------------------------------------------------
-- Function: kival.user_can_admin_active_group(group_id, user_id)
-- Purpose
--   Evaluate current administrative access to an active group.
-- Parameters
--   p_group_id  Group being authorized.
--   p_user_id   User being authorized.
-- Returns
--   TRUE for an active global administrator or active group administrator when
--   the group itself is active; FALSE otherwise.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.user_can_admin_active_group(
    p_group_id uuid,
    p_user_id uuid
)
RETURNS boolean
LANGUAGE sql
STABLE
AS $$
    SELECT EXISTS (
        SELECT 1
        FROM kival.groups g
        WHERE g.id = p_group_id
          AND g.archived_at IS NULL
          AND (
              EXISTS (
                  SELECT 1
                  FROM kival.global_admins ga
                  WHERE ga.user_id = p_user_id
                    AND ga.revoked_at IS NULL
              )
              OR EXISTS (
                  SELECT 1
                  FROM kival.group_memberships gm
                  WHERE gm.group_id = p_group_id
                    AND gm.user_id = p_user_id
                    AND gm.group_role = 'admin'
                    AND gm.revoked_at IS NULL
              )
          )
    )
$$;

COMMENT ON FUNCTION kival.user_can_admin_active_group(uuid, uuid) IS
    'Returns whether a user may currently administer an active group.';

-- =====================================================================
-- Resource capability assertions
-- =====================================================================
-- ---------------------------------------------------------------------
-- Function: kival.require_read_workspace(workspace_id, user_id)
-- Purpose
--   Assert current read capability for an active workspace.
-- Parameters
--   p_workspace_id  Workspace being resolved and authorized.
--   p_user_id       User being authorized.
-- Returns
--   TRUE when the workspace exists and is readable.
-- Error semantics
--   Delegates to `kival.require_capability` so a missing/inactive workspace is
--   distinguishable from an existing workspace without the required capability.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.require_read_workspace(
    p_workspace_id uuid,
    p_user_id uuid
)
RETURNS boolean
LANGUAGE sql
STABLE
AS $$
    SELECT kival.require_capability(
        EXISTS (
            SELECT 1
            FROM kival.workspaces w
            WHERE w.id = p_workspace_id
              AND w.archived_at IS NULL
        ),
        kival.user_can_read_workspace(p_workspace_id, p_user_id)
    )
$$;

COMMENT ON FUNCTION kival.require_read_workspace(uuid, uuid) IS
    'Requires current read access to an active workspace while preserving missing-resource-versus-missing-capability semantics.';

-- ---------------------------------------------------------------------
-- Function: kival.require_read_group(group_id, user_id)
-- Purpose
--   Assert current metadata-read capability for a group.
-- Parameters
--   p_group_id  Group being resolved and authorized.
--   p_user_id   User being authorized.
-- Returns
--   TRUE when the group exists and is readable.
-- Error semantics
--   Delegates to `kival.require_capability` to preserve the distinction between
--   a missing group and a missing read capability.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.require_read_group(
    p_group_id uuid,
    p_user_id uuid
)
RETURNS boolean
LANGUAGE sql
STABLE
AS $$
    SELECT kival.require_capability(
        EXISTS (
            SELECT 1
            FROM kival.groups g
            WHERE g.id = p_group_id
        ),
        kival.user_can_read_group(p_group_id, p_user_id)
    )
$$;

COMMENT ON FUNCTION kival.require_read_group(uuid, uuid) IS
    'Requires current group metadata access while preserving missing-resource-versus-missing-capability semantics.';

-- ---------------------------------------------------------------------
-- Function: kival.require_read_object(workspace_id, object_id, user_id)
-- Purpose
--   Assert current read capability for an object in an active workspace.
-- Parameters
--   p_workspace_id  Workspace expected to own the object.
--   p_object_id     Object being resolved and authorized.
--   p_user_id       User being authorized.
-- Returns
--   TRUE when the scoped object exists and is currently readable.
-- Error semantics
--   Delegates to `kival.require_capability` so missing scoped state and missing
--   authorization remain distinct outcomes.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.require_read_object(
    p_workspace_id uuid,
    p_object_id uuid,
    p_user_id uuid
)
RETURNS boolean
LANGUAGE sql
STABLE
AS $$
    SELECT kival.require_capability(
        EXISTS (
            SELECT 1
            FROM kival.objects o
            JOIN kival.workspaces w
              ON w.id = o.workspace_id
            WHERE o.workspace_id = p_workspace_id
              AND o.id = p_object_id
              AND w.archived_at IS NULL
        ),
        kival.user_can_read_object(p_workspace_id, p_object_id, p_user_id)
    )
$$;

COMMENT ON FUNCTION kival.require_read_object(uuid, uuid, uuid) IS
    'Requires current object readability while preserving missing-resource-versus-missing-capability semantics.';

-- ---------------------------------------------------------------------
-- Function: kival.require_access_active_object(workspace_id, object_id, user_id, required_role)
-- Purpose
--   Assert a minimum role on an active object in an active workspace.
-- Parameters
--   p_workspace_id  Workspace expected to own the object.
--   p_object_id     Object being resolved and authorized.
--   p_user_id       User being authorized.
--   p_required_role Minimum viewer/editor/admin role required by the operation.
-- Returns
--   TRUE when the active object exists and the required role is held.
-- Error semantics
--   Delegates to `kival.require_capability` to preserve resource-versus-capability
--   failure semantics.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.require_access_active_object(
    p_workspace_id uuid,
    p_object_id uuid,
    p_user_id uuid,
    p_required_role kival.object_role
)
RETURNS boolean
LANGUAGE sql
STABLE
AS $$
    SELECT kival.require_capability(
        EXISTS (
            SELECT 1
            FROM kival.objects o
            JOIN kival.workspaces w
              ON w.id = o.workspace_id
            WHERE o.workspace_id = p_workspace_id
              AND o.id = p_object_id
              AND o.archived_at IS NULL
              AND w.archived_at IS NULL
        ),
        kival.user_can_access_active_object(
            p_workspace_id,
            p_object_id,
            p_user_id,
            p_required_role
        )
    )
$$;

COMMENT ON FUNCTION kival.require_access_active_object(uuid, uuid, uuid, kival.object_role) IS
    'Requires a minimum role on an active object while preserving missing-resource-versus-missing-capability semantics.';

-- ---------------------------------------------------------------------
-- Function: kival.require_admin_active_group(group_id, user_id)
-- Purpose
--   Assert administrative capability for an active group.
-- Parameters
--   p_group_id  Group being resolved and authorized.
--   p_user_id   User being authorized.
-- Returns
--   TRUE when the group is active and the user may administer it.
-- Error semantics
--   Delegates to `kival.require_capability` so missing/inactive groups remain
--   distinguishable from missing administrative capability.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.require_admin_active_group(
    p_group_id uuid,
    p_user_id uuid
)
RETURNS boolean
LANGUAGE sql
STABLE
AS $$
    SELECT kival.require_capability(
        EXISTS (
            SELECT 1
            FROM kival.groups g
            WHERE g.id = p_group_id
              AND g.archived_at IS NULL
        ),
        kival.user_can_admin_active_group(p_group_id, p_user_id)
    )
$$;

COMMENT ON FUNCTION kival.require_admin_active_group(uuid, uuid) IS
    'Requires administration of an active group while preserving missing-resource-versus-missing-capability semantics.';
