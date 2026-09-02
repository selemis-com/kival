import { createBrowserRouter, Navigate } from "react-router";
import { PasskeyEnrollmentPage } from "../features/auth/PasskeyEnrollmentPage";
import { usePageTitle } from "./documentTitle";
import { KivalApp } from "./KivalApp";
import { RouteErrorPage } from "./RouteErrorPage";

function EnrollmentRoute({ code }: { code: string | null }) {
  usePageTitle("Create a passkey");
  return <PasskeyEnrollmentPage code={code} />;
}

export function createKivalRouter(enrollmentCode: string | null) {
  return createBrowserRouter([
    {
      path: "/auth/enroll",
      element: <EnrollmentRoute code={enrollmentCode} />,
      errorElement: <RouteErrorPage />,
    },
    {
      path: "/",
      element: <KivalApp />,
      errorElement: <RouteErrorPage />,
      HydrateFallback: null,
      children: [
        {
          index: true,
          lazy: async () => {
            const { HomeRoute } = await import("./routes/HomeRoute");
            return { Component: HomeRoute };
          },
        },
        {
          path: "inbox",
          lazy: async () => {
            const { InboxRoute } = await import("./routes/InboxRoute");
            return { Component: InboxRoute };
          },
        },
        {
          path: "users",
          lazy: async () => {
            const { UsersRoute } = await import("./routes/UsersRoute");
            return { Component: UsersRoute };
          },
        },
        {
          path: "groups",
          lazy: async () => {
            const { GroupsRoute } = await import("./routes/GroupsRoute");
            return { Component: GroupsRoute };
          },
        },
        {
          path: "events",
          lazy: async () => {
            const { EventsRoute } = await import("./routes/EventsRoute");
            return { Component: EventsRoute };
          },
        },
        {
          path: "settings/security",
          lazy: async () => {
            const { SecurityRoute } = await import("./routes/SecurityRoute");
            return { Component: SecurityRoute };
          },
        },
        {
          path: "settings/api-keys",
          lazy: async () => {
            const { ApiKeysRoute } = await import("./routes/ApiKeysRoute");
            return { Component: ApiKeysRoute };
          },
        },
        {
          path: "w/:workspaceId/*",
          lazy: async () => {
            const { WorkspaceRoute } = await import("./routes/WorkspaceRoute");
            return { Component: WorkspaceRoute };
          },
        },
        { path: "*", element: <Navigate to="/" replace /> },
      ],
    },
  ]);
}
