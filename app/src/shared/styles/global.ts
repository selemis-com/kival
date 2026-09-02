export const globalCss = `
html,
body,
#root {
  width: 100%;
  min-width: 320px;
  min-height: 100%;
}

body {
  background: var(--kival-background);
  color: var(--kival-text);
  font-family: "Karla", ui-sans-serif, system-ui, sans-serif;
  user-select: text;
}

h1,
h2,
h3,
h4,
h5,
h6 {
  font-family: "Inter", ui-sans-serif, system-ui, sans-serif;
  font-weight: 500;
  letter-spacing: 0;
}

button,
a {
  touch-action: manipulation;
}

button,
input,
textarea,
select {
  border-radius: 8px;
  transition:
    border-color 160ms ease,
    background-color 160ms ease,
    color 160ms ease,
    opacity 160ms ease;
}

button:disabled,
input:disabled,
textarea:disabled,
select:disabled {
  cursor: not-allowed !important;
  opacity: 0.52;
}

:focus-visible {
  outline: 1px solid var(--kival-accent) !important;
  outline-offset: 3px;
}

.kival-markdown-textarea:focus-visible {
  outline: none !important;
  box-shadow: inset 0 0 0 1px var(--kival-accent);
}

.kival-markdown-tool {
  position: relative;
}

.kival-markdown-tool::after {
  content: attr(data-tooltip);
  position: absolute;
  z-index: 60;
  top: calc(100% + 7px);
  left: 50%;
  padding: 5px 7px;
  border: 1px solid var(--kival-border);
  border-radius: 6px;
  background: var(--kival-surface-strong);
  color: var(--kival-text-on-strong);
  font-family: "Karla", ui-sans-serif, system-ui, sans-serif;
  font-size: 11px;
  font-weight: 500;
  line-height: 1.2;
  white-space: nowrap;
  opacity: 0;
  pointer-events: none;
  transform: translate(-50%, -2px);
  transition:
    opacity 100ms ease 180ms,
    transform 100ms ease 180ms;
}

.kival-object-list::after {
  content: "";
  position: absolute;
  z-index: 10;
  inset: 0;
  border: 1px solid var(--kival-border);
  border-radius: 8px;
  pointer-events: none;
}

.kival-object-list > :first-child {
  border-radius: 8px 8px 0 0;
}

.kival-object-list > :last-child {
  border-radius: 0 0 8px 8px;
}

.kival-object-list > :only-child {
  border-radius: 8px;
}

.kival-row-list > :last-child {
  border-bottom: 0 !important;
}

.kival-markdown-tool:hover::after,
.kival-markdown-tool:focus-visible::after {
  opacity: 1;
  transform: translate(-50%, 0);
}

::selection {
  background: color-mix(in srgb, var(--kival-accent) 24%, transparent);
  color: var(--kival-text);
}

::-moz-selection {
  background: color-mix(in srgb, var(--kival-accent) 24%, transparent);
  color: var(--kival-text);
}

.kival-dark-panel ::selection {
  background: var(--kival-text-on-strong);
  color: var(--kival-surface-strong);
}

.kival-dark-panel ::-moz-selection {
  background: var(--kival-text-on-strong);
  color: var(--kival-surface-strong);
}

* {
  scrollbar-color: var(--kival-border-strong) transparent;
  scrollbar-width: thin;
}

.kival-loading-indicator {
  width: 100%;
  min-height: 96px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--kival-accent);
}

.kival-loading-indicator-compact {
  min-height: 44px;
}

.kival-loading-dots {
  display: inline-flex;
  align-items: center;
  gap: 7px;
}

.kival-loading-dots > span {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: currentColor;
  animation: kival-loading-pulse 1.15s ease-in-out infinite;
}

.kival-loading-dots > span:nth-child(2) {
  animation-delay: 140ms;
}

.kival-loading-dots > span:nth-child(3) {
  animation-delay: 280ms;
}

.kival-visually-hidden {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  border: 0;
}

@keyframes kival-loading-pulse {
  0%,
  70%,
  100% {
    opacity: 0.28;
    transform: scale(0.72);
  }

  35% {
    opacity: 1;
    transform: scale(1);
  }
}

@media (max-width: 760px) {
  .kival-login-page {
    padding: 16px !important;
  }

  .kival-login-shell {
    min-height: 0 !important;
    grid-template-columns: 1fr !important;
  }

  .kival-login-intro {
    min-height: 310px;
    padding: 28px !important;
  }

  .kival-login-panel {
    padding: 36px 28px !important;
  }
}

@media (prefers-reduced-motion: reduce) {
  *,
  ::before,
  ::after {
    scroll-behavior: auto !important;
    transition-duration: 0.01ms !important;
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
  }
}
`;
