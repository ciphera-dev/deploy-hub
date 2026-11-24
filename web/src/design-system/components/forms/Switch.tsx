import type { CSSProperties } from "react";
import { Switch as UiSwitch } from "@/components/ui/switch";

export interface SwitchProps {
  checked?: boolean;
  onChange?: (v: boolean) => void;
  /** si fourni, rend une ligne de reglage : libelle + description a gauche, interrupteur a droite */
  label?: string;
  description?: string;
  /** nom accessible de l'interrupteur seul (sans `label` visible) */
  ariaLabel?: string;
  disabled?: boolean;
  style?: CSSProperties;
}

export function Switch({ checked = false, onChange, label, description, ariaLabel, disabled = false, style }: SwitchProps) {
  const sw = (
    <UiSwitch
      checked={checked}
      onCheckedChange={(v) => onChange?.(v)}
      disabled={disabled}
      aria-label={label ?? ariaLabel}
      style={{ width: 32, height: 18, flex: "none", borderRadius: 999, border: 0, padding: 2, boxShadow: "none", background: checked ? "var(--surface-inverse)" : "var(--surface-selected)", cursor: disabled ? "default" : "pointer", opacity: disabled ? 0.4 : 1, transition: "background var(--dur-base) var(--ease-out)" }}
      thumbStyle={{ display: "block", width: 14, height: 14, borderRadius: 99, background: checked ? "var(--text-inverse)" : "var(--gray-750)", translate: checked ? "14px" : "0", transition: "translate var(--dur-base) var(--ease-out)" }}
    />
  );
  if (!label) return sw;
  return (
    <div style={{ display: "flex", alignItems: "flex-start", justifyContent: "space-between", gap: 16, ...style }}>
      <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
        <span style={{ font: "500 13px/1.2 var(--font-sans)", color: "var(--text-strong)" }}>{label}</span>
        {description && <span style={{ font: "400 12px/1.4 var(--font-sans)", color: "var(--text-subtle)" }}>{description}</span>}
      </div>
      {sw}
    </div>
  );
}
