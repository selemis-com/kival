import { Navigate } from "react-router";
import { GroupsPage } from "../../features/groups/GroupsPage";
import { useKivalApp } from "../context";
import { useKivalNavigation } from "../navigation";

export function GroupsRoute() {
  const app = useKivalApp();
  const navigation = useKivalNavigation();

  if (!app.canManageGroups) {
    return <Navigate to="/" replace />;
  }

  return (
    <GroupsPage
      user={app.user}
      isGlobalAdmin={app.isGlobalAdmin}
      workspaces={app.workspaces}
      workspacesNextCursor={app.workspacesNextCursor}
      workspacesLoadingMore={app.workspacesLoadingMore}
      onLoadMoreWorkspaces={app.loadMoreWorkspaces}
      onHome={navigation.onHome}
      onInboxClick={navigation.onInboxClick}
      unreadInboxCount={app.unreadInboxCount}
      onWorkspaceSelect={navigation.onWorkspaceSelect}
      onSecurityClick={navigation.onSecurityClick}
      onApiKeysClick={navigation.onApiKeysClick}
      onUsersClick={navigation.onUsersClick}
      onGroupsClick={() => undefined}
      onEventsClick={navigation.onEventsClick}
      onLogout={app.logout}
      onCurrentUserAuthorityChanged={app.refreshCurrentIdentity}
    />
  );
}
