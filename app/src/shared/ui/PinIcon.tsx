type Props = {
  size?: number;
  active?: boolean;
};

export function PinIcon({ size = 16, active = false }: Props) {
  return (
    <svg width={size} height={size} viewBox="0 0 20 20" fill="none" aria-hidden="true">
      <path
        d="M12.47 2.86a1.2 1.2 0 0 1 1.72-.1l3.05 3.05a1.2 1.2 0 0 1-.1 1.72l-3.22 2.5a2.56 2.56 0 0 0-.98 2.01v1.64c0 .7-.4 1.34-1.03 1.65a1.83 1.83 0 0 1-2.09-.34l-4.81-4.81a1.83 1.83 0 0 1-.34-2.09c.31-.63.95-1.03 1.65-1.03h1.64c.79 0 1.53-.36 2.01-.98l2.5-3.22Z"
        fill={active ? "currentColor" : "none"}
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinejoin="round"
      />
      <path
        d="m6.65 13.35-4.15 4.15"
        stroke="currentColor"
        strokeWidth="1.35"
        strokeLinecap="round"
      />
    </svg>
  );
}
