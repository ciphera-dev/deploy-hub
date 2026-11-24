import { useState, type CSSProperties, type ReactNode } from "react";

export interface Column<T> {
  key: keyof T & string;
  label: string;
  /** piste de grille CSS, minmax(0,1fr) par defaut */
  width?: string;
  align?: "left" | "right";
  render?: (value: T[keyof T & string], row: T) => ReactNode;
}

export interface DataTableProps<T> {
  columns: Column<T>[];
  rows: T[];
  onRowClick?: (row: T) => void;
  /** affiche quand rows est vide (et pas en chargement) */
  empty?: ReactNode;
  /** affiche 5 lignes squelette */
  loading?: boolean;
  style?: CSSProperties;
}

const SKELETON_ROWS = 5;

export function DataTable<T>({ columns = [], rows = [], onRowClick, empty, loading = false, style }: DataTableProps<T>) {
  const [h, setH] = useState(-1);
  const tmpl = columns.map((c) => c.width || "minmax(0,1fr)").join(" ");
  const cell = (c: Column<T>): CSSProperties => ({ textAlign: c.align || "left", justifySelf: c.align === "right" ? "end" : "start", minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" });
  const rowBase: CSSProperties = { display: "grid", gridTemplateColumns: tmpl, gap: 16, alignItems: "center", minHeight: 48, padding: "0 12px", borderBottom: "1px solid var(--border-subtle)" };
  return (
    <div role="table" aria-busy={loading || undefined} style={{ display: "flex", flexDirection: "column", ...style }}>
      <div role="row" style={{ display: "grid", gridTemplateColumns: tmpl, gap: 16, padding: "0 12px 12px", borderBottom: "1px solid var(--border-subtle)", font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" }}>
        {columns.map((c) => (
          <div key={c.key} role="columnheader" style={cell(c)}>{c.label}</div>
        ))}
      </div>
      {loading &&
        Array.from({ length: SKELETON_ROWS }, (_, i) => (
          <div role="row" key={"sk" + i} data-skeleton="true" aria-hidden="true" style={rowBase}>
            {columns.map((c) => (
              <div key={c.key} style={{ height: 12, width: "60%", borderRadius: 6, background: "var(--surface-selected)", opacity: 0.6, justifySelf: c.align === "right" ? "end" : "start" }} />
            ))}
          </div>
        ))}
      {!loading && rows.length === 0 && empty != null && (
        <div style={{ padding: "32px 12px", textAlign: "center", font: "400 13px/1.4 var(--font-sans)", color: "var(--text-subtle)" }}>{empty}</div>
      )}
      {!loading &&
        rows.map((r, i) => (
          <div
            role="row"
            key={i}
            onClick={onRowClick ? () => onRowClick(r) : undefined}
            onMouseEnter={() => setH(i)}
            onMouseLeave={() => setH(-1)}
            style={{ ...rowBase, background: h === i ? "var(--surface-raised)" : "transparent", cursor: onRowClick ? "pointer" : "default", font: "400 13px/1.2 var(--font-sans)", color: "var(--text-body)", transition: "background var(--dur-fast)" }}
          >
            {columns.map((c, j) => (
              <div key={c.key} role="cell" style={{ ...cell(c), color: j === 0 ? "var(--text-strong)" : undefined }}>
                {c.render ? c.render(r[c.key], r) : (r[c.key] as ReactNode)}
              </div>
            ))}
          </div>
        ))}
    </div>
  );
}
