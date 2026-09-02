import { useState } from "react";
import { styles } from "../../shared/styles/index";
import { KivalLogo } from "../../shared/ui/KivalLogo";

type Props = {
  loading: boolean;
  error: string | null;
  onLogin: (username: string) => Promise<void>;
};

export function LoginPage({ loading, error, onLogin }: Props) {
  const [username, setUsername] = useState("");

  return (
    <main className="kival-login-page" style={styles.loginPage}>
      <div className="kival-login-shell" style={styles.loginShell}>
        <section className="kival-login-intro kival-dark-panel" style={styles.loginIntro}>
          <KivalLogo variant="on-dark" style={{ width: 88 }} />

          <div style={styles.loginIntroCopy}>
            <p style={styles.loginEyebrow}>Knowledge, connected</p>
            <h1 style={styles.loginHeadline}>Sign in to Kival.</h1>
            <p style={styles.loginDescription}>
              Sign in with your passkey to access your workspaces, objects, connections, and shared
              knowledge.
            </p>
          </div>
        </section>

        <section className="kival-login-panel" style={styles.loginPanel}>
          <form
            onSubmit={async (event) => {
              event.preventDefault();
              await onLogin(username);
            }}
            style={styles.loginCard}
          >
            <div style={styles.loginCardHeader}>
              <h2 style={styles.loginTitle}>Sign in</h2>
              <p style={styles.muted}>Use a passkey registered for your Kival account.</p>
            </div>

            <div style={styles.loginFields}>
              <label style={styles.field}>
                <span style={styles.fieldLabel}>Username</span>

                <input
                  type="text"
                  value={username}
                  onChange={(event) => setUsername(event.target.value)}
                  autoComplete="username webauthn"
                  maxLength={30}
                  placeholder="your-username"
                  autoFocus
                  required
                  disabled={loading}
                  style={styles.input}
                />
              </label>
            </div>

            {error && (
              <div style={styles.loginError} role="alert">
                {error}
              </div>
            )}

            <button type="submit" disabled={loading} style={styles.primaryButton}>
              {loading ? "Waiting for passkey…" : "Continue with passkey"}
            </button>
          </form>
        </section>
      </div>
    </main>
  );
}
