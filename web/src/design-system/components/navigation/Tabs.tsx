import type { CSSProperties, KeyboardEvent } from "react";

export type TabItem = string | { value: string; label: string; count?: number };

export interface TabsProps {
  /** chaines ou {value,label,count} */
  tabs: TabItem[];
  value?: string;
  onChange?: (v: string) => void;
  style?: CSSProperties;
}

const val = (t: TabItem) => (typeof t === "string" ? t : t.value);

export function Tabs({ tabs = [], value, onChange, style }: TabsProps) {
  const cur = value ?? (tabs[0] !== undefined ? val(tabs[0]) : undefined);
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const i = tabs.findIndex((t) => val(t) === cur);
    const step = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (!step || i < 0) return;
    e.preventDefault();
    const next = tabs[(i + step + tabs.length) % tabs.length];
    onChange?.(val(next));
    const btns = e.currentTarget.querySelectorAll<HTMLButtonElement>('[role="tab"]');
    btns[(i + step + tabs.length) % tabs.length]?.focus();
  };
  return (
    <div role="tablist" onKeyDown={onKey} style={{ display: "flex", gap: 22, borderBottom: "1px solid var(--border-subtle)", ...style }}>
      {tabs.map((t) => {
        const v = val(t);
        const l = typeof t === "string" ? t : t.label;
        const count = typeof t === "string" ? undefined : t.count;
        const a = v === cur;
        return (
          <button
            key={v}
            role="tab"
            aria-selected={a}
            tabIndex={a ? 0 : -1}
            type="button"
            onClick={() => onChange?.(v)}
            style={{ height: 38, padding: 0, background: "none", border: 0, borderBottom: "1px solid " + (a ? "var(--gray-950)" : "transparent"), marginBottom: -1, color: a ? "var(--text-strong)" : "var(--text-subtle)", font: "500 13px/1 var(--font-sans)", cursor: "pointer", display: "flex", alignItems: "center", gap: 6 }}
          >
            {l}
            {count != null && <span style={{ font: "400 11px/1 var(--font-sans)", color: "var(--text-faint)" }}>{count}</span>}
          </button>
        );
      })}
    </div>
  );
}
