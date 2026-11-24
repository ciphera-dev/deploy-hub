import type { CSSProperties, ReactNode } from "react";

import { Icon, type IconName } from "@/design-system";
import { FormError } from "@/features/auth/ui";
import { ApiError } from "@/lib/api/errors";

// Styles de Pages2.jsx (SettingsV2) : libelles, filets, cartes.
export const lbl: CSSProperties = { font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" };
const line = "1px solid rgba(255,255,255,.06)";
export const CARD: CSSProperties = { borderRadius: 18, overflow: "hidden", border: "1px solid var(--border-default)", background: "var(--surface-app)" };

/** Formulaire de la section affichee : le bouton « Enregistrer » de l'en-tete le soumet. */
export const SECTION_FORM_ID = "settings-section-form";

/** Carte de reglages de SettingsV2 : titre, description, lignes. */
export function Card({ title, desc, children }: { title: string; desc?: string; children: ReactNode }) {
  return (
    <section style={CARD}>
      <div style={{ padding: "18px 22px", borderBottom: "1px solid var(--border-subtle)", display: "flex", flexDirection: "column", gap: 7 }}>
        <h2 style={{ margin: 0, font: "600 15px/1 var(--font-sans)", color: "var(--text-strong)" }}>{title}</h2>
        {desc && <span style={{ ...lbl, lineHeight: 1.4 }}>{desc}</span>}
      </div>
      {children}
    </section>
  );
}

/** Ligne de reglage de SettingsV2 : intitule et aide a gauche, controle a droite. */
export function Row({ t, d, first, children }: { t: string; d?: ReactNode; first?: boolean; children: ReactNode }) {
  return (
    <div style={{ display: "grid", gridTemplateColumns: "minmax(0,1fr) minmax(0,1.2fr)", gap: 24, alignItems: "center", padding: "18px 20px", borderTop: first ? "none" : line }}>
      <span style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        <span style={{ font: "500 13px/1.2 var(--font-sans)", color: "var(--text-strong)" }}>{t}</span>
        {d && <span style={{ font: "400 12px/1.45 var(--font-sans)", color: "var(--text-subtle)" }}>{d}</span>}
      </span>
      <div style={{ display: "flex", justifyContent: "flex-end", alignItems: "center", gap: 8, minWidth: 0, flexWrap: "wrap" }}>{children}</div>
    </div>
  );
}

interface InputProps {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  mono?: boolean;
  type?: string;
  autoComplete?: string;
}

/** Champ `inp` de SettingsV2 (38 px, fond translucide) ; le libelle de la ligne est son nom accessible. */
export function Input({ label, value, onChange, placeholder, mono, type = "text", autoComplete = "off" }: InputProps) {
  return (
    <input
      aria-label={label}
      type={type}
      value={value}
      autoComplete={autoComplete}
      spellCheck={false}
      onChange={(e) => onChange(e.target.value)}
      placeholder={placeholder}
      style={{ width: "100%", maxWidth: 340, height: 38, padding: "0 12px", boxSizing: "border-box", borderRadius: 10, border: "1px solid rgba(255,255,255,.1)", background: "rgba(255,255,255,.045)", color: "var(--text-strong)", font: "400 13px/1 " + (mono ? "var(--font-mono)" : "var(--font-sans)"), outline: "none" }}
    />
  );
}

/** Pastille d'etat (DNS, HTTPS) de SettingsV2. */
export function Pill({ tone, icon, children, mono }: { tone: "ok" | "error" | "muted"; icon: IconName; children: ReactNode; mono?: boolean }) {
  const bg = tone === "ok" ? "rgba(34,197,94,.12)" : tone === "error" ? "rgba(248,113,113,.12)" : "var(--surface-selected)";
  const color = tone === "ok" ? "var(--green-500)" : tone === "error" ? "#f87171" : "var(--text-muted)";
  return (
    <span style={{ display: "flex", alignItems: "center", gap: 6, height: 28, padding: "0 10px", borderRadius: 6, background: bg, font: "500 12px/1 " + (mono ? "var(--font-mono)" : "var(--font-sans)"), color }}>
      <Icon name={icon} size={12} />
      {children}
    </span>
  );
}

/** Mention « Disponible en v0.2 » d'un reglage porte mais pas encore actif. */
export function Soon() {
  return <span style={{ ...lbl, whiteSpace: "nowrap" }}>Disponible en v0.2</span>;
}

/** Encadre d'information (Pages2.jsx : fond releve, icone). */
export function Note({ icon = "Info", color = "var(--text-muted)", children }: { icon?: IconName; color?: string; children: ReactNode }) {
  return (
    <div style={{ display: "flex", alignItems: "flex-start", gap: 10, margin: "0 20px 16px", padding: "10px 12px", borderRadius: 10, background: "var(--surface-raised)", font: "400 12px/1.45 var(--font-sans)", color: "var(--text-muted)" }}>
      <Icon name={icon} size={14} color={color} style={{ marginTop: 1 }} />
      <span>{children}</span>
    </div>
  );
}

/** Chargement ou erreur de lecture d'une section (403 : message de role). */
export function SectionState({ error, what, denied }: { error: unknown; what: string; denied: string }) {
  if (error) {
    const status = error instanceof ApiError ? error.status : 0;
    return <FormError>{status === 403 || status === 401 ? denied : `Impossible de charger ${what}. Réessayez plus tard.`}</FormError>;
  }
  return (
    <div role="status" style={{ display: "flex", alignItems: "center", gap: 10, padding: 20, font: "400 13px/1 var(--font-sans)", color: "var(--text-muted)" }}>
      <Icon name="Loader" size={14} className="login-spin" />
      Chargement des paramètres…
    </div>
  );
}
