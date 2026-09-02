import { formatTimestamp } from "../../shared/format";
import { styles } from "../../shared/styles/index";
import type { ApiKey } from "../../shared/types";
import { apiKeyScopeOptions } from "./model";

const scopeLabels = new Map(apiKeyScopeOptions.map(([scope, label]) => [scope, label]));

type Props = {
  apiKeys: ApiKey[];
  workspaceNames: Map<string, string>;
  onEdit: (apiKey: ApiKey) => void;
  onRevoke: (apiKey: ApiKey) => void;
};

function formatOptionalTimestamp(value: string | null) {
  return value ? formatTimestamp(value) : "Never";
}

function keyStatus(apiKey: ApiKey) {
  if (apiKey.revoked_at) {
    return "Revoked";
  }

  if (apiKey.expires_at && new Date(apiKey.expires_at).getTime() <= Date.now()) {
    return "Expired";
  }

  return "Active";
}

export function ApiKeyCards({ apiKeys, workspaceNames, onEdit, onRevoke }: Props) {
  return (
    <div style={styles.apiKeyList}>
      {apiKeys.map((apiKey) => {
        const status = keyStatus(apiKey);

        return (
          <article key={apiKey.id} style={styles.apiKeyCard}>
            <div style={styles.apiKeyCardHeader}>
              <div style={styles.apiKeyCardTitle}>
                <strong>{apiKey.label}</strong>
                <span style={status === "Active" ? styles.apiKeyStatus : styles.apiKeyStatusMuted}>
                  {status}
                </span>
              </div>
              {status === "Active" && (
                <div style={styles.directoryHeaderActions}>
                  <button
                    type="button"
                    style={styles.secondaryButtonCompact}
                    onClick={() => onEdit(apiKey)}
                  >
                    Edit access
                  </button>
                  <button
                    type="button"
                    style={styles.apiKeyDangerButton}
                    onClick={() => onRevoke(apiKey)}
                  >
                    Revoke
                  </button>
                </div>
              )}
            </div>

            <dl style={styles.apiKeyMetadata}>
              <div>
                <dt>Created</dt>
                <dd>{formatOptionalTimestamp(apiKey.created_at)}</dd>
              </div>
              <div>
                <dt>Last used</dt>
                <dd>{formatOptionalTimestamp(apiKey.last_used_at)}</dd>
              </div>
              <div>
                <dt>Expires</dt>
                <dd>{formatOptionalTimestamp(apiKey.expires_at)}</dd>
              </div>
              <div>
                <dt>Workspaces</dt>
                <dd>{apiKey.workspace_ids.length}</dd>
              </div>
            </dl>

            <div style={styles.apiKeyRestrictions}>
              <span style={styles.apiKeyRestrictionLabel}>Scopes</span>
              <div style={styles.apiKeyPills}>
                {apiKey.scopes.map((scope) => (
                  <span key={scope} style={styles.apiKeyPill}>
                    {scopeLabels.get(scope) ?? scope}
                  </span>
                ))}
              </div>
              <span style={styles.apiKeyRestrictionLabel}>Workspace access</span>
              <div style={styles.apiKeyPills}>
                {apiKey.workspace_ids.length === 0 ? (
                  <span style={styles.apiKeyPill}>None</span>
                ) : (
                  apiKey.workspace_ids.map((workspaceId) => (
                    <span key={workspaceId} style={styles.apiKeyPill}>
                      {workspaceNames.get(workspaceId) ?? workspaceId}
                    </span>
                  ))
                )}
              </div>
            </div>
          </article>
        );
      })}
    </div>
  );
}
