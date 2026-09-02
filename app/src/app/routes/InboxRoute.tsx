import { InboxPage } from "../../features/notifications/InboxPage";
import { useKivalApp } from "../context";
import { useKivalNavigation } from "../navigation";

export function InboxRoute() {
  const app = useKivalApp();
  const navigation = useKivalNavigation();

  return (
    <InboxPage
      user={app.user}
      workspaces={app.workspaces}
      workspacesNextCursor={app.workspacesNextCursor}
      workspacesLoadingMore={app.workspacesLoadingMore}
      unreadCount={app.unreadInboxCount}
      inboxRevision={app.inboxRevision}
      onLoadMoreWorkspaces={app.loadMoreWorkspaces}
      onHome={navigation.onHome}
      onWorkspaceSelect={navigation.onWorkspaceSelect}
      onInboxClick={navigation.onInboxClick}
      onUsersClick={navigation.onUsersClick}
      onGroupsClick={navigation.onGroupsClick}
      onEventsClick={navigation.onEventsClick}
      onSecurityClick={navigation.onSecurityClick}
      onApiKeysClick={navigation.onApiKeysClick}
      onInboxChanged={app.refreshInbox}
      onLogout={app.logout}
    />
  );
}
