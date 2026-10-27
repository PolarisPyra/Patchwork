import { useState } from "react";

/** The arc's lap time; matches the `.spinner-arc` animation in theme.css. */
const LAP_MS = 800;

/** A still ring with an arc that runs around it. Nothing rotates, so the ring keeps its pixels
    from frame to frame; a rotated icon is redrawn at each angle and looks like it shakes. */
export default function Spinner({ size = 12, className }: { size?: number; className?: string }) {
  // An animation starts when its spinner appears, so spinners shown at different times would run
  // out of step. Starting each partway into its lap, by the page's clock, keeps them all together.
  const [delay] = useState(() => `${-(performance.now() % LAP_MS)}ms`);
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={3}
      className={`flex-none ${className ?? ""}`}
    >
      <circle cx={12} cy={12} r={9} opacity={0.25} />
      <circle
        cx={12}
        cy={12}
        r={9}
        strokeLinecap="round"
        strokeDasharray="14.14 42.41"
        className="spinner-arc"
        style={{ animationDelay: delay }}
      />
    </svg>
  );
}
