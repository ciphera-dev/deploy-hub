import type { CSSProperties, ReactNode } from "react";

import { Icon, type IconName } from "@/design-system";

// Briques communes des maquettes du Profil (window.ProfileUI de
// ProfileSecurity.jsx, Card et Row de ProfilePage.jsx).

export const hair = "1px solid var(--border-subtle)";
export const card: CSSProperties = { borderRadius: 16, border: "1px solid var(--border-default)", background: "var(--surface-app)", overflow: "hidden" };
const tileBackground: CSSProperties = { background: "linear-gradient(180deg,#232327,#141417)", border: "1px solid rgba(255,255,255,.08)", boxShadow: "inset 0 1px 0 rgba(255,255,255,.07), 0 1px 2px rgba(0,0,0,.4)" };

/** Pastille d'icone en relief. */
export function Tile({ icon, size = 36 }: { icon: IconName; size?: number }) {
  return (
    <span style={{ width: size, height: size, borderRadius: size * 0.28, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", ...tileBackground }}>
      <Icon name={icon} size={size * 0.42} color="#ededed" />
    </span>
  );
}

/** Etat « Active » (vert) ou « Inactive » (ambre). */
export function Pill({ ok, children }: { ok: boolean; children: ReactNode }) {
  return (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 6, height: 22, padding: "0 9px", borderRadius: 99, background: ok ? "rgba(34,192,138,.12)" : "rgba(229,180,84,.12)", color: ok ? "var(--green-500)" : "#e5b454", font: "500 11px/1 var(--font-sans)", whiteSpace: "nowrap" }}>
      <span style={{ width: 6, height: 6, borderRadius: 99, background: "currentColor" }} />
      {children}
    </span>
  );
}

/** Titre de section : intitule, description, action a droite. */
export function Title({ t, d, right }: { t: string; d?: string; right?: ReactNode }) {
  return (
    <div style={{ display: "flex", alignItems: "flex-end", gap: 12, padding: "0 2px" }}>
      <span style={{ flex: 1, display: "flex", flexDirection: "column", gap: 6 }}>
        <h2 style={{ margin: 0, font: "600 15px/1 var(--font-sans)", color: "var(--text-strong)" }}>{t}</h2>
        {d && <span style={{ font: "400 12px/1.3 var(--font-sans)", color: "var(--text-subtle)" }}>{d}</span>}
      </span>
      {right}
    </div>
  );
}

/** Carte titree de ProfilePage.jsx. */
export function Card({ icon, title, desc, children }: { icon: IconName; title: string; desc?: string; children: ReactNode }) {
  return (
    <section style={{ ...card, position: "relative" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "16px 20px", borderBottom: hair }}>
        <span style={{ width: 34, height: 34, borderRadius: 10, display: "flex", alignItems: "center", justifyContent: "center", flex: "none", ...tileBackground }}>
          <Icon name={icon} size={16} color="#ededed" />
        </span>
        <span style={{ display: "flex", flexDirection: "column", gap: 6, flex: 1, minWidth: 0 }}>
          <h2 style={{ margin: 0, font: "600 15px/1 var(--font-sans)", color: "var(--text-strong)" }}>{title}</h2>
          {desc && <span style={{ font: "400 12px/1.3 var(--font-sans)", color: "var(--text-subtle)" }}>{desc}</span>}
        </span>
      </div>
      {children}
    </section>
  );
}

/** Message d'erreur annonce aux lecteurs d'ecran. */
export function Alert({ children }: { children: string }) {
  return (
    <span role="alert" style={{ display: "flex", alignItems: "center", gap: 6, font: "400 12px/1.3 var(--font-sans)", color: "var(--text-negative)" }}>
      <Icon name="CircleX" size={13} />
      {children}
    </span>
  );
}
