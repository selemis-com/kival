import { SecurityPage } from "../../features/security/SecurityPage";
import { useKivalApp } from "../context";
import { useKivalNavigation } from "../navigation";

export function SecurityRoute() {
  const app = useKivalApp();
  const navigation = useKivalNavigation();

  return (
    <SecurityPage
      user={app.user}
      workspaces={app.workspaces}
      workspacesNextCursor={app.workspacesNextCursor}
      workspacesLoadingMore={app.workspacesLoadingMore}
      onLoadMoreWorkspaces={app.loadMoreWorkspaces}
      onHome={navigation.onHome}
      onInboxClick={navigation.onInboxClick}
      unreadInboxCount={app.unreadInboxCount}
      onSecurityClick={() => undefined}
      onApiKeysClick={navigation.onApiKeysClick}
      onUsersClick={navigation.onUsersClick}
      onGroupsClick={navigation.onGroupsClick}
      onEventsClick={navigation.onEventsClick}
      onLogout={app.logout}
      onCurrentSessionRevoked={() => window.location.assign("/")}
    />
  );
}
