import { useNavigate } from "react-router";
import { useKivalApp } from "./context";

export function useKivalNavigation() {
  const navigate = useNavigate();
  const { isGlobalAdmin, canManageGroups } = useKivalApp();

  return {
    onHome: () => navigate("/"),
    onWorkspaceSelect: (workspaceId: string) => navigate(`/w/${workspaceId}`),
    onInboxClick: () => navigate("/inbox"),
    onSecurityClick: () => navigate("/settings/security"),
    onApiKeysClick: () => navigate("/settings/api-keys"),
    onUsersClick: isGlobalAdmin ? () => navigate("/users") : undefined,
    onGroupsClick: canManageGroups ? () => navigate("/groups") : undefined,
    onEventsClick: isGlobalAdmin ? () => navigate("/events") : undefined,
  };
}
