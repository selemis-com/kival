import type { CSSProperties } from "react";
import { apiKeysStyles } from "./apiKeys";
import { authStyles } from "./auth";
import { baseStyles } from "./base";
import { editorStyles } from "./editor";
import { eventsStyles } from "./events";
import { graphStyles } from "./graph";
import { inboxStyles } from "./inbox";
import { markdownStyles } from "./markdown";
import { navigationStyles } from "./navigation";
import { objectsStyles } from "./objects";
import { settingsStyles } from "./settings";

export { reset } from "./reset";

export const styles: Record<string, CSSProperties> = {
  ...apiKeysStyles,
  ...baseStyles,
  ...authStyles,
  ...navigationStyles,
  ...editorStyles,
  ...eventsStyles,
  ...graphStyles,
  ...inboxStyles,
  ...objectsStyles,
  ...markdownStyles,
  ...settingsStyles,
};
