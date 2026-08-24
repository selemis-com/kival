import { styles } from "../styles/index";

type Props = {
  active: "workspaces" | "inbox" | "users" | "groups" | "events" | "security" | "api-keys";
  onWorkspacesClick: () => void;
  onUsersClick?: () => void;
  onGroupsClick?: () => void;
  onEventsClick?: () => void;
  onSecurityClick: () => void;
  onApiKeysClick: () => void;
};

export function KivalSideBar({
  active,
  onWorkspacesClick,
  onUsersClick,
  onGroupsClick,
  onEventsClick,
  onSecurityClick,
  onApiKeysClick,
}: Props) {
  return (
    <aside style={styles.sidebar}>
      <div style={styles.sidebarNavigation}>
        <nav style={styles.nav} aria-label="Kival navigation">
          <span style={styles.sidebarLabel}>Kival</span>

          <button
            type="button"
            style={active === "workspaces" ? styles.navItemActive : styles.navItem}
            onClick={onWorkspacesClick}
          >
            Workspaces
          </button>

          {onUsersClick && (
            <button
              type="button"
              style={active === "users" ? styles.navItemActive : styles.navItem}
              onClick={onUsersClick}
            >
              Users
            </button>
          )}

          {onGroupsClick && (
            <button
              type="button"
              style={active === "groups" ? styles.navItemActive : styles.navItem}
              onClick={onGroupsClick}
            >
              Groups
            </button>
          )}

          {onEventsClick && (
            <button
              type="button"
              style={active === "events" ? styles.navItemActive : styles.navItem}
              onClick={onEventsClick}
            >
              Events
            </button>
          )}
        </nav>

        <div style={styles.sidebarSection}>
          <span style={styles.sidebarLabel}>Account</span>

          <button
            type="button"
            style={active === "security" ? styles.navItemActive : styles.navItem}
            onClick={onSecurityClick}
          >
            Security
          </button>

          <button
            type="button"
            style={active === "api-keys" ? styles.navItemActive : styles.navItem}
            onClick={onApiKeysClick}
          >
            API keys
          </button>
        </div>
      </div>
    </aside>
  );
}
