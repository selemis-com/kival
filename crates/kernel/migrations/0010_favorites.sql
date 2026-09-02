-- =====================================================================
-- Kival migration 0010: personal favorites and pins
-- =====================================================================
-- Purpose
--   Add user-owned workspace pins, object favorites, and object pins as lightweight
--   personal organization metadata.
--
-- Depends on
--   * 0001_identity.sql for users.
--   * 0002_workspaces.sql for workspace identity and scoping.
--   * 0004_objects.sql for workspace-scoped object targets.
--
-- Owns
--   * `kival.workspace_pins`
--   * `kival.object_favorites`
--   * `kival.object_pins`
--
-- Design notes
--   Favorites and pins are retained independently of current access. API queries
--   always apply normal visibility rules, so inaccessible markers remain hidden
--   and become visible again if access is restored.
-- =====================================================================

-- =====================================================================
-- Workspace pins
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.workspace_pins (
    user_id uuid NOT NULL REFERENCES kival.users(id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    workspace_id uuid NOT NULL REFERENCES kival.workspaces(id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, workspace_id)
);

CREATE INDEX IF NOT EXISTS workspace_pins_workspace_idx
    ON kival.workspace_pins (workspace_id, user_id);

-- =====================================================================
-- Object favorites
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.object_favorites (
    user_id uuid NOT NULL REFERENCES kival.users(id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    workspace_id uuid NOT NULL REFERENCES kival.workspaces(id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    object_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, object_id),
    CONSTRAINT object_favorites_object_fk
        FOREIGN KEY (workspace_id, object_id)
        REFERENCES kival.objects (workspace_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS object_favorites_object_idx
    ON kival.object_favorites (workspace_id, object_id, user_id);

-- =====================================================================
-- Object pins
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.object_pins (
    user_id uuid NOT NULL REFERENCES kival.users(id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    workspace_id uuid NOT NULL REFERENCES kival.workspaces(id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    object_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, object_id),
    CONSTRAINT object_pins_object_fk
        FOREIGN KEY (workspace_id, object_id)
        REFERENCES kival.objects (workspace_id, id)
        ON UPDATE RESTRICT ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS object_pins_object_idx
    ON kival.object_pins (workspace_id, object_id, user_id);
