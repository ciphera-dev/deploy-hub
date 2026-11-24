import type { CSSProperties } from "react";

export interface SparkProps {
  points: number[];
  height?: number;
  style?: CSSProperties;
}

const W = 100;

/** Courbe compacte en SVG pur, largeur fluide (viewBox 100 x height). */
export function Spark({ points, height = 24, style }: SparkProps) {
  const n = points.length;
  const min = Math.min(...points);
  const max = Math.max(...points);
  const span = max - min || 1;
  const pad = 1.5;
  const d = points
    .map((v, i) => {
      const x = n > 1 ? (i / (n - 1)) * W : W / 2;
      const y = height - pad - ((v - min) / span) * (height - pad * 2);
      return (i === 0 ? "M" : "L") + x.toFixed(2) + "," + y.toFixed(2);
    })
    .join(" ");
  return (
    <svg viewBox={"0 0 " + W + " " + height} preserveAspectRatio="none" width="100%" height={height} role="img" aria-label="Tendance" style={{ display: "block", ...style }}>
      {n > 0 && <path d={d} fill="none" stroke="var(--viz-line)" strokeWidth="1.5" vectorEffect="non-scaling-stroke" strokeLinejoin="round" strokeLinecap="round" />}
    </svg>
  );
}
