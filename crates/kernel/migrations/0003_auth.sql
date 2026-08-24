-- =====================================================================
-- Kival migration 0003: authentication
-- =====================================================================
-- Purpose
--   Define Kival authentication state: interactive passkeys, WebAuthn
--   ceremonies, administrator-authorized enrollment and recovery, browser
--   sessions, API keys, mutable API-key scopes, and explicit workspace grants
--   for machine credentials.
--
-- Depends on
--   * 0000_setup.sql for the `kival` schema and shared trigger helpers.
--   * 0001_identity.sql for immutable user IDs and account lookup.
--   * 0002_workspaces.sql for API-key workspace allow-lists.
--
-- Owns
--   * `kival.passkey_credentials`
--   * `kival.sessions`
--   * `kival.passkey_enrollment_codes`
--   * `kival.webauthn_ceremonies`
--   * `kival.api_keys`
--   * `kival.api_key_scopes`
--   * `kival.api_key_workspaces`
--   * Authentication credential/session lifecycle and immutability triggers.
--
-- Design notes
--   Kival's only interactive user credential is a deliberately narrow
--   WebAuthn/passkey profile. The database stores globally unique credential
--   IDs and normalized uncompressed P-256 public keys, never private key
--   material or recoverable authentication secrets.
--
--   WebAuthn challenges are short-lived, single-use, and bound to an immutable
--   user ID plus a browser session or enrollment capability where required.
--   Username is an account lookup identifier and is not itself an
--   authentication factor.
--
--   Passkeys are optional credentials, not recovery credentials. Administrative
--   reset revokes passkeys and browser sessions; API keys remain an independent,
--   workspace-scoped credential class. Raw session and API-key tokens are never
--   stored; only their hashes belong in this schema.
--
--   API-key token identity is immutable, while scopes and explicit workspace
--   grants are mutable authorization state. The application changes that state
--   only through a fresh passkey-authenticated browser session and replaces it
--   atomically. `authorization_revision` provides optimistic concurrency for
--   those edits without coupling authorization changes to token rotation.
-- =====================================================================

