import { isRouteErrorResponse, useRouteError } from "react-router";
import { styles } from "../shared/styles/index";
import { KivalLogo } from "../shared/ui/KivalLogo";
import { usePageTitle } from "./documentTitle";

export function RouteErrorPage() {
  usePageTitle("Something went wrong");
  const error = useRouteError();
  const message = isRouteErrorResponse(error)
    ? error.statusText || `Request failed with status ${error.status}.`
    : error instanceof Error
      ? error.message
      : "Kival could not render this page.";

  return (
    <main style={styles.loadingPage}>
      <section style={{ display: "grid", gap: 12, maxWidth: 520, textAlign: "center" }}>
        <KivalLogo style={{ width: 118, margin: "0 auto" }} />
        <p role="alert" style={styles.error}>
          {message}
        </p>
        <a href="/">Return to Kival</a>
      </section>
    </main>
  );
}
