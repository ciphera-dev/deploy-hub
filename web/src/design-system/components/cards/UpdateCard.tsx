import type { CSSProperties } from "react";

export interface UpdateCardProps {
  eyebrow?: string;
  title?: string;
  body?: string;
  cta?: string;
  href?: string;
  style?: CSSProperties;
}

export function UpdateCard({ eyebrow = "Mise à jour", title = "Nouveautés", body = "Derniers correctifs et nouvelles fonctions.", cta = "En savoir plus", href = "#", style }: UpdateCardProps) {
  return (
    <div style={{ padding: "14px 12px 12px", borderRadius: "var(--radius-lg)", background: "var(--gray-50)", border: "1px solid var(--border-default)", display: "flex", flexDirection: "column", gap: 7, ...style }}>
      <div style={{ font: "500 10px/1 var(--font-sans)", letterSpacing: ".06em", textTransform: "uppercase", color: "var(--text-faint)" }}>{eyebrow}</div>
      <div style={{ font: "500 12px/1.2 var(--font-sans)", color: "var(--text-strong)" }}>{title}</div>
      <div style={{ font: "400 10px/1.3 var(--font-sans)", color: "var(--text-muted)" }}>{body}</div>
      <a href={href} style={{ font: "400 11px/1 var(--font-sans)", color: "var(--text-body)", marginTop: 6 }}>{cta}</a>
    </div>
  );
}
