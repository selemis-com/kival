import type { ReactNode } from "react";
import { useState } from "react";
import { styles } from "../styles/index";

type CardProps = {
  displayName: string;
  username?: string;
  meta?: string;
  detail?: string;
  workspaceRole?: string;
  accessRole?: string;
  align?: "left" | "right";
};

function formatRole(role: string) {
  return role.replaceAll("_", " ").replace(/^./, (character) => character.toUpperCase());
}

export function ProfileHoverCard({
  displayName,
  username,
  meta,
  detail,
  workspaceRole,
  accessRole,
  align = "left",
}: CardProps) {
  return (
    <span
      style={{
        ...styles.profileHoverCard,
        ...(align === "right" ? { right: 0 } : { left: 0 }),
      }}
    >
      <strong style={styles.profileHoverCardName}>{displayName}</strong>
      <span style={styles.profileHoverCardMeta}>{username ? `@${username}` : meta}</span>
      {detail && <span style={styles.profileHoverCardDetail}>{detail}</span>}
      {(workspaceRole || accessRole) && (
        <span style={styles.profileHoverCardRoles}>
          {workspaceRole && (
            <span style={styles.profileHoverCardRole}>
              <span style={styles.profileHoverCardRoleLabel}>Workspace role</span>
              <span style={styles.profileHoverCardRoleValue}>{formatRole(workspaceRole)}</span>
            </span>
          )}
          {accessRole && (
            <span style={styles.profileHoverCardRole}>
              <span style={styles.profileHoverCardRoleLabel}>Object access</span>
              <span style={styles.profileHoverCardRoleValue}>{formatRole(accessRole)}</span>
            </span>
          )}
        </span>
      )}
    </span>
  );
}

type NameProps = CardProps & {
  children: ReactNode;
};

export function ProfileHoverName({ children, ...cardProps }: NameProps) {
  const [open, setOpen] = useState(false);

  return (
    <button
      type="button"
      style={open ? { ...styles.profileHoverName, zIndex: 50 } : styles.profileHoverName}
      onPointerEnter={() => setOpen(true)}
      onPointerLeave={() => setOpen(false)}
      onFocus={() => setOpen(true)}
      onBlur={() => setOpen(false)}
    >
      {children}
      {open && <ProfileHoverCard {...cardProps} />}
    </button>
  );
}