-- =====================================================================
-- Passkey credentials
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.passkey_credentials (
    id uuid PRIMARY KEY DEFAULT uuidv7(),

    user_id uuid NOT NULL REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    credential_id bytea NOT NULL,
    public_key bytea NOT NULL,
    label text,
    signature_count bigint NOT NULL DEFAULT 0,

    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    last_used_at timestamptz,

    revoked_at timestamptz,
    revoked_by uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    revoked_by_operator boolean NOT NULL DEFAULT false,
    revocation_reason text,

    CONSTRAINT passkeys_credential_id_not_empty
        CHECK (octet_length(credential_id) > 0),

    CONSTRAINT passkeys_public_key_p256_length
        CHECK (octet_length(public_key) = 65 AND get_byte(public_key, 0) = 4),

    CONSTRAINT passkeys_label_not_blank
        CHECK (label IS NULL OR length(btrim(label)) > 0),

    CONSTRAINT passkeys_label_length
        CHECK (label IS NULL OR char_length(label) <= 64),

    CONSTRAINT passkeys_signature_count_u32
        CHECK (signature_count >= 0 AND signature_count <= 4294967295),

    CONSTRAINT passkeys_revocation_complete
        CHECK (
            (
                revoked_at IS NULL AND revoked_by IS NULL
                AND NOT revoked_by_operator AND revocation_reason IS NULL
            )
            OR
            (
                revoked_at IS NOT NULL AND (revoked_by IS NOT NULL) <> revoked_by_operator
                AND revocation_reason IS NOT NULL
            )
        ),

    CONSTRAINT passkeys_revocation_reason_not_blank
        CHECK (revocation_reason IS NULL OR length(btrim(revocation_reason)) > 0),

    CONSTRAINT passkeys_last_used_after_creation
        CHECK (last_used_at IS NULL OR last_used_at >= created_at),

    CONSTRAINT passkeys_revoked_after_creation
        CHECK (revoked_at IS NULL OR revoked_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS passkeys_credential_id_unique
    ON kival.passkey_credentials (credential_id);

CREATE INDEX IF NOT EXISTS passkeys_user_active_idx
    ON kival.passkey_credentials (user_id, created_at DESC)
    WHERE revoked_at IS NULL;

-- ---------------------------------------------------------------------
-- Function: kival.passkey_credentials_before_update()
-- Purpose
--   Enforce passkey credential immutability and lifecycle state.
-- Trigger contract
--   BEFORE UPDATE on `kival.passkey_credentials`.
-- Behavior
--   Keeps credential identity and key material immutable. A revoked credential
--   is immutable. Mutable lifecycle fields are validated by table constraints,
--   and `updated_at` is maintained automatically.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.passkey_credentials_before_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.revoked_at IS NOT NULL THEN
        RAISE EXCEPTION 'revoked passkey credentials are immutable';
    END IF;

    IF NEW.id IS DISTINCT FROM OLD.id
       OR NEW.user_id IS DISTINCT FROM OLD.user_id
       OR NEW.credential_id IS DISTINCT FROM OLD.credential_id
       OR NEW.public_key IS DISTINCT FROM OLD.public_key
       OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN
        RAISE EXCEPTION 'passkey credential identity and key material are immutable';
    END IF;

    NEW.updated_at = now();
    RETURN NEW;
END;
$$;

COMMENT ON FUNCTION kival.passkey_credentials_before_update() IS
    'Keeps passkey credential identity and key material immutable and makes revoked credentials immutable.';

DROP TRIGGER IF EXISTS passkey_credentials_before_update ON kival.passkey_credentials;

CREATE TRIGGER passkey_credentials_before_update
BEFORE UPDATE ON kival.passkey_credentials
FOR EACH ROW
EXECUTE FUNCTION kival.passkey_credentials_before_update();

COMMENT ON TABLE kival.passkey_credentials IS
    'Public WebAuthn credential material and lifecycle metadata; private keys never enter Kival.';

-- =====================================================================
-- Sessions
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.sessions (
    id uuid PRIMARY KEY DEFAULT uuidv7(),

    user_id uuid NOT NULL REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    session_token_hash bytea NOT NULL,
    csrf_token_hash bytea NOT NULL,

    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),

    expires_at timestamptz NOT NULL,
    fresh_authenticated_at timestamptz,

    revoked_at timestamptz,
    revoked_by uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    revoked_by_operator boolean NOT NULL DEFAULT false,
    revocation_reason text,

    last_seen_at timestamptz,

    user_agent text,
    ip_address inet,

    CONSTRAINT sessions_session_token_hash_sha256_length
        CHECK (octet_length(session_token_hash) = 32),

    CONSTRAINT sessions_csrf_token_hash_sha256_length
        CHECK (octet_length(csrf_token_hash) = 32),

    CONSTRAINT sessions_expires_at_after_created_at
        CHECK (expires_at > created_at),

    CONSTRAINT sessions_fresh_authenticated_after_creation
        CHECK (fresh_authenticated_at IS NULL OR fresh_authenticated_at >= created_at),

    CONSTRAINT sessions_revocation_complete
        CHECK (
            (
                revoked_at IS NULL AND revoked_by IS NULL
                AND NOT revoked_by_operator AND revocation_reason IS NULL
            )
            OR
            (
                revoked_at IS NOT NULL AND (revoked_by IS NOT NULL) <> revoked_by_operator
                AND revocation_reason IS NOT NULL
            )
        ),

    CONSTRAINT sessions_revocation_reason_not_blank_if_present
        CHECK (revocation_reason IS NULL OR length(btrim(revocation_reason)) > 0),

    CONSTRAINT sessions_last_seen_at_after_created_at
        CHECK (last_seen_at IS NULL OR last_seen_at >= created_at),

    CONSTRAINT sessions_revoked_at_after_created_at
        CHECK (revoked_at IS NULL OR revoked_at >= created_at),

    CONSTRAINT sessions_updated_at_after_created_at
        CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS sessions_session_token_hash_unique
    ON kival.sessions (session_token_hash);

CREATE INDEX IF NOT EXISTS sessions_user_active_idx
    ON kival.sessions (user_id, expires_at)
    WHERE revoked_at IS NULL;

CREATE INDEX IF NOT EXISTS sessions_terminal_retention_idx
    ON kival.sessions ((COALESCE(revoked_at, expires_at)), id);

-- ---------------------------------------------------------------------
-- Function: kival.sessions_before_update()
-- Purpose
--   Enforce the limited set of mutations allowed for an active session.
-- Trigger contract
--   BEFORE UPDATE on `kival.sessions`.
-- Behavior
--   Keeps session identity, owner, credential hashes, and creation time
--   immutable while allowing expiry, activity, fresh-authentication, and
--   revocation metadata to change. A revoked session is immutable.
--   `updated_at` is maintained automatically.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.sessions_before_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.revoked_at IS NOT NULL THEN
        RAISE EXCEPTION 'revoked sessions are immutable';
    END IF;

    IF NEW.id IS DISTINCT FROM OLD.id
       OR NEW.user_id IS DISTINCT FROM OLD.user_id
       OR NEW.session_token_hash IS DISTINCT FROM OLD.session_token_hash
       OR NEW.csrf_token_hash IS DISTINCT FROM OLD.csrf_token_hash
       OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN
        RAISE EXCEPTION 'session identity and credential material are immutable';
    END IF;

    NEW.updated_at = now();

    IF (
        to_jsonb(NEW)
        - 'updated_at'
        - 'expires_at'
        - 'revoked_at'
        - 'revoked_by'
        - 'revoked_by_operator'
        - 'revocation_reason'
        - 'last_seen_at'
        - 'fresh_authenticated_at'
    ) IS DISTINCT FROM (
        to_jsonb(OLD)
        - 'updated_at'
        - 'expires_at'
        - 'revoked_at'
        - 'revoked_by'
        - 'revoked_by_operator'
        - 'revocation_reason'
        - 'last_seen_at'
        - 'fresh_authenticated_at'
    ) THEN
        RAISE EXCEPTION 'only session lifecycle/activity fields may be updated';
    END IF;

    RETURN NEW;
END;
$$;

COMMENT ON FUNCTION kival.sessions_before_update() IS
    'Restricts active session updates to expiry, activity, fresh-authentication, and revocation metadata and makes revoked sessions immutable.';

DROP TRIGGER IF EXISTS sessions_before_update ON kival.sessions;

CREATE TRIGGER sessions_before_update
BEFORE UPDATE ON kival.sessions
FOR EACH ROW
EXECUTE FUNCTION kival.sessions_before_update();

-- =====================================================================
-- Passkey enrollment and recovery capabilities
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.passkey_enrollment_codes (
    id uuid PRIMARY KEY DEFAULT uuidv7(),

    user_id uuid NOT NULL REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    code_hash bytea NOT NULL,

    created_by uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    created_by_operator boolean NOT NULL DEFAULT false,

    purpose text NOT NULL,

    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,

    revoked_at timestamptz,
    revoked_by uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    revoked_by_operator boolean NOT NULL DEFAULT false,

    CONSTRAINT passkey_enrollment_code_hash_length
        CHECK (octet_length(code_hash) = 32),

    CONSTRAINT passkey_enrollment_purpose_valid
        CHECK (purpose IN ('enrollment', 'passkey_reset')),

    CONSTRAINT passkey_enrollment_issuer_valid
        CHECK (
            (created_by_operator AND created_by IS NULL)
            OR (NOT created_by_operator AND created_by IS NOT NULL)
        ),

    CONSTRAINT passkey_enrollment_expiry_valid
        CHECK (expires_at > created_at),

    CONSTRAINT passkey_enrollment_consumed_after_creation
        CHECK (consumed_at IS NULL OR consumed_at >= created_at),

    CONSTRAINT passkey_enrollment_revoked_after_creation
        CHECK (revoked_at IS NULL OR revoked_at >= created_at),

    CONSTRAINT passkey_enrollment_revocation_complete
        CHECK (
            (revoked_at IS NULL AND revoked_by IS NULL AND NOT revoked_by_operator)
            OR
            (revoked_at IS NOT NULL AND (revoked_by IS NOT NULL) <> revoked_by_operator)
        ),

    CONSTRAINT passkey_enrollment_terminal_state_valid
        CHECK (consumed_at IS NULL OR revoked_at IS NULL)
);

CREATE UNIQUE INDEX IF NOT EXISTS passkey_enrollment_code_hash_unique
    ON kival.passkey_enrollment_codes (code_hash);

CREATE INDEX IF NOT EXISTS passkey_enrollment_codes_active_idx
    ON kival.passkey_enrollment_codes (user_id, expires_at)
    WHERE consumed_at IS NULL AND revoked_at IS NULL;

-- ---------------------------------------------------------------------
-- Function: kival.passkey_enrollment_codes_before_update()
-- Purpose
--   Enforce the single-use enrollment/reset capability lifecycle.
-- Trigger contract
--   BEFORE UPDATE on `kival.passkey_enrollment_codes`.
-- Behavior
--   Keeps capability identity, ownership, issuer, purpose, and expiry immutable.
--   An active capability may transition exactly once to consumed or revoked;
--   terminal capability rows are immutable.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.passkey_enrollment_codes_before_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.consumed_at IS NOT NULL OR OLD.revoked_at IS NOT NULL THEN
        RAISE EXCEPTION 'completed passkey enrollment codes are immutable';
    END IF;

    IF NEW.id IS DISTINCT FROM OLD.id
       OR NEW.user_id IS DISTINCT FROM OLD.user_id
       OR NEW.code_hash IS DISTINCT FROM OLD.code_hash
       OR NEW.created_by IS DISTINCT FROM OLD.created_by
       OR NEW.created_by_operator IS DISTINCT FROM OLD.created_by_operator
       OR NEW.purpose IS DISTINCT FROM OLD.purpose
       OR NEW.created_at IS DISTINCT FROM OLD.created_at
       OR NEW.expires_at IS DISTINCT FROM OLD.expires_at
       OR (NEW.consumed_at IS NULL AND NEW.revoked_at IS NULL)
       OR (NEW.consumed_at IS NOT NULL AND NEW.revoked_at IS NOT NULL)
       OR (NEW.revoked_at IS NULL AND (NEW.revoked_by IS NOT NULL OR NEW.revoked_by_operator))
       OR (NEW.revoked_at IS NOT NULL AND (NEW.revoked_by IS NOT NULL) = NEW.revoked_by_operator) THEN
        RAISE EXCEPTION 'passkey enrollment code mutation is not permitted';
    END IF;

    RETURN NEW;
END;
$$;

COMMENT ON FUNCTION kival.passkey_enrollment_codes_before_update() IS
    'Allows one terminal consumed-or-revoked transition for active passkey enrollment codes and makes terminal rows immutable.';

DROP TRIGGER IF EXISTS passkey_enrollment_codes_before_update ON kival.passkey_enrollment_codes;

CREATE TRIGGER passkey_enrollment_codes_before_update
BEFORE UPDATE ON kival.passkey_enrollment_codes
FOR EACH ROW
EXECUTE FUNCTION kival.passkey_enrollment_codes_before_update();

DROP TRIGGER IF EXISTS passkey_enrollment_codes_prevent_delete ON kival.passkey_enrollment_codes;

CREATE TRIGGER passkey_enrollment_codes_prevent_delete
BEFORE DELETE ON kival.passkey_enrollment_codes
FOR EACH ROW
EXECUTE FUNCTION kival.prevent_mutation();

COMMENT ON TABLE kival.passkey_enrollment_codes IS
    'Short-lived, single-use hashed codes issued by administrators for account-identifier-bound passkey enrollment.';

-- =====================================================================
-- WebAuthn ceremonies
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.webauthn_ceremonies (
    id uuid PRIMARY KEY DEFAULT uuidv7(),

    user_id uuid NOT NULL REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    session_id uuid REFERENCES kival.sessions(id)
        ON UPDATE RESTRICT
        ON DELETE CASCADE,

    enrollment_code_id uuid REFERENCES kival.passkey_enrollment_codes(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    kind text NOT NULL,
    challenge bytea NOT NULL,

    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    consumed_at timestamptz,

    CONSTRAINT webauthn_ceremonies_kind_valid
        CHECK (kind IN (
            'authentication',
            'registration',
            'enrollment_registration',
            'fresh_authentication'
        )),

    CONSTRAINT webauthn_ceremonies_challenge_length
        CHECK (octet_length(challenge) = 32),

    CONSTRAINT webauthn_ceremonies_expiry_valid
        CHECK (expires_at > created_at),

    CONSTRAINT webauthn_ceremonies_consumed_after_creation
        CHECK (consumed_at IS NULL OR consumed_at >= created_at),

    CONSTRAINT webauthn_ceremonies_session_binding
        CHECK (
            (kind IN ('registration', 'fresh_authentication') AND session_id IS NOT NULL)
            OR
            (kind IN ('authentication', 'enrollment_registration') AND session_id IS NULL)
        ),

    CONSTRAINT webauthn_ceremonies_enrollment_binding
        CHECK (
            (kind = 'enrollment_registration' AND enrollment_code_id IS NOT NULL)
            OR
            (kind <> 'enrollment_registration' AND enrollment_code_id IS NULL)
        )
);

CREATE INDEX IF NOT EXISTS webauthn_ceremonies_active_idx
    ON kival.webauthn_ceremonies (id, expires_at)
    WHERE consumed_at IS NULL;

CREATE INDEX IF NOT EXISTS webauthn_ceremonies_retention_idx
    ON kival.webauthn_ceremonies (user_id, expires_at, id);

CREATE UNIQUE INDEX IF NOT EXISTS webauthn_ceremonies_active_enrollment_code_unique
    ON kival.webauthn_ceremonies (enrollment_code_id)
    WHERE enrollment_code_id IS NOT NULL AND consumed_at IS NULL;

-- Allow exactly one mutation: consuming an active ceremony. Any other update,
-- including an update after consumption, is rejected by the shared immutable-row
-- trigger helper.
DROP TRIGGER IF EXISTS webauthn_ceremonies_prevent_update ON kival.webauthn_ceremonies;

CREATE TRIGGER webauthn_ceremonies_prevent_update
BEFORE UPDATE ON kival.webauthn_ceremonies
FOR EACH ROW
WHEN (
    OLD.consumed_at IS NOT NULL
    OR NEW.consumed_at IS NULL
    OR (to_jsonb(NEW) - 'consumed_at') IS DISTINCT FROM (to_jsonb(OLD) - 'consumed_at')
)
EXECUTE FUNCTION kival.prevent_mutation();

-- ---------------------------------------------------------------------
-- Function: kival.webauthn_ceremonies_before_delete()
-- Purpose
--   Protect live WebAuthn challenges while allowing bounded retention cleanup.
-- Trigger contract
--   BEFORE DELETE on `kival.webauthn_ceremonies`.
-- Behavior
--   Active, unexpired ceremonies cannot be deleted. Consumed or expired
--   ceremonies may be pruned by the application.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.webauthn_ceremonies_before_delete()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.consumed_at IS NULL AND OLD.expires_at > now() THEN
        RAISE EXCEPTION 'active WebAuthn ceremonies cannot be deleted';
    END IF;

    RETURN OLD;
END;
$$;

COMMENT ON FUNCTION kival.webauthn_ceremonies_before_delete() IS
    'Prevents deletion of active unexpired WebAuthn ceremonies while permitting cleanup of consumed or expired rows.';

DROP TRIGGER IF EXISTS webauthn_ceremonies_prevent_delete ON kival.webauthn_ceremonies;

CREATE TRIGGER webauthn_ceremonies_prevent_delete
BEFORE DELETE ON kival.webauthn_ceremonies
FOR EACH ROW
EXECUTE FUNCTION kival.webauthn_ceremonies_before_delete();

COMMENT ON TABLE kival.webauthn_ceremonies IS
    'Short-lived, single-use WebAuthn challenges bound to a user and, where required, a browser session or enrollment capability.';

-- =====================================================================
-- API keys
-- =====================================================================

CREATE TABLE IF NOT EXISTS kival.api_keys (
    id uuid PRIMARY KEY DEFAULT uuidv7(),

    user_id uuid NOT NULL REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    label text NOT NULL,
    token_hash bytea NOT NULL,

    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),

    authorization_revision integer NOT NULL DEFAULT 0,

    expires_at timestamptz,
    revoked_at timestamptz,
    revoked_by uuid REFERENCES kival.users(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,
    revoked_by_operator boolean NOT NULL DEFAULT false,
    last_used_at timestamptz,

    CONSTRAINT api_keys_label_not_blank
        CHECK (length(btrim(label)) > 0),
    CONSTRAINT api_keys_label_length
        CHECK (char_length(label) <= 64),
    CONSTRAINT api_keys_token_hash_sha256_length
        CHECK (octet_length(token_hash) = 32),
    CONSTRAINT api_keys_expires_at_after_created_at
        CHECK (expires_at IS NULL OR expires_at > created_at),
    CONSTRAINT api_keys_revocation_complete
        CHECK (
            (
                revoked_at IS NULL
                AND revoked_by IS NULL
                AND NOT revoked_by_operator
            )
            OR
            (
                revoked_at IS NOT NULL
                AND (revoked_by IS NOT NULL) <> revoked_by_operator
            )
        ),
    CONSTRAINT api_keys_last_used_at_after_created_at
        CHECK (last_used_at IS NULL OR last_used_at >= created_at),
    CONSTRAINT api_keys_revoked_at_after_created_at
        CHECK (revoked_at IS NULL OR revoked_at >= created_at),
    CONSTRAINT api_keys_updated_at_after_created_at
        CHECK (updated_at >= created_at),
    CONSTRAINT api_keys_authorization_revision_nonnegative
        CHECK (authorization_revision >= 0),
    CONSTRAINT api_keys_audit_identity_unique
        UNIQUE (id, user_id, label)
);

CREATE UNIQUE INDEX IF NOT EXISTS api_keys_token_hash_unique
    ON kival.api_keys (token_hash);

CREATE INDEX IF NOT EXISTS api_keys_user_active_idx
    ON kival.api_keys (user_id, created_at DESC)
    WHERE revoked_at IS NULL;

CREATE TABLE IF NOT EXISTS kival.api_key_scopes (
    api_key_id uuid NOT NULL REFERENCES kival.api_keys(id)
        ON UPDATE RESTRICT
        ON DELETE CASCADE,
    scope text NOT NULL,

    PRIMARY KEY (api_key_id, scope),

    CONSTRAINT api_key_scopes_scope_valid CHECK (scope IN (
        'workspaces:read',
        'workspaces:write',
        'objects:read',
        'objects:write',
        'attachments:read',
        'attachments:write',
        'graph:read',
        'graph:write',
        'events:read',
        'realtime:read',
        'access:manage',
        'admin'
    ))
);

CREATE TABLE IF NOT EXISTS kival.api_key_workspaces (
    api_key_id uuid NOT NULL REFERENCES kival.api_keys(id)
        ON UPDATE RESTRICT
        ON DELETE CASCADE,
    workspace_id uuid NOT NULL REFERENCES kival.workspaces(id)
        ON UPDATE RESTRICT
        ON DELETE RESTRICT,

    PRIMARY KEY (api_key_id, workspace_id)
);

CREATE INDEX IF NOT EXISTS api_key_workspaces_workspace_idx
    ON kival.api_key_workspaces (workspace_id, api_key_id);

COMMENT ON COLUMN kival.api_keys.authorization_revision IS
    'Monotonic optimistic-concurrency revision for the mutable scope and workspace-grant sets.';

COMMENT ON COLUMN kival.api_keys.revoked_by_operator IS
    'True when the API key was revoked by a deployment operator rather than a Kival user.';

COMMENT ON TABLE kival.api_key_scopes IS
    'Mutable explicit capability grants for an API key; authorization edits insert or delete rows without rotating the key token.';

COMMENT ON TABLE kival.api_key_workspaces IS
    'Mutable explicit workspace grants for an API key; absence of a row denies workspace access.';

-- ---------------------------------------------------------------------
-- Function: kival.api_key_authorization_before_mutation()
-- Purpose
--   Preserve the final authorization record of a revoked API key.
-- Trigger contract
--   BEFORE INSERT OR DELETE on API-key scope and workspace-grant tables.
-- Behavior
--   Locks the parent key and rejects authorization-row mutations once the key
--   has been revoked. Active keys remain editable through explicit row
--   insertion and deletion inside the revisioned application transaction.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.api_key_authorization_before_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    parent_revoked_at timestamptz;
    parent_api_key_id uuid;
BEGIN
    parent_api_key_id := CASE
        WHEN TG_OP = 'DELETE' THEN OLD.api_key_id
        ELSE NEW.api_key_id
    END;

    SELECT revoked_at
    INTO parent_revoked_at
    FROM kival.api_keys
    WHERE id = parent_api_key_id
    FOR UPDATE;

    IF NOT FOUND OR parent_revoked_at IS NOT NULL THEN
        RAISE EXCEPTION 'revoked API key authorization is immutable';
    END IF;

    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    END IF;

    RETURN NEW;
END;
$$;

COMMENT ON FUNCTION kival.api_key_authorization_before_mutation() IS
    'Locks the parent API key and prevents scope or workspace-grant insertion or deletion after revocation.';

DROP TRIGGER IF EXISTS api_key_scopes_guard_lifecycle ON kival.api_key_scopes;

CREATE TRIGGER api_key_scopes_guard_lifecycle
BEFORE INSERT OR DELETE ON kival.api_key_scopes
FOR EACH ROW
EXECUTE FUNCTION kival.api_key_authorization_before_mutation();

DROP TRIGGER IF EXISTS api_key_workspaces_guard_lifecycle ON kival.api_key_workspaces;

CREATE TRIGGER api_key_workspaces_guard_lifecycle
BEFORE INSERT OR DELETE ON kival.api_key_workspaces
FOR EACH ROW
EXECUTE FUNCTION kival.api_key_authorization_before_mutation();

DROP TRIGGER IF EXISTS api_key_scopes_prevent_update ON kival.api_key_scopes;

CREATE TRIGGER api_key_scopes_prevent_update
BEFORE UPDATE ON kival.api_key_scopes
FOR EACH ROW
EXECUTE FUNCTION kival.prevent_mutation();

DROP TRIGGER IF EXISTS api_key_workspaces_prevent_update ON kival.api_key_workspaces;

CREATE TRIGGER api_key_workspaces_prevent_update
BEFORE UPDATE ON kival.api_key_workspaces
FOR EACH ROW
EXECUTE FUNCTION kival.prevent_mutation();

-- ---------------------------------------------------------------------
-- Function: kival.api_keys_before_update()
-- Purpose
--   Enforce the lifecycle and activity policy for API keys.
-- Trigger contract
--   BEFORE UPDATE on `kival.api_keys`.
-- Behavior
--   Keeps key identity, owner, label, token hash, and creation time immutable.
--   Authorization revision, expiry, last-use, and revocation metadata may change
--   while active; a revoked key is immutable. Authorization revisions may only
--   advance by one. `updated_at` is maintained automatically.
-- ---------------------------------------------------------------------
CREATE OR REPLACE FUNCTION kival.api_keys_before_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF OLD.revoked_at IS NOT NULL THEN
        RAISE EXCEPTION 'revoked API keys are immutable';
    END IF;

    IF NEW.id IS DISTINCT FROM OLD.id THEN
        RAISE EXCEPTION 'API key id is immutable';
    END IF;

    IF NEW.user_id IS DISTINCT FROM OLD.user_id THEN
        RAISE EXCEPTION 'API key user_id is immutable';
    END IF;

    IF NEW.label IS DISTINCT FROM OLD.label THEN
        RAISE EXCEPTION 'API key label is immutable';
    END IF;

    IF NEW.token_hash IS DISTINCT FROM OLD.token_hash THEN
        RAISE EXCEPTION 'API key token hash is immutable';
    END IF;

    IF NEW.created_at IS DISTINCT FROM OLD.created_at THEN
        RAISE EXCEPTION 'API key created_at is immutable';
    END IF;

    IF NEW.authorization_revision IS DISTINCT FROM OLD.authorization_revision
       AND NEW.authorization_revision <> OLD.authorization_revision + 1 THEN
        RAISE EXCEPTION 'API key authorization_revision may only advance by one';
    END IF;

    NEW.updated_at = now();

    IF (
        to_jsonb(NEW)
        - 'updated_at'
        - 'authorization_revision'
        - 'expires_at'
        - 'revoked_at'
        - 'revoked_by'
        - 'revoked_by_operator'
        - 'last_used_at'
    ) IS DISTINCT FROM (
        to_jsonb(OLD)
        - 'updated_at'
        - 'authorization_revision'
        - 'expires_at'
        - 'revoked_at'
        - 'revoked_by'
        - 'revoked_by_operator'
        - 'last_used_at'
    ) THEN
        RAISE EXCEPTION 'only API key authorization revision and lifecycle/activity fields may be updated';
    END IF;

    RETURN NEW;
END;
$$;

COMMENT ON FUNCTION kival.api_keys_before_update() IS
    'Restricts active API key updates to a single-step authorization revision plus expiry, activity, and user/operator revocation metadata and makes revoked keys immutable.';

DROP TRIGGER IF EXISTS api_keys_before_update ON kival.api_keys;

CREATE TRIGGER api_keys_before_update
BEFORE UPDATE ON kival.api_keys
FOR EACH ROW
EXECUTE FUNCTION kival.api_keys_before_update();
