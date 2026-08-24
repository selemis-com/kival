import { useNavigate } from "react-router";
import { WorkspaceChooser } from "../../features/workspaces/WorkspaceChooser";
import { useKivalApp } from "../context";
import { useKivalNavigation } from "../navigation";

export function HomeRoute() {
  const app = useKivalApp();
  const navigation = useKivalNavigation();
  const navigate = useNavigate();

  return (
    <WorkspaceChooser
      user={app.user}
      workspaces={app.workspaces}
      pinnedWorkspaces={app.pinnedWorkspaces}
      workspacesNextCursor={app.workspacesNextCursor}
      workspacesLoadingMore={app.workspacesLoadingMore}
      error={app.error}
      onLoadMoreWorkspaces={app.loadMoreWorkspaces}
      onInboxClick={navigation.onInboxClick}
      unreadInboxCount={app.unreadInboxCount}
      onOpenWorkspace={(workspace) => navigate(`/w/${workspace.id}`)}
      onCreateWorkspace={app.createWorkspace}
      onRestoreWorkspace={app.restoreWorkspace}
      onSetWorkspacePin={app.setWorkspacePin}
      onSecurityClick={navigation.onSecurityClick}
      onApiKeysClick={navigation.onApiKeysClick}
      onUsersClick={navigation.onUsersClick}
      onGroupsClick={navigation.onGroupsClick}
      onEventsClick={navigation.onEventsClick}
      onLogout={app.logout}
    />
  );
}
