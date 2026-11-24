import type { CSSProperties, ReactNode } from "react";

const C = { ok: "var(--green-500)", error: "var(--red-500)", pending: "var(--viz-1)", off: "var(--viz-3)" };

export interface StatusDotProps {
  /** ok = vert, error = rouge, pending = gris, off = attenue */
  status?: "ok" | "error" | "pending" | "off";
  label?: ReactNode;
  style?: CSSProperties;
}

export function StatusDot({ status = "ok", label, style }: StatusDotProps) {
  return (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 8, font: "400 13px/1 var(--font-sans)", color: "var(--text-body)", ...style }}>
      <span style={{ width: 7, height: 7, borderRadius: 99, background: C[status] ?? C.off, flex: "none" }} />
      {label}
    </span>
  );
}
