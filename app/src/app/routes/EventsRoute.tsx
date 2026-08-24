import { Navigate } from "react-router";
import { EventsPage } from "../../features/events/EventsPage";
import { useKivalApp } from "../context";
import { useKivalNavigation } from "../navigation";

export function EventsRoute() {
  const app = useKivalApp();
  const navigation = useKivalNavigation();

  if (!app.isGlobalAdmin) {
    return <Navigate to="/" replace />;
  }

  return (
    <EventsPage
      user={app.user}
      workspaces={app.workspaces}
      workspacesNextCursor={app.workspacesNextCursor}
      workspacesLoadingMore={app.workspacesLoadingMore}
      onLoadMoreWorkspaces={app.loadMoreWorkspaces}
      onHome={navigation.onHome}
      onInboxClick={navigation.onInboxClick}
      unreadInboxCount={app.unreadInboxCount}
      onUsersClick={navigation.onUsersClick ?? (() => undefined)}
      onGroupsClick={navigation.onGroupsClick}
      onEventsClick={() => undefined}
      onSecurityClick={navigation.onSecurityClick}
      onApiKeysClick={navigation.onApiKeysClick}
      onLogout={app.logout}
    />
  );
}
