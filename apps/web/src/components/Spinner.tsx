import { useLayoutEffect, useRef } from "react";

function pin(arc: SVGCircleElement) {
  for (const animation of arc.getAnimations()) animation.startTime = 0;
}

/** A still ring with an arc that runs around it. Nothing rotates, so the ring keeps its pixels
    from frame to frame; a rotated icon is redrawn at each angle and looks like it shakes. */
export default function Spinner({ size = 12, className }: { size?: number; className?: string }) {
  // An animation starts when its spinner first paints, or shows again after being hidden, so
  // spinners shown at different times would run out of step. Every arc's lap is counted from the
  // page's start instead, which keeps them all together however late each one appears.
  const arc = useRef<SVGCircleElement>(null);
  useLayoutEffect(() => {
    if (arc.current) pin(arc.current);
  }, []);
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={3}
      className={`flex-none ${className ?? ""}`}
      role="img"
      aria-label="Loading"
    >
      <circle cx={12} cy={12} r={9} opacity={0.25} />
      <circle
        ref={arc}
        cx={12}
        cy={12}
        r={9}
        strokeLinecap="round"
        strokeDasharray="14.14 42.41"
        className="spinner-arc"
        onAnimationStart={(e) => pin(e.currentTarget)}
      />
    </svg>
  );
}
