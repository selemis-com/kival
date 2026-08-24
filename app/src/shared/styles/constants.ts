export type ResolvedTheme = "light" | "dark";
export type ThemePreference = "system" | ResolvedTheme;

const themeVariableNames = {
  background: "--kival-background",
  surface: "--kival-surface",
  surfaceSubtle: "--kival-surface-subtle",
  surfaceMuted: "--kival-surface-muted",
  surfaceSelected: "--kival-surface-selected",
  surfaceStrong: "--kival-surface-strong",
  buttonPrimary: "--kival-button-primary",
  text: "--kival-text",
  textMuted: "--kival-text-muted",
  textSubtle: "--kival-text-subtle",
  textOnStrong: "--kival-text-on-strong",
  border: "--kival-border",
  borderStrong: "--kival-border-strong",
  accent: "--kival-accent",
  accentMuted: "--kival-accent-muted",
  danger: "--kival-danger",
  dangerStrong: "--kival-danger-strong",
  dangerSurface: "--kival-danger-surface",
  dangerBorder: "--kival-danger-border",
  highlight: "--kival-highlight",
  overlay: "--kival-overlay",
  overlayStrong: "--kival-overlay-strong",
  glass: "--kival-glass",
  glassStrong: "--kival-glass-strong",
  shadowSmall: "--kival-shadow-small",
  shadowMedium: "--kival-shadow-medium",
  shadowLarge: "--kival-shadow-large",
} as const;

type ThemeValues = Record<keyof typeof themeVariableNames, string>;

const themes: Record<ResolvedTheme, ThemeValues> = {
  light: {
    background: "#ece6d8",
    surface: "#f6f1e6",
    surfaceSubtle: "#eee7d9",
    surfaceMuted: "#e2d8c5",
    surfaceSelected: "#d8cbb4",
    surfaceStrong: "#11100d",
    buttonPrimary: "#11100d",
    text: "#11100d",
    textMuted: "#5d574d",
    textSubtle: "#81786a",
    textOnStrong: "#f6f1e6",
    border: "#d5cbb8",
    borderStrong: "#a99d87",
    accent: "#7a2e18",
    accentMuted: "#d1a898",
    danger: "#b42318",
    dangerStrong: "#9f2d24",
    dangerSurface: "#fff3f1",
    dangerBorder: "#f0d2ce",
    highlight: "#e6c794",
    overlay: "rgba(17, 16, 13, 0.24)",
    overlayStrong: "rgba(17, 16, 13, 0.42)",
    glass: "rgba(236, 230, 216, 0.92)",
    glassStrong: "rgba(246, 241, 230, 0.96)",
    shadowSmall: "0 2px 8px rgba(17, 16, 13, 0.06)",
    shadowMedium: "0 12px 30px rgba(17, 16, 13, 0.12)",
    shadowLarge: "0 20px 56px rgba(17, 16, 13, 0.18)",
  },
  dark: {
    background: "#171512",
    surface: "#201e1a",
    surfaceSubtle: "#27231d",
    surfaceMuted: "#332e26",
    surfaceSelected: "#40382c",
    surfaceStrong: "#0f0e0c",
    buttonPrimary: "#a84b2d",
    text: "#eee7da",
    textMuted: "#b7ad9d",
    textSubtle: "#8d8273",
    textOnStrong: "#f6f1e6",
    border: "#3e382f",
    borderStrong: "#5b5143",
    accent: "#d27855",
    accentMuted: "#75402e",
    danger: "#f97066",
    dangerStrong: "#fda29b",
    dangerSurface: "#3a211f",
    dangerBorder: "#713b35",
    highlight: "#66501f",
    overlay: "rgba(0, 0, 0, 0.46)",
    overlayStrong: "rgba(0, 0, 0, 0.62)",
    glass: "rgba(31, 31, 29, 0.92)",
    glassStrong: "rgba(31, 31, 29, 0.97)",
    shadowSmall: "0 2px 10px rgba(0, 0, 0, 0.24)",
    shadowMedium: "0 14px 38px rgba(0, 0, 0, 0.34)",
    shadowLarge: "0 24px 70px rgba(0, 0, 0, 0.46)",
  },
};

function cssVariables(values: ThemeValues) {
  return Object.entries(themeVariableNames)
    .map(([name, variable]) => `${variable}: ${values[name as keyof ThemeValues]};`)
    .join("\n");
}

export const themeCss = `
:root {
  ${cssVariables(themes.light)}
  color-scheme: light;
}

:root[data-theme="dark"] {
  ${cssVariables(themes.dark)}
  color-scheme: dark;
}

@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) {
    ${cssVariables(themes.dark)}
    color-scheme: dark;
  }
}
`;

