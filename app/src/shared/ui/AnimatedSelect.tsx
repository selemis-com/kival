import type { CSSProperties, SelectHTMLAttributes } from "react";

type Props = SelectHTMLAttributes<HTMLSelectElement> & {
  wrapperStyle?: CSSProperties;
};

export const animatedSelectCss = `
.kival-select:open + .kival-select-caret {
  transform: rotate(180deg);
}

@media (prefers-reduced-motion: reduce) {
  .kival-select-caret {
    transition: none !important;
  }
}
`;

export function AnimatedSelect({
  children,
  className,
  disabled,
  style,
  wrapperStyle,
  ...props
}: Props) {
  return (
    <span
      style={{
        position: "relative",
        display: "inline-flex",
        ...wrapperStyle,
      }}
    >
      <select
        {...props}
        className={["kival-select", className].filter(Boolean).join(" ")}
        disabled={disabled}
        style={{
          ...style,
          width: style?.width ?? "100%",
          appearance: "none",
          paddingRight: 30,
        }}
      >
        {children}
      </select>
      <span
        className="kival-select-caret"
        aria-hidden="true"
        style={{
          position: "absolute",
          top: 0,
          right: 10,
          bottom: 0,
          display: "flex",
          alignItems: "center",
          opacity: disabled ? 0.45 : 0.75,
          pointerEvents: "none",
          transition: "transform 160ms ease",
        }}
      >
        <svg aria-hidden="true" width="12" height="8" viewBox="0 0 12 8">
          <path
            d="m1 1.5 5 5 5-5"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      </span>
    </span>
  );
}
