/** A still ring with an arc that runs around it. Nothing rotates, so the ring keeps its pixels
    from frame to frame; a rotated icon is redrawn at each angle and looks like it shakes. */
export default function Spinner({ size = 12, className }: { size?: number; className?: string }) {
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
      />
    </svg>
  );
}
