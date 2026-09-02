import { ApiKeysPage } from "../../features/api-keys/ApiKeysPage";
import { useKivalApp } from "../context";
import { useKivalNavigation } from "../navigation";

export function ApiKeysRoute() {
  const app = useKivalApp();
  const navigation = useKivalNavigation();

  return (
    <ApiKeysPage
      user={app.user}
      workspaces={app.workspaces}
      workspacesNextCursor={app.workspacesNextCursor}
      workspacesLoadingMore={app.workspacesLoadingMore}
      onLoadMoreWorkspaces={app.loadMoreWorkspaces}
      onHome={navigation.onHome}
      onInboxClick={navigation.onInboxClick}
      unreadInboxCount={app.unreadInboxCount}
      onLogout={app.logout}
      onUsersClick={navigation.onUsersClick}
      onGroupsClick={navigation.onGroupsClick}
      onEventsClick={navigation.onEventsClick}
      onSecurityClick={navigation.onSecurityClick}
    />
  );
}
