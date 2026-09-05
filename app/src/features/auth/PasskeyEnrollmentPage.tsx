import { useState } from "react";
import { Link, useNavigate } from "react-router";
import { finishPasskeyEnrollment, startPasskeyEnrollment } from "../../shared/api";
import {
  clearEnrollmentCode,
  decodeBase64Url,
  registrationCredential,
} from "../../shared/auth/webauthn";
import { styles } from "../../shared/styles/index";
import { KivalLogo } from "../../shared/ui/KivalLogo";

type Props = {
  code: string | null;
  initialUsername: string | null;
};

export function PasskeyEnrollmentPage({ code, initialUsername }: Props) {
  const navigate = useNavigate();
  const [username, setUsername] = useState(initialUsername ?? "");
  const [label, setLabel] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const missingCode = !code;

  return (
    <main className="kival-login-page" style={styles.loginPage}>
      <div className="kival-login-shell" style={styles.loginShell}>
        <section className="kival-login-intro kival-dark-panel" style={styles.loginIntro}>
          <KivalLogo variant="on-dark" style={{ width: 88 }} />

          <div style={styles.loginIntroCopy}>
            <p style={styles.loginEyebrow}>Sign up</p>
            <h1 style={styles.loginHeadline}>Create your passkey.</h1>
            <p style={styles.loginDescription}>
              Use your invitation to sign up for Kival. You will use a passkey instead of a password
              to sign in.
            </p>
          </div>
        </section>

        <section className="kival-login-panel" style={styles.loginPanel}>
          <form
            onSubmit={async (event) => {
              event.preventDefault();
              if (!code) {
                setError("This enrollment link is missing its one-time code.");
                return;
              }
              if (!window.PublicKeyCredential || !navigator.credentials?.create) {
                setError("This browser or device does not support passkeys.");
                return;
              }
              const normalizedLabel = label.trim();

              if (!normalizedLabel) {
                setError("Enter a name for this passkey.");
                return;
              }

              setLoading(true);
              setError(null);

              try {
                const options = await startPasskeyEnrollment(username.trim(), code);
                const publicKey: PublicKeyCredentialCreationOptions = {
                  ...options.publicKey,
                  challenge: decodeBase64Url(options.publicKey.challenge),
                  user: {
                    ...options.publicKey.user,
                    id: decodeBase64Url(options.publicKey.user.id),
                  },
                  excludeCredentials: options.publicKey.excludeCredentials?.map((credential) => ({
                    ...credential,
                    id: decodeBase64Url(credential.id),
                  })),
                };
                const created = await navigator.credentials.create({ publicKey });

                if (!(created instanceof PublicKeyCredential)) {
                  throw new Error("The authenticator did not create a passkey.");
                }

                await finishPasskeyEnrollment({
                  username: username.trim(),
                  code,
                  ceremonyId: options.ceremonyId,
                  label: normalizedLabel,
                  credential: registrationCredential(created),
                });
                clearEnrollmentCode();
                navigate("/", { replace: true });
              } catch (cause) {
                if (cause instanceof DOMException && cause.name === "NotAllowedError") {
                  setError("Passkey creation was cancelled or was not allowed by this device.");
                } else {
                  setError(cause instanceof Error ? cause.message : "Passkey creation failed.");
                }
              } finally {
                setLoading(false);
              }
            }}
            style={styles.loginCard}
          >
            <div style={styles.loginCardHeader}>
              <h2 style={styles.loginTitle}>Sign up</h2>
              <p style={styles.muted}>
                Enter the username your administrator assigned to this account.
              </p>
            </div>

            <div style={styles.loginFields}>
              <label style={styles.field}>
                <span style={styles.fieldLabel}>Username</span>
                <input
                  type="text"
                  value={username}
                  onChange={(event) => setUsername(event.target.value)}
                  autoComplete="username"
                  readOnly={initialUsername !== null}
                  maxLength={30}
                  placeholder="your-username"
                  autoFocus
                  required
                  disabled={loading || missingCode}
                  style={{
                    ...styles.input,
                    ...(initialUsername !== null ? styles.inputReadOnly : {}),
                  }}
                />
              </label>

              <label style={styles.field}>
                <span style={styles.fieldLabel}>Passkey name</span>
                <input
                  type="text"
                  data-1p-ignore="true"
                  value={label}
                  onChange={(event) => setLabel(event.target.value)}
                  autoComplete="off"
                  maxLength={64}
                  placeholder="Work laptop"
                  required
                  disabled={loading || missingCode}
                  style={styles.input}
                />
                <span style={styles.fieldHint}>Use a name you will recognize later.</span>
              </label>
            </div>

            {(missingCode || error) && (
              <div style={styles.loginError} role="alert">
                {error ??
                  "This enrollment link is incomplete. Ask your administrator for a new link."}
              </div>
            )}

            <button type="submit" disabled={loading || missingCode} style={styles.primaryButton}>
              {loading ? "Creating passkey…" : "Create passkey"}
            </button>

            <Link to="/" style={styles.authTextLink}>
              Return to sign in
            </Link>
          </form>
        </section>
      </div>
    </main>
  );
}
