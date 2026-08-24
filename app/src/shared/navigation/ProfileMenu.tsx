import { useEffect, useRef, useState } from "react";
import { styles } from "../styles/index";
import { useTheme } from "../styles/theme";
import type { User } from "../types";

type Props = {
  user: User;
  onSecurityClick?: () => void;
  onApiKeysClick?: () => void;
  onLogout?: () => Promise<void>;
};

export function ProfileMenu({ user, onSecurityClick, onApiKeysClick, onLogout }: Props) {
  const avatarLabel = user.display_name.slice(0, 1).toUpperCase() || "R";
  const { preference: themePreference, setPreference: setThemePreference } = useTheme();
  const [open, setOpen] = useState(false);
  const [logoutLoading, setLogoutLoading] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) {
      return;
    }

    function handlePointerDown(event: PointerEvent) {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setOpen(false);
      }
    }

    document.addEventListener("pointerdown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);

    return () => {
      document.removeEventListener("pointerdown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [open]);

  return (
    <div ref={menuRef} style={styles.userMenu}>
      {open && (
        <div role="menu" style={styles.userMenuPopover}>
          <div style={styles.userMenuIdentity}>
            <strong>{user.display_name}</strong>
            <span style={styles.userMenuUsername}>{user.username}</span>
          </div>

          <div style={styles.userMenuDivider} />

          {onSecurityClick && (
            <button
              type="button"
              role="menuitem"
              style={styles.userMenuItem}
              onClick={() => {
                setOpen(false);
                onSecurityClick();
              }}
            >
              Security
            </button>
          )}

          {onApiKeysClick && (
            <button
              type="button"
              role="menuitem"
              style={styles.userMenuItem}
              onClick={() => {
                setOpen(false);
                onApiKeysClick();
              }}
            >
              API keys
            </button>
          )}

          {(onSecurityClick || onApiKeysClick) && <div style={styles.userMenuDivider} />}

          <div style={styles.userMenuTheme}>
            <span style={styles.userMenuThemeLabel}>Appearance</span>
            <div style={styles.userMenuThemeOptions}>
              {(["system", "light", "dark"] as const).map((preference) => (
                <button
                  key={preference}
                  type="button"
                  style={
                    themePreference === preference
                      ? styles.userMenuThemeOptionActive
                      : styles.userMenuThemeOption
                  }
                  aria-pressed={themePreference === preference}
                  onClick={() => setThemePreference(preference)}
                >
                  {preference[0].toUpperCase() + preference.slice(1)}
                </button>
              ))}
            </div>
          </div>

          <div style={styles.userMenuDivider} />

          <button
            type="button"
            role="menuitem"
            style={styles.userMenuItem}
            disabled={logoutLoading}
            onClick={async () => {
              if (!onLogout) {
                return;
              }

              setLogoutLoading(true);
              try {
                await onLogout();
              } finally {
                setLogoutLoading(false);
              }
            }}
          >
            {logoutLoading ? "Logging out…" : "Log out"}
          </button>
        </div>
      )}

      <button
        type="button"
        style={styles.userMenuTrigger}
        aria-label={`Open profile menu for ${user.display_name}`}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((current) => !current)}
      >
        <div style={styles.avatar}>{avatarLabel}</div>
      </button>
    </div>
  );
}
