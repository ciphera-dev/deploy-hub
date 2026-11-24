import { useId, useState, type CSSProperties } from "react";

export interface TextFieldProps {
  label?: string;
  hint?: string;
  value?: string;
  defaultValue?: string;
  placeholder?: string;
  onChange?: (v: string) => void;
  /** Valeur en Geist Mono (URL, cles, IP) */
  mono?: boolean;
  type?: string;
  /** Indication au navigateur et aux gestionnaires de mots de passe (ex. "new-password") */
  autoComplete?: string;
  style?: CSSProperties;
}

export function TextField({ label, hint, value, defaultValue, placeholder, onChange, mono = false, type = "text", autoComplete, style }: TextFieldProps) {
  const [fc, setF] = useState(false);
  const id = useId();
  const labelId = id + "-label";
  const hintId = id + "-hint";
  return (
    <label style={{ display: "flex", flexDirection: "column", gap: 8, ...style }}>
      {label && <span id={labelId} style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-muted)" }}>{label}</span>}
      <input
        type={type}
        autoComplete={autoComplete}
        aria-labelledby={label ? labelId : undefined}
        aria-describedby={hint ? hintId : undefined}
        value={value}
        defaultValue={defaultValue}
        placeholder={placeholder}
        onChange={(e) => onChange?.(e.target.value)}
        onFocus={() => setF(true)}
        onBlur={() => setF(false)}
        style={{ height: 36, padding: "0 12px", borderRadius: "var(--radius-md)", background: "var(--surface-control)", border: "1px solid " + (fc ? "var(--gray-500)" : "var(--border-strong)"), boxShadow: "var(--shadow-control)", color: "var(--text-strong)", font: "400 13px/1 " + (mono ? "var(--font-mono)" : "var(--font-sans)"), outline: "none" }}
      />
      {hint && <span id={hintId} style={{ font: "400 11px/1.4 var(--font-sans)", color: "var(--text-faint)" }}>{hint}</span>}
    </label>
  );
}
