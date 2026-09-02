import { styles } from "../styles/index";

type Props = {
  view: "home" | "favorites" | "recent" | "graph" | "archived" | "members" | "settings";
  onViewChange: (
    view: "home" | "favorites" | "recent" | "graph" | "archived" | "members" | "settings",
  ) => void;
  canManageWorkspace: boolean;
};

export function SideBar({ view, onViewChange, canManageWorkspace }: Props) {
  return (
    <aside style={styles.sidebar}>
      <div style={styles.sidebarNavigation}>
        <nav style={styles.nav} aria-label="Workspace navigation">
          <span style={styles.sidebarLabel}>Workspace</span>

          <button
            type="button"
            style={view === "home" ? styles.navItemActive : styles.navItem}
            onClick={() => onViewChange("home")}
          >
            Home
          </button>

          <button
            type="button"
            style={view === "favorites" ? styles.navItemActive : styles.navItem}
            onClick={() => onViewChange("favorites")}
          >
            Favorites
          </button>

          <button
            type="button"
            style={view === "recent" ? styles.navItemActive : styles.navItem}
            onClick={() => onViewChange("recent")}
          >
            Recent
          </button>

          <button
            type="button"
            style={view === "graph" ? styles.navItemActive : styles.navItem}
            onClick={() => onViewChange("graph")}
          >
            Graph
          </button>
        </nav>

        <div style={styles.sidebarSection}>
          <span style={styles.sidebarLabel}>Manage</span>

          <button
            type="button"
            style={view === "archived" ? styles.navItemActive : styles.navItem}
            onClick={() => onViewChange("archived")}
          >
            Archived
          </button>

          <button
            type="button"
            style={view === "members" ? styles.navItemActive : styles.navItem}
            onClick={() => onViewChange("members")}
          >
            Members
          </button>

          {canManageWorkspace && (
            <button
              type="button"
              style={view === "settings" ? styles.navItemActive : styles.navItem}
              onClick={() => onViewChange("settings")}
            >
              Settings
            </button>
          )}
        </div>
      </div>
    </aside>
  );
}
