import type { CSSProperties } from "react";
import { CopyButton } from "../core/CopyButton";

export interface CodeBlockLine {
  text: string;
  /** colonne horodatage, ex. "12:04:31" */
  time?: string;
  tone?: "info" | "muted" | "ok" | "error" | "cmd";
}

export interface CodeBlockProps {
  /** texte brut (coupe sur les retours a la ligne) */
  code?: string;
  /** ou lignes structurees */
  lines?: CodeBlockLine[];
  copy?: boolean;
  maxHeight?: number | string;
  style?: CSSProperties;
}

const col = { info: "var(--text-body)", muted: "var(--text-faint)", ok: "var(--text-positive)", error: "var(--text-negative)", cmd: "var(--text-strong)" };

export function CodeBlock({ code = "", lines, copy = true, maxHeight, style }: CodeBlockProps) {
  const L: CodeBlockLine[] = lines ?? String(code).split("\n").map((t) => ({ text: t }));
  return (
    <div style={{ position: "relative", borderRadius: "var(--radius-lg)", background: "var(--gray-0)", border: "1px solid var(--border-default)", ...style }}>
      {copy && <CopyButton size={26} value={L.map((l) => l.text).join("\n")} style={{ position: "absolute", top: 8, right: 8 }} />}
      <pre style={{ margin: 0, padding: "14px 44px 14px 14px", overflow: "auto", maxHeight, font: "400 12px/1.7 var(--font-mono)" }}>
        {L.map((l, i) => (
          <div key={i} style={{ display: "flex", gap: 14, color: col[l.tone ?? "info"] }}>
            {l.time && <span style={{ color: "var(--text-faint)", flex: "none" }}>{l.time}</span>}
            <span style={{ whiteSpace: "pre-wrap", wordBreak: "break-all" }}>{l.text}</span>
          </div>
        ))}
      </pre>
    </div>
  );
}
