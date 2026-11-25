import { useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";

import { Button, CodeBlock, Icon, Select, StatusDot, type CodeBlockLine } from "@/design-system";

import { copyText } from "@/lib/clipboard";

import { useLogStream, type LogLevel, type LogLine, type LogSource } from "./useLogStream";
import "./logs.css";

// Port de prototypes/dashboard/ProjectLogs.jsx (structure, libellés, tokens).
// Écarts : les lignes viennent du WebSocket (les tirages Math.random de la
// maquette sont devenus des fixtures de test) ; un conteneur par service du
// projet, sans CPU ni mémoire (métriques avec la Task 31) ; la période filtre
// les lignes reçues. Variante `deployment` : le panneau « Logs de build » de
// ProjectDeployments.jsx (CodeBlock), alimenté par le flux du déploiement.

const hair = "1px solid var(--border-subtle)";
const LV: Record<LogLevel, { c: string; bg: string; t: string }> = {
  info: { c: "var(--text-muted)", bg: "transparent", t: "INFO" },
  warn: { c: "#e5b454", bg: "rgba(229,180,84,.10)", t: "WARN" },
  error: { c: "#f87171", bg: "rgba(239,68,68,.12)", t: "ERR" },
  debug: { c: "var(--text-faint)", bg: "transparent", t: "DBG" },
};
/** Couleurs des conteneurs de la maquette, dans l'ordre. */
export const CONTAINER_COLORS = ["#b5b5b5", "#8fa3b8", "#7fae99", "#a395c4", "#6f6f6f"];
const RANGES: [string, number][] = [
  ["5 dernières minutes", 5],
  ["15 dernières minutes", 15],
  ["1 dernière heure", 60],
  ["24 dernières heures", 1440],
  ["7 derniers jours", 10080],
];
const ALL = "all";

/** Conteneur de la colonne de gauche : un par service du projet. */
export interface LogContainer {
  /** nom affiché (nom du service) */
  id: string;
  /** identifiant du service (cible du flux) */
  service: string;
  /** image servie, ou « — » */
  img: string;
  /** ok : un déploiement sert le service ; off : arrêté */
  st: "ok" | "off";
  col: string;
  /** CPU (%) et mémoire (octets) mesurés par l'agent ; absents sans mesure récente */
  cpu?: number | null;
  mem?: number | null;
}

/** « 212 Mo », « 1,1 Go » (ProjectLogs.jsx `mem`). */
export function formatMem(bytes: number): string {
  const mo = bytes / (1024 * 1024);
  return mo >= 1024 ? `${(mo / 1024).toLocaleString("fr-FR", { maximumFractionDigits: 1 })} Go` : `${Math.round(mo)} Mo`;
}

const line = (l: LogLine) => `${l.date} [${LV[l.lv].t}] ${l.c} | ${l.msg}`;

function Histogram({ logs }: { logs: LogLine[] }) {
  const B = 60;
  if (!logs.length) return <div style={{ height: 44 }}></div>;
  const t0 = logs[0]!.t;
  const t1 = logs[logs.length - 1]!.t || t0 + 1;
  const span = Math.max(1, t1 - t0);
  const bins = Array.from({ length: B }, () => ({ n: 0, e: 0, w: 0 }));
  logs.forEach((l) => {
    const b = bins[Math.min(B - 1, Math.max(0, Math.floor(((l.t - t0) / span) * B)))]!;
    b.n++;
    if (l.lv === "error") b.e++;
    if (l.lv === "warn") b.w++;
  });
  const max = Math.max(1, ...bins.map((b) => b.n));
  return (
    <div style={{ display: "flex", alignItems: "flex-end", gap: 2, height: 44 }}>
      {bins.map((b, i) => (
        <div key={i} title={b.n + " lignes"} style={{ flex: 1, height: Math.max(2, (b.n / max) * 44), display: "flex", flexDirection: "column", justifyContent: "flex-start", borderRadius: 2, overflow: "hidden", background: "var(--viz-3)" }}>
          {b.e > 0 && <span style={{ height: (b.e / b.n) * 100 + "%", background: "var(--red-500)", minHeight: 2 }}></span>}
          {b.w > 0 && <span style={{ height: (b.w / b.n) * 100 + "%", background: "#e5b454", minHeight: 2 }}></span>}
        </div>
      ))}
    </div>
  );
}

function LogRow({ l, i, showC, wrap, ts, hl, col, onCopy }: { l: LogLine; i: number; showC: boolean; wrap: boolean; ts: boolean; hl: (t: string) => ReactNode; col?: string; onCopy: (l: LogLine) => void }) {
  const [h, setH] = useState(false);
  const L = LV[l.lv];
  return (
    <div data-testid="log-row" onMouseEnter={() => setH(true)} onMouseLeave={() => setH(false)} style={{ position: "relative", display: "flex", alignItems: "flex-start", gap: 14, padding: "3px 16px 3px 0", minHeight: 24, background: h ? "var(--surface-raised)" : l.lv === "error" ? "rgba(239,68,68,.05)" : "transparent", boxShadow: l.lv === "error" ? "inset 2px 0 0 #ef4444" : l.lv === "warn" ? "inset 2px 0 0 #e5b454" : "none" }}>
      <span style={{ width: 44, flex: "none", textAlign: "right", color: "var(--gray-500)", userSelect: "none" }}>{i + 1}</span>
      {ts && <span style={{ color: "var(--text-faint)", flex: "none" }}>{l.ts}</span>}
      <span style={{ flex: "none", width: 36, height: 18, display: "flex", alignItems: "center", justifyContent: "center", borderRadius: 4, background: L.bg, color: L.c, fontSize: 10.5, fontWeight: 500, letterSpacing: ".03em", marginTop: 1 }}>{L.t}</span>
      {showC && (
        <span style={{ flex: "none", width: 84, display: "flex", alignItems: "center", gap: 6, color: "var(--text-muted)", overflow: "hidden", whiteSpace: "nowrap" }}>
          <span style={{ width: 6, height: 6, borderRadius: 2, background: col, flex: "none" }}></span>
          {l.c}
        </span>
      )}
      <span data-testid="log-message" style={{ flex: 1, minWidth: 0, color: l.lv === "error" ? "#fca5a5" : l.lv === "debug" ? "var(--text-subtle)" : "var(--text-body)", whiteSpace: wrap ? "pre-wrap" : "pre", overflow: wrap ? "visible" : "hidden", textOverflow: "ellipsis", wordBreak: wrap ? "break-all" : "normal" }}>
        {hl(l.msg)}
      </span>
      {h && (
        <button onClick={() => onCopy(l)} title="Copier la ligne" style={{ position: "absolute", right: 10, top: 2, width: 22, height: 22, display: "flex", alignItems: "center", justifyContent: "center", borderRadius: 5, border: "1px solid var(--border-strong)", background: "var(--surface-control)", color: "var(--text-muted)", cursor: "pointer", padding: 0 }}>
          <Icon name="Copy" size={11} />
        </button>
      )}
    </div>
  );
}

function useFlash() {
  const [toast, setToast] = useState("");
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const flash = (m: string) => {
    setToast(m);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setToast(""), 1800);
  };
  return { toast, flash };
}

