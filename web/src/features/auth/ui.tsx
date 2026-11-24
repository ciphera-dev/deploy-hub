import { useId, useState, type ReactNode } from "react";

import { Icon, type IconName } from "@/design-system";

import "./login.css";

/** Autre methode de verification (bouton liste de 2fa.html). */
export function AltMethod({ icon, title, text, onClick }: { icon: IconName; title: string; text: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="login-alt"
      style={{ display: "flex", alignItems: "center", gap: 12, width: "100%", padding: "12px 14px", borderRadius: 12, border: "1px solid var(--border-default)", cursor: "pointer", textAlign: "left" }}
    >
      <span style={{ width: 34, height: 34, borderRadius: 9, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", background: "var(--surface-selected)" }}>
        <Icon name={icon} size={15} color="var(--text-strong)" />
      </span>
      <span style={{ display: "flex", flexDirection: "column", gap: 5, flex: 1 }}>
        <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>{title}</span>
        <span style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" }}>{text}</span>
      </span>
      <Icon name="ChevronRight" size={14} color="var(--text-subtle)" />
    </button>
  );
}

/** Separateur « Autres méthodes » de 2fa.html. */
export function MethodsDivider({ children = "Autres méthodes" }: { children?: ReactNode }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 14, font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)", marginTop: 4 }}>
      <span style={{ flex: 1, height: 1, background: "var(--border-subtle)" }} />
      {children}
      <span style={{ flex: 1, height: 1, background: "var(--border-subtle)" }} />
    </div>
  );
}

/** Titre d'etape (24 px, centre), porte de login.html. */
export function StepTitle({ children }: { children: ReactNode }) {
  return <h1 style={{ margin: 0, font: "600 24px/1.2 var(--font-sans)", color: "var(--text-strong)", letterSpacing: "-0.01em", textAlign: "center" }}>{children}</h1>;
}

export interface LoginInputProps {
  label: string;
  value: string;
  onChange: (value: string) => void;
  type?: string;
  autoComplete?: string;
  autoFocus?: boolean;
  error?: boolean;
  right?: ReactNode;
}

/** Champ de la page de connexion (42 px, halo violet au focus, bord rouge en erreur). */
export function LoginInput({ label, value, onChange, type = "text", autoComplete, autoFocus, error, right }: LoginInputProps) {
  const [focused, setFocused] = useState(false);
  const id = useId();
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <label htmlFor={id} style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>
        {label}
      </label>
      <span
        style={{
          display: "flex",
          alignItems: "center",
          height: 42,
          padding: "0 12px",
          borderRadius: 10,
          border: "1px solid " + (error ? "rgba(248,113,113,.6)" : focused ? "rgba(167,139,250,.7)" : "var(--border-default)"),
          background: "var(--surface-raised)",
          boxShadow: focused ? "0 0 0 3px rgba(139,124,246,.15)" : "none",
          transition: "all .15s",
        }}
      >
        <input
          id={id}
          className="login-field"
          autoFocus={autoFocus}
          type={type}
          value={value}
          autoComplete={autoComplete}
          aria-invalid={error || undefined}
          onChange={(e) => onChange(e.target.value)}
          onFocus={() => setFocused(true)}
          onBlur={() => setFocused(false)}
          style={{ flex: 1, minWidth: 0, background: "transparent", border: 0, outline: "none", color: "var(--text-strong)", font: "400 14px/1 var(--font-sans)" }}
        />
        {right}
      </span>
    </div>
  );
}

export interface PrimaryButtonProps {
  children: ReactNode;
  busy?: boolean;
  disabled?: boolean;
  onClick?: () => void;
  type?: "submit" | "button";
}

/** Bouton principal (44 px, violet), indicateur de chargement pendant la requete. */
export function PrimaryButton({ children, busy, disabled, onClick, type = "submit" }: PrimaryButtonProps) {
  const [hover, setHover] = useState(false);
  const off = busy || disabled;
  return (
    <button
      type={type}
      onClick={onClick}
      disabled={off}
      aria-busy={busy || undefined}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        gap: 8,
        height: 44,
        width: "100%",
        borderRadius: 11,
        border: 0,
        cursor: off ? "default" : "pointer",
        background: disabled ? "rgba(139,124,246,.4)" : hover && !busy ? "#7c6cf0" : "#8b7cf6",
        color: "#fff",
        font: "600 14px/1 var(--font-sans)",
        transition: "background .15s",
      }}
    >
      {busy && <Icon name="Loader" size={15} color="#fff" className="login-spin" />}
      {children}
    </button>
  );
}

/** Message d'erreur sous un formulaire (annonce aux lecteurs d'ecran). */
export function FormError({ children }: { children: ReactNode }) {
  return (
    <span role="alert" style={{ display: "flex", alignItems: "center", gap: 6, font: "400 12px/1.3 var(--font-sans)", color: "#f87171" }}>
      <Icon name="CircleX" size={13} />
      {children}
    </span>
  );
}
