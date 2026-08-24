import { useEffect, useMemo, useState } from "react";
import { kival } from "../../../shared/api";
import { styles } from "../../../shared/styles/index";
import type { ObjectGrant, WorkspaceGroup, WorkspaceMembership } from "../../../shared/types";
import { ProfileHoverCard } from "../../../shared/ui/ProfileHoverCard";

type Props = {
  workspaceId: string;
  objectId: string;
  currentUserId: string;
  onClick: () => void;
};

const MAX_VISIBLE_AVATARS = 4;

function initials(value: string) {
  const parts = value.trim().split(/\s+/).filter(Boolean);

  if (parts.length === 0) {
    return "?";
  }

  return parts
    .slice(0, 2)
    .map((part) => part.slice(0, 1).toUpperCase())
    .join("");
}

export function ObjectShareAvatars({ workspaceId, objectId, currentUserId, onClick }: Props) {
  const [grants, setGrants] = useState<ObjectGrant[]>([]);
  const [memberships, setMemberships] = useState<WorkspaceMembership[]>([]);
  const [groups, setGroups] = useState<WorkspaceGroup[]>([]);
  const [hoveredGrantId, setHoveredGrantId] = useState<string | null>(null);

  useEffect(() => {
    const controller = new AbortController();

    void Promise.all([
      kival.listObjectGrants({ workspaceId, objectId, signal: controller.signal }),
      kival.listWorkspaceMemberships({ workspaceId, signal: controller.signal }),
      kival.listWorkspaceGroups({ workspaceId, signal: controller.signal }),
    ]).then(
      ([grantResponse, membershipResponse, groupResponse]) => {
        setGrants(grantResponse.items);
        setMemberships(membershipResponse.items);
        setGroups(groupResponse.items);
      },
      () => {
        if (!controller.signal.aborted) {
          setGrants([]);
          setMemberships([]);
          setGroups([]);
        }
      },
    );

    return () => controller.abort();
  }, [objectId, workspaceId]);

  const membershipsByUserId = useMemo(
    () => new Map(memberships.map((membership) => [membership.user_id, membership])),
    [memberships],
  );
  const groupsById = useMemo(
    () => new Map(groups.map((group) => [group.group_id, group])),
    [groups],
  );
  const recipients = grants.filter((grant) => grant.principal_user_id !== currentUserId);
  const visibleRecipients = recipients.slice(0, MAX_VISIBLE_AVATARS);
  const remaining = recipients.length - visibleRecipients.length;

  if (recipients.length === 0) {
    return null;
  }

  return (
    <button
      type="button"
      style={styles.objectShareAvatarButton}
      aria-label={`Manage access shared with ${recipients.length} ${
        recipients.length === 1 ? "recipient" : "recipients"
      }`}
      onClick={onClick}
      onFocus={() => setHoveredGrantId(visibleRecipients[0]?.id ?? null)}
      onBlur={() => setHoveredGrantId(null)}
    >
      <span style={styles.objectShareAvatars}>
        {visibleRecipients.map((grant) => {
          const membership = grant.principal_user_id
            ? membershipsByUserId.get(grant.principal_user_id)
            : null;
          const group = grant.principal_group_id ? groupsById.get(grant.principal_group_id) : null;
          const label = membership
            ? `${membership.user_display_name} (${membership.user_username})`
            : group
              ? `Group ${group.group_name}`
              : grant.principal_group_id
                ? "Unknown group"
                : "Unknown user";

          return (
            <span
              key={grant.id}
              style={
                hoveredGrantId === grant.id
                  ? { ...styles.objectShareAvatar, zIndex: 2 }
                  : styles.objectShareAvatar
              }
              aria-hidden="true"
              onPointerEnter={() => setHoveredGrantId(grant.id)}
              onPointerLeave={() => setHoveredGrantId(null)}
            >
              {membership
                ? initials(membership.user_display_name || membership.user_username)
                : group
                  ? initials(group.group_name)
                  : grant.principal_group_id
                    ? "G"
                    : "?"}

              {hoveredGrantId === grant.id && (
                <ProfileHoverCard
                  displayName={membership?.user_display_name || group?.group_name || label}
                  username={membership?.user_username}
                  meta={group ? "Group" : membership ? undefined : label}
                  workspaceRole={membership?.workspace_role}
                  accessRole={membership?.workspace_role === "admin" ? "admin" : grant.object_role}
                  align="right"
                />
              )}
            </span>
          );
        })}

        {remaining > 0 && (
          <span style={styles.objectShareAvatarOverflow} aria-hidden="true">
            +{remaining}
          </span>
        )}
      </span>
    </button>
  );
}