export const colors = {
  transparent: "transparent",
  background: `var(${themeVariableNames.background})`,
  surface: `var(${themeVariableNames.surface})`,
  surfaceSubtle: `var(${themeVariableNames.surfaceSubtle})`,
  surfaceMuted: `var(${themeVariableNames.surfaceMuted})`,
  surfaceSelected: `var(${themeVariableNames.surfaceSelected})`,
  surfaceStrong: `var(${themeVariableNames.surfaceStrong})`,
  buttonPrimary: `var(${themeVariableNames.buttonPrimary})`,
  text: `var(${themeVariableNames.text})`,
  textMuted: `var(${themeVariableNames.textMuted})`,
  textSubtle: `var(${themeVariableNames.textSubtle})`,
  textOnStrong: `var(${themeVariableNames.textOnStrong})`,
  border: `var(${themeVariableNames.border})`,
  borderStrong: `var(${themeVariableNames.borderStrong})`,
  accent: `var(${themeVariableNames.accent})`,
  accentMuted: `var(${themeVariableNames.accentMuted})`,
  danger: `var(${themeVariableNames.danger})`,
  dangerStrong: `var(${themeVariableNames.dangerStrong})`,
  dangerSurface: `var(${themeVariableNames.dangerSurface})`,
  dangerBorder: `var(${themeVariableNames.dangerBorder})`,
  highlight: `var(${themeVariableNames.highlight})`,
  overlay: `var(${themeVariableNames.overlay})`,
  overlayStrong: `var(${themeVariableNames.overlayStrong})`,
  glass: `var(${themeVariableNames.glass})`,
  glassStrong: `var(${themeVariableNames.glassStrong})`,
} as const;

export const fontFamilies = {
  sans: '"Karla", ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
  display:
    '"Inter", ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
  mono: '"IBM Plex Mono", ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace',
} as const;

export const shadows = {
  small: `var(${themeVariableNames.shadowSmall})`,
  medium: `var(${themeVariableNames.shadowMedium})`,
  large: `var(${themeVariableNames.shadowLarge})`,
} as const;

export const layout = {
  topBarHeight: 58,
  sidebarWidth: 220,
  contextPanelWidth: 280,
  objectContentMaxWidth: 760,
  editorMaxWidth: 820,
  markdownEditorMinHeight: 420,
} as const;

export const graphThemes = {
  light: {
    background: [0.925, 0.902, 0.847] as const,
    grid: "rgba(17, 16, 13, 0.06)",
    arrow: "rgba(48, 44, 37, 0.5)",
    labelStroke: "rgba(246, 241, 230, 0.92)",
    label: "#5d574d",
    labelEmphasized: "#302c25",
    labelHovered: "#11100d",
    localLabel: "#5d574d",
    localLabelHovered: "#302c25",
    localLabelCurrent: "#11100d",
    edgeBase: [0.5, 0.5, 0.49] as const,
    edgeFocused: [0.3, 0.31, 0.32] as const,
    nodeNeutral: [0.31, 0.31, 0.3] as const,
    nodeConnected: [0.22, 0.23, 0.24] as const,
    nodeSelected: [0.13, 0.14, 0.15] as const,
    nodeRim: [0.8, 0.8, 0.78] as const,
    nodeRimConnected: [0.7, 0.71, 0.72] as const,
    nodeRimSelected: [0.62, 0.64, 0.66] as const,
    fieldLight: [1, 1, 1] as const,
  },
  dark: {
    background: [0.082, 0.082, 0.074] as const,
    grid: "rgba(220, 218, 208, 0.07)",
    arrow: "rgba(207, 204, 194, 0.46)",
    labelStroke: "rgba(21, 21, 19, 0.92)",
    label: "#b8b5ac",
    labelEmphasized: "#dedbd3",
    labelHovered: "#f1f0eb",
    localLabel: "#b8b5ac",
    localLabelHovered: "#dedbd3",
    localLabelCurrent: "#f1f0eb",
    edgeBase: [0.46, 0.46, 0.43] as const,
    edgeFocused: [0.72, 0.71, 0.67] as const,
    nodeNeutral: [0.46, 0.45, 0.42] as const,
    nodeConnected: [0.61, 0.6, 0.56] as const,
    nodeSelected: [0.82, 0.81, 0.76] as const,
    nodeRim: [0.28, 0.28, 0.26] as const,
    nodeRimConnected: [0.38, 0.39, 0.4] as const,
    nodeRimSelected: [0.5, 0.52, 0.54] as const,
    fieldLight: [0.08, 0.08, 0.07] as const,
  },
} as const;
