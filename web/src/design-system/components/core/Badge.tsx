import type { CSSProperties, ReactNode } from "react";

export interface BadgeProps {
  children: ReactNode;
  /** neutral (defaut, comme "Termine") | positive | negative */
  tone?: "neutral" | "positive" | "negative";
  style?: CSSProperties;
}

const TONES: Record<string, [string, string]> = {
  neutral: ["var(--surface-selected)", "var(--text-strong)"],
  positive: ["var(--green-900)", "var(--text-positive)"],
  negative: ["var(--red-900)", "var(--text-negative)"],
};

export function Badge({ children, tone = "neutral", style }: BadgeProps) {
  const t = TONES[tone] ?? [];
  return (
    <span style={{ display: "inline-flex", alignItems: "center", height: 20, padding: "0 8px", borderRadius: "var(--radius-sm)", background: t[0], color: t[1], font: "500 11px/1 var(--font-sans)", whiteSpace: "nowrap", ...style }}>
      {children}
    </span>
  );
}
