import type { CSSProperties } from "react";
import { useTheme } from "../styles/theme";

type Props = {
  variant?: "theme" | "on-light" | "on-dark";
  style?: CSSProperties;
};

export function KivalLogo({ variant = "theme", style }: Props) {
  const { resolvedTheme } = useTheme();
  const useDarkLogo = variant === "on-dark" || (variant === "theme" && resolvedTheme === "dark");

  return (
    <img
      src={useDarkLogo ? "/kival-logo-dark.svg" : "/kival-logo-light.svg"}
      alt="Kival"
      width={250}
      height={80}
      style={{ display: "block", width: "100%", height: "auto", ...style }}
    />
  );
}