function saveFile(href: string, name?: string) {
  const a = document.createElement("a");
  a.href = href;
  if (name) a.download = name;
  document.body.appendChild(a);
  a.click();
  a.remove();
}

function ProjectLogViewer({ project, containers }: { project: string; containers: LogContainer[] }) {
  const [sel, setSel] = useState(ALL);
  const [lv, setLv] = useState<"all" | LogLevel>("all");
  const [q, setQ] = useState("");
  const [live, setLive] = useState(true);
  const [wrap, setWrap] = useState(false);
  const [ts, setTs] = useState(true);
  const [range, setRange] = useState("15 dernières minutes");
  const { toast, flash } = useFlash();
  const [atBottom, setAtBottom] = useState(true);
  const box = useRef<HTMLDivElement>(null);
  const selC = containers.find((c) => c.id === sel);
  const sources = useMemo<LogSource[]>(() => containers.filter((c) => c.st === "ok").map((c) => ({ service: c.service, tail: 500 })), [containers]);
  const { lines, clear, reconnecting } = useLogStream(sources);
  // En pause : affichage figé sur la dernière ligne reçue ; la suite reste gardée et s'affiche à la reprise.
  const [frozen, setFrozen] = useState<number | null>(null);
  const logs = frozen === null ? lines : lines.filter((l) => l.id <= frozen);
  const toggleLive = () => {
    setFrozen(live ? (lines[lines.length - 1]?.id ?? 0) : null);
    setLive(!live);
  };
  // Période : les lignes plus anciennes que l'heure courante moins la période sont masquées.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 15_000);
    return () => clearInterval(t);
  }, []);
  const minutes = RANGES.find(([n]) => n === range)?.[1] ?? 15;
  const since = now - minutes * 60_000;
  const scoped = logs.filter((l) => (sel === ALL || l.c === sel) && l.t >= since);
  const shown = scoped.filter((l) => (lv === "all" ? l.lv !== "debug" : l.lv === lv) && (!q || (l.msg + l.c).toLowerCase().includes(q.toLowerCase())));
  useLayoutEffect(() => {
    const b = box.current;
    if (b && atBottom) b.scrollTop = b.scrollHeight;
  }, [shown.length, atBottom, sel, lv]);
  const onScroll = () => {
    const b = box.current;
    if (b) setAtBottom(b.scrollHeight - b.scrollTop - b.clientHeight < 24);
  };
  const download = () => {
    const blob = new Blob([shown.map(line).join("\n") + "\n"], { type: "text/plain;charset=utf-8" });
    const href = URL.createObjectURL(blob);
    saveFile(href, `${project || "projet"}_${sel === ALL ? "tous" : sel.replace(/\W+/g, "-")}_${new Date().toISOString().slice(0, 19).replace(/:/g, "-")}.txt`);
    setTimeout(() => URL.revokeObjectURL(href), 1000);
    flash(shown.length + " lignes téléchargées");
  };
  const copyAll = () => {
    void copyText(shown.map(line).join("\n")).then((ok) => flash(ok ? shown.length + " lignes copiées" : "Copie impossible"));
  };
  const counts = { info: 0, warn: 0, error: 0, debug: 0 };
  scoped.forEach((l) => counts[l.lv]++);
  const hl = (t: string): ReactNode => {
    if (!q) return t;
    const i = t.toLowerCase().indexOf(q.toLowerCase());
    if (i < 0) return t;
    return (
      <>
        {t.slice(0, i)}
        <mark style={{ background: "var(--text-strong)", color: "#0b0b0b", borderRadius: 2, padding: "0 1px" }}>{t.slice(i, i + q.length)}</mark>
        {t.slice(i + q.length)}
      </>
    );
  };
  const LVTABS: ["all" | LogLevel, string, number][] = [
    ["all", "Tout", counts.info + counts.warn + counts.error],
    ["error", "Erreurs", counts.error],
    ["warn", "Warnings", counts.warn],
    ["info", "Info", counts.info],
    ["debug", "Debug", counts.debug],
  ];
  const ctrl: CSSProperties = { display: "flex", alignItems: "center", height: 32, borderRadius: "var(--radius-md)", border: "1px solid var(--border-strong)", background: "var(--surface-control)" };
  const tbtn = (on: boolean): CSSProperties => ({ display: "flex", alignItems: "center", justifyContent: "center", gap: 6, height: 30, minWidth: 30, padding: "0 8px", border: 0, borderRadius: 7, cursor: "pointer", background: on ? "var(--surface-selected)" : "transparent", color: on ? "var(--text-strong)" : "var(--text-muted)", font: "500 12px/1 var(--font-sans)" });
  const stopped = selC?.st === "off";
  const colOf = (c: string) => containers.find((x) => x.id === c)?.col;
  return (
    <div style={{ display: "grid", gridTemplateColumns: "220px minmax(0,1fr)", gap: 20, alignItems: "start" }}>
      <div style={{ display: "flex", flexDirection: "column", gap: 4, position: "sticky", top: 0 }}>
        <div style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)", padding: "8px 10px 10px" }}>Conteneurs</div>
        {[{ id: ALL } as const, ...containers].map((C) => {
          const on = sel === C.id;
          const isAll = C.id === ALL;
          const c = isAll ? undefined : (C as LogContainer);
          return (
            <button key={C.id} aria-pressed={on} onClick={() => setSel(C.id)} style={{ display: "flex", flexDirection: "column", gap: 8, padding: "10px 10px", borderRadius: "var(--radius-md)", border: "1px solid " + (on ? "var(--border-strong)" : "transparent"), cursor: "pointer", textAlign: "left", background: on ? "var(--surface-raised)" : "transparent", opacity: c?.st === "off" && !on ? 0.6 : 1 }}>
              <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                {isAll ? <Icon name="Layers" size={13} color="var(--text-muted)" /> : <span style={{ width: 8, height: 8, borderRadius: 2, background: c?.col, flex: "none" }}></span>}
                <span style={{ flex: 1, font: isAll ? "500 13px/1 var(--font-sans)" : "500 12px/1 var(--font-mono)", color: on ? "var(--text-strong)" : "var(--text-body)" }}>{isAll ? "Tous les conteneurs" : C.id}</span>
                {c && <StatusDot status={c.st} />}
              </span>
              {c && (
                <span style={{ display: "flex", alignItems: "center", gap: 10, paddingLeft: 16, font: "400 11px/1 var(--font-sans)", color: "var(--text-faint)" }}>
                  <span>{c.img}</span>
                  <span style={{ flex: 1 }}></span>
                  {c.st === "off" ? (
                    <span>arrêté</span>
                  ) : (
                    <>
                      {c.cpu != null && <span>{Math.round(c.cpu)} %</span>}
                      {c.mem != null && <span>{formatMem(c.mem)}</span>}
                    </>
                  )}
                </span>
              )}
              {c && c.st !== "off" && c.cpu != null && (
                <span style={{ marginLeft: 16, height: 2, borderRadius: 2, background: "var(--viz-4)", overflow: "hidden" }}>
                  <span style={{ display: "block", height: "100%", width: Math.min(100, c.cpu) + "%", background: "var(--viz-1)" }}></span>
                </span>
              )}
            </button>
          );
        })}
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 14, minWidth: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <div style={{ ...ctrl, flex: 1, minWidth: 0, gap: 8, padding: "0 10px" }}>
            <Icon name="Search" size={14} color="var(--text-subtle)" />
            <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Rechercher dans les logs" style={{ flex: 1, minWidth: 0, background: "transparent", border: 0, outline: "none", color: "var(--text-strong)", font: "400 13px/1 var(--font-sans)" }} />
            {q ? (
              <button aria-label="Effacer la recherche" onClick={() => setQ("")} style={{ background: "none", border: 0, padding: 0, cursor: "pointer", color: "var(--text-subtle)", display: "flex" }}>
                <Icon name="X" size={13} />
              </button>
            ) : (
              <span style={{ font: "500 10px/1 var(--font-mono)", color: "var(--text-faint)", border: "1px solid var(--border-strong)", borderRadius: 4, padding: "3px 5px" }}>/</span>
            )}
          </div>
          <Select icon="Clock" options={RANGES.map(([n]) => n)} value={range} onChange={setRange} />
          <Button icon="Download" variant="inverse" onClick={download}>
            Télécharger .txt
          </Button>
        </div>
        <div style={{ borderRadius: "var(--radius-xl)", border: "1px solid var(--border-default)", background: "var(--gray-0)", overflow: "hidden", position: "relative" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "10px 10px 10px 14px", borderBottom: hair }}>
            <div style={{ display: "flex", gap: 2, padding: 2, borderRadius: 9, background: "var(--surface-control)", border: "1px solid var(--border-default)" }}>
              {LVTABS.map(([k, n, c]) => (
                <button key={k} aria-pressed={lv === k} onClick={() => setLv(k)} style={tbtn(lv === k)}>
                  {k !== "all" && <span style={{ width: 6, height: 6, borderRadius: 99, background: k === "error" ? "#ef4444" : k === "warn" ? "#e5b454" : k === "info" ? "var(--viz-1)" : "var(--viz-3)" }}></span>}
                  {n}
                  <span style={{ color: "var(--text-faint)", fontFamily: "var(--font-mono)", fontSize: 11, fontWeight: 400 }}>{c}</span>
                </button>
              ))}
            </div>
            <span style={{ flex: 1 }}></span>
            <div style={{ display: "flex", gap: 2 }}>
              <button title="Horodatage" aria-pressed={ts} onClick={() => setTs((v) => !v)} style={tbtn(ts)}>
                <Icon name="Clock" size={14} />
              </button>
              <button title="Retour à la ligne" aria-pressed={wrap} onClick={() => setWrap((v) => !v)} style={tbtn(wrap)}>
                <Icon name="TextWrap" size={14} />
              </button>
              <button title="Copier" onClick={copyAll} style={tbtn(false)}>
                <Icon name="Copy" size={14} />
              </button>
              <button
                title="Vider"
                onClick={() => {
                  clear();
                  flash("Affichage vidé");
                }}
                style={tbtn(false)}
              >
                <Icon name="Eraser" size={14} />
              </button>
            </div>
            <span style={{ width: 1, height: 20, background: "var(--border-strong)" }}></span>
            <button onClick={toggleLive} style={{ ...tbtn(false), gap: 8, padding: "0 10px", color: "var(--text-strong)" }}>
              <span style={{ position: "relative", width: 8, height: 8 }}>
                {live && !stopped && !reconnecting && <span className="live-ping" style={{ position: "absolute", inset: 0, borderRadius: 99, background: "var(--green-500)" }}></span>}
                <span style={{ position: "absolute", inset: 0, borderRadius: 99, background: live && !stopped && !reconnecting ? "var(--green-500)" : "var(--viz-2)" }}></span>
              </span>
              {!live ? "En pause" : reconnecting ? "Reconnexion…" : "En direct"}
              <Icon name={live ? "Pause" : "Play"} size={12} color="var(--text-muted)" />
            </button>
          </div>
          <div style={{ padding: "14px 16px 10px", borderBottom: hair, display: "flex", flexDirection: "column", gap: 8 }}>
            <Histogram logs={scoped} />
            <div style={{ display: "flex", justifyContent: "space-between", font: "400 11px/1 var(--font-sans)", color: "var(--text-faint)" }}>
              <span>{scoped[0]?.ts.slice(0, 5) || "—"}</span>
              <span>
                {shown.length} lignes affichées · {sel === ALL ? "tous les conteneurs" : sel}
              </span>
              <span>{scoped[scoped.length - 1]?.ts.slice(0, 5) || "—"}</span>
            </div>
          </div>
          <div ref={box} onScroll={onScroll} className="log-scroll" style={{ height: 460, overflowY: "auto", padding: "6px 0", font: "400 12px/18px var(--font-mono)" }}>
            {shown.map((l, i) => (
              <LogRow
                key={l.id}
                l={l}
                i={i}
                showC={sel === ALL}
                wrap={wrap}
                ts={ts}
                hl={hl}
                col={colOf(l.c)}
                onCopy={(x) => {
                  void copyText(line(x)).then((ok) => flash(ok ? "Ligne copiée" : "Copie impossible"));
                }}
              />
            ))}
            {!shown.length && (
              <div style={{ height: "100%", display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 10, fontFamily: "var(--font-sans)" }}>
                <Icon name={stopped ? "CirclePause" : "SearchX"} size={20} color="var(--text-faint)" />
                <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-body)" }}>{stopped ? "Conteneur arrêté" : "Aucun log"}</span>
                <span style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" }}>{stopped ? "Les logs conservés sont disponibles au téléchargement." : "Modifiez la recherche ou le filtre de niveau."}</span>
              </div>
            )}
          </div>
          {!atBottom && (
            <button onClick={() => setAtBottom(true)} style={{ position: "absolute", bottom: 16, left: "50%", transform: "translateX(-50%)", display: "flex", alignItems: "center", gap: 6, height: 30, padding: "0 14px", borderRadius: 99, border: 0, background: "var(--surface-inverse)", color: "var(--text-inverse)", font: "500 12px/1 var(--font-sans)", cursor: "pointer", boxShadow: "0 8px 24px rgba(0,0,0,.5)" }}>
              <Icon name="ArrowDown" size={12} />
              Revenir au direct
            </button>
          )}
          {toast && (
            <div role="status" style={{ position: "absolute", top: 12, left: "50%", transform: "translateX(-50%)", padding: "8px 12px", borderRadius: "var(--radius-md)", background: "var(--surface-tooltip)", boxShadow: "var(--shadow-tooltip)", font: "400 12px/1 var(--font-sans)", color: "var(--text-strong)", display: "flex", alignItems: "center", gap: 8 }}>
              <Icon name="Check" size={12} color="var(--green-500)" />
              {toast}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

const TONE: Record<LogLevel, CodeBlockLine["tone"]> = { info: "info", warn: "muted", error: "error", debug: "muted" };

/** Mise en page du panneau d'un déploiement : `body` (les lignes), `reconnecting` (flux coupé). */
export type DeploymentLogLayout = (body: ReactNode, reconnecting: boolean) => ReactNode;

/** Logs de build d'un déploiement (ProjectDeployments.jsx : CodeBlock du panneau de détail). */
function DeploymentLogViewer({ deployment, layout }: { deployment: string; layout?: DeploymentLogLayout }) {
  const sources = useMemo<LogSource[]>(() => [{ deployment }], [deployment]);
  const { lines, reconnecting } = useLogStream(sources);
  const rows: CodeBlockLine[] = lines.length ? lines.map((l) => ({ time: l.ts.slice(0, 8), text: l.msg, tone: TONE[l.lv] })) : [{ text: "Aucun log", tone: "muted" }];
  const body = (
    <>
      {reconnecting && (
        <div role="status" style={{ padding: "0 2px 8px", font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" }}>
          Connexion perdue, reconnexion…
        </div>
      )}
      <CodeBlock maxHeight={340} lines={rows} />
    </>
  );
  return layout ? layout(body, reconnecting) : body;
}

export type LogViewerProps = { project: string; containers: LogContainer[] } | { deployment: string; layout?: DeploymentLogLayout };

/** Logs en direct : conteneurs d'un projet (onglet Logs) ou build d'un déploiement. */
export function LogViewer(props: LogViewerProps) {
  return "deployment" in props ? <DeploymentLogViewer deployment={props.deployment} layout={props.layout} /> : <ProjectLogViewer project={props.project} containers={props.containers} />;
}

/** Lien de téléchargement du journal d'un déploiement (GET /api/deployments/{id}/logs.txt). */
export function downloadDeploymentLogs(deployment: string) {
  saveFile(`/api/deployments/${encodeURIComponent(deployment)}/logs.txt`);
}
