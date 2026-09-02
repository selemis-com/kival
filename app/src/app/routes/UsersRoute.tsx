import { Navigate } from "react-router";
import { UsersPage } from "../../features/users/UsersPage";
import { useKivalApp } from "../context";
import { useKivalNavigation } from "../navigation";

export function UsersRoute() {
  const app = useKivalApp();
  const navigation = useKivalNavigation();

  if (!app.isGlobalAdmin) {
    return <Navigate to="/" replace />;
  }

  return (
    <UsersPage
      user={app.user}
      workspaces={app.workspaces}
      workspacesNextCursor={app.workspacesNextCursor}
      workspacesLoadingMore={app.workspacesLoadingMore}
      onLoadMoreWorkspaces={app.loadMoreWorkspaces}
      onHome={navigation.onHome}
      onInboxClick={navigation.onInboxClick}
      unreadInboxCount={app.unreadInboxCount}
      onWorkspaceSelect={navigation.onWorkspaceSelect}
      onUsersClick={() => undefined}
      onGroupsClick={navigation.onGroupsClick}
      onEventsClick={navigation.onEventsClick ?? (() => undefined)}
      onSecurityClick={navigation.onSecurityClick}
      onApiKeysClick={navigation.onApiKeysClick}
      onLogout={app.logout}
      onCurrentUserUpdate={app.replaceCurrentUser}
    />
  );
}
