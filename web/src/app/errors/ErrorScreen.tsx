import type { ReactNode } from "react";

import { Icon, type IconName } from "@/design-system";

export interface ErrorScreenProps {
  icon: IconName;
  title: string;
  description: string;
  /** Petit code affiche au-dessus du titre (ex. « 500 ») */
  code?: string;
  children?: ReactNode;
}

/** Squelette commun des ecrans d'erreur : icone, titre, texte, actions. */
export function ErrorScreen({ icon, title, description, code, children }: ErrorScreenProps) {
  return (
    <div role="alert" style={{ display: "flex", justifyContent: "center", padding: "80px 20px" }}>
      <div style={{ width: "100%", maxWidth: 460, display: "flex", flexDirection: "column", alignItems: "center", textAlign: "center", gap: 16 }}>
        <span style={{ width: 48, height: 48, borderRadius: 14, display: "flex", alignItems: "center", justifyContent: "center", background: "var(--surface-selected)", border: "1px solid var(--border-default)" }}>
          <Icon name={icon} size={22} color="var(--text-strong)" />
        </span>
        {code && <span style={{ font: "500 12px/1 var(--font-mono)", color: "var(--text-subtle)", letterSpacing: ".08em" }}>{code}</span>}
        <h1 style={{ margin: 0, font: "600 24px/1.2 var(--font-sans)", color: "var(--text-strong)", letterSpacing: "-0.01em" }}>{title}</h1>
        <p style={{ margin: 0, font: "400 14px/1.55 var(--font-sans)", color: "var(--text-muted)", maxWidth: 380 }}>{description}</p>
        {children && <div style={{ display: "flex", gap: 8, marginTop: 6, flexWrap: "wrap", justifyContent: "center" }}>{children}</div>}
      </div>
    </div>
  );
}
