import { useId, useState, type ReactNode } from "react";

import { Icon, type IconName } from "@/design-system";

import "./setup.css";

/** Libelle secondaire de la maquette (12px, texte discret). */
export const lbl = { font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" } as const;

export interface SetupInputProps {
  label: string;
  value: string;
  onChange: (value: string) => void;
  type?: string;
  placeholder?: string;
  hint?: string;
  autoFocus?: boolean;
}

/** Champ de saisie de l'assistant (42 px, halo violet au focus), porte de install.html. */
export function SetupInput({ label, value, onChange, type = "text", placeholder, hint, autoFocus }: SetupInputProps) {
  const [focused, setFocused] = useState(false);
  const id = useId();
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <label htmlFor={id} style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>
        {label}
      </label>
      <input
        id={id}
        aria-describedby={hint ? id + "-hint" : undefined}
        className="setup-field"
        autoFocus={autoFocus}
        type={type}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onFocus={() => setFocused(true)}
        onBlur={() => setFocused(false)}
        placeholder={placeholder}
        style={{
          height: 42,
          padding: "0 12px",
          borderRadius: 10,
          border: "1px solid " + (focused ? "rgba(167,139,250,.7)" : "var(--border-default)"),
          background: "var(--surface-raised)",
          boxShadow: focused ? "0 0 0 3px rgba(139,124,246,.15)" : "none",
          color: "var(--text-strong)",
          font: "400 14px/1 var(--font-sans)",
          outline: "none",
        }}
      />
      {hint && (
        <span id={id + "-hint"} style={{ ...lbl, fontSize: 11 }}>
          {hint}
        </span>
      )}
    </div>
  );
}

export interface SetupButtonProps {
  children: ReactNode;
  onClick: () => void;
  disabled?: boolean;
  ghost?: boolean;
  icon?: IconName;
}

/** Bouton de l'assistant (42 px) : plein violet, ou contour pour « Retour ». */
export function SetupButton({ children, onClick, disabled, ghost, icon }: SetupButtonProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        gap: 8,
        height: 42,
        padding: "0 18px",
        borderRadius: 10,
        cursor: disabled ? "default" : "pointer",
        border: ghost ? "1px solid var(--border-default)" : 0,
        background: ghost ? "transparent" : disabled ? "rgba(139,124,246,.4)" : "#8b7cf6",
        color: ghost ? "var(--text-strong)" : "#fff",
        font: "600 14px/1 var(--font-sans)",
      }}
    >
      {children}
      {icon && <Icon name={icon} size={15} color={ghost ? "var(--text-strong)" : "#fff"} />}
    </button>
  );
}

/** Titre + sous-titre d'une etape. */
export function StepHeading({ title, subtitle }: { title: string; subtitle: string }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8, marginBottom: 6 }}>
      <h1 style={{ margin: 0, font: "600 24px/1.2 var(--font-sans)", color: "var(--text-strong)", letterSpacing: "-0.01em" }}>{title}</h1>
      <span style={{ font: "400 14px/1.5 var(--font-sans)", color: "var(--text-muted)" }}>{subtitle}</span>
    </div>
  );
}

/** Carte a lignes separees (verifications, finalisation). */
export function RowCard({ children }: { children: ReactNode }) {
  return <div style={{ borderRadius: "var(--radius-xl)", border: "1px solid var(--border-default)", background: "var(--surface-app)", overflow: "hidden" }}>{children}</div>;
}

export function Spinner({ size = 16 }: { size?: number }) {
  return <Icon name="Loader" size={size} color="var(--text-muted)" className="setup-spin" />;
}
