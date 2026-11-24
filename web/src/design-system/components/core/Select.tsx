import type { CSSProperties } from "react";
import { Select as UiSelect, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Icon, type IconName } from "./Icon";

export interface SelectProps {
  options: string[];
  value?: string;
  onChange?: (v: string) => void;
  /** icone lucide en tete (ex. "Calendar") */
  icon?: IconName;
  /** sans bordure, pour l'usage dans un ButtonGroup */
  bare?: boolean;
  style?: CSSProperties;
}

export function Select({ options = [], value, onChange, icon, bare = false, style }: SelectProps) {
  const cur = value ?? options[0] ?? "";
  const full = Boolean(style && style.width);
  return (
    <div style={{ position: "relative", display: "inline-flex", ...style }}>
      <UiSelect value={cur} onValueChange={(v) => onChange?.(v)}>
        <SelectTrigger
          className="hover:!bg-[var(--surface-control-hover)] [&>svg:last-child]:!size-[13px] [&>svg:last-child]:!opacity-100 [&>svg:last-child]:!text-[color:var(--text-muted)]"
          style={{
            display: "inline-flex",
            alignItems: "center",
            justifyContent: "flex-start",
            gap: 7,
            width: full ? "100%" : undefined,
            height: style?.height ?? 32,
            padding: "0 10px",
            borderRadius: "var(--radius-md)",
            background: bare ? "transparent" : "var(--surface-control)",
            border: bare ? "1px solid transparent" : "1px solid var(--border-strong)",
            boxShadow: bare ? "none" : "var(--shadow-control)",
            color: "var(--text-body)",
            font: "500 13px/1 var(--font-sans)",
            cursor: "pointer",
            whiteSpace: "nowrap",
          }}
        >
          {icon && <Icon name={icon} size={14} className="size-[14px]" />}
          <span style={{ flex: full ? 1 : undefined, textAlign: "left" }}>
            <SelectValue />
          </span>
        </SelectTrigger>
        <SelectContent
          position="popper"
          sideOffset={4}
          style={{ zIndex: 1000, maxHeight: 280, padding: 4, background: "var(--surface-tooltip)", border: 0, borderRadius: "var(--radius-lg)", boxShadow: "var(--shadow-tooltip)", color: "var(--text-body)" }}
        >
          {options.map((o) => (
            <SelectItem
              key={o}
              value={o}
              className="data-[highlighted]:!bg-[var(--surface-control-hover)]"
              style={{ padding: "7px 8px", borderRadius: 6, font: "400 13px/1.2 var(--font-sans)", color: o === cur ? "var(--text-strong)" : "var(--text-muted)", background: o === cur ? "var(--surface-selected)" : "transparent", cursor: "pointer", whiteSpace: "nowrap" }}
            >
              {o}
            </SelectItem>
          ))}
        </SelectContent>
      </UiSelect>
    </div>
  );
}
