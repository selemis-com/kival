import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { RouterProvider } from "react-router";
import { createKivalRouter } from "./app/router";
import { readEnrollmentCode, rememberEnrollmentCode } from "./shared/auth/webauthn";
import { themeCss } from "./shared/styles/constants";
import { fontCss } from "./shared/styles/fonts";
import { globalCss } from "./shared/styles/global";
import { reset } from "./shared/styles/reset";
import { applyThemePreference, readThemePreference, ThemeProvider } from "./shared/styles/theme";
import { animatedSelectCss } from "./shared/ui/AnimatedSelect";

applyThemePreference(readThemePreference());

const initialUrl = new URL(window.location.href);
const enrollmentFragment = new URLSearchParams(initialUrl.hash.slice(1));
const fragmentEnrollmentCode =
  initialUrl.pathname === "/auth/enroll" ? enrollmentFragment.get("code") : null;
const fragmentEnrollmentUsername =
  initialUrl.pathname === "/auth/enroll" ? enrollmentFragment.get("username") : null;

if (fragmentEnrollmentCode) {
  rememberEnrollmentCode(fragmentEnrollmentCode);
}

if (fragmentEnrollmentCode !== null) {
  enrollmentFragment.delete("code");
  const remainingFragment = enrollmentFragment.toString();
  initialUrl.hash = remainingFragment ? `#${remainingFragment}` : "";
  window.history.replaceState(
    window.history.state,
    "",
    `${initialUrl.pathname}${initialUrl.search}${initialUrl.hash}`,
  );
}

const enrollmentCode =
  initialUrl.pathname === "/auth/enroll" ? (fragmentEnrollmentCode ?? readEnrollmentCode()) : null;

const styleElement = document.createElement("style");
styleElement.textContent = `${fontCss}\n${themeCss}\n${reset}\n${globalCss}\n${animatedSelectCss}`;
document.head.appendChild(styleElement);

const rootElement = document.getElementById("root");

if (!rootElement) {
  throw new Error('Root element with id "root" not found');
}

const router = createKivalRouter(enrollmentCode, fragmentEnrollmentUsername);

createRoot(rootElement).render(
  <StrictMode>
    <ThemeProvider>
      <RouterProvider router={router} />
    </ThemeProvider>
  </StrictMode>,
);
