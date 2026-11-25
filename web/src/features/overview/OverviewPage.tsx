import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useState, type CSSProperties, type ReactNode } from "react";

import { Badge, Button, Icon, type IconName } from "@/design-system";
import { fetchMe, ME_QUERY_KEY, MFA_PENDING } from "@/features/auth/api";
import type { Deployment } from "@/features/deployments/api";
import { deploymentTitle, shortSha } from "@/features/deployments/format";
import { timeAgo } from "@/features/projects/format";
import { soonTooltip } from "@/app/layout/nav";

import { fetchOverview, overviewQueryKey, type KpiSeries, type Overview, type OverviewPeriod } from "./api";
import { AreaChart } from "./charts";
import { dayLabel, firstName, formatCount, formatMs, greeting, ISSUE_VIEW, kpiChange, kpiTotal, longDate, PERIOD_LABEL } from "./format";

// Pages2.jsx#OverviewV2 : en-tête (date, salutation, état, indicateurs,
// actions, « À traiter »), graphe à trois indicateurs, déploiements récents
// et charge des serveurs. Tout vient de GET /api/overview : un bloc sans
// donnée affiche son état vide (requêtes et latence n'ont pas encore de
// source), jamais de valeur inventée.

const lbl: CSSProperties = { font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" };
const mono = (v: ReactNode, c = "var(--text-body)", s = 12) => <span style={{ font: `400 ${s}px/1 var(--font-mono)`, color: c, whiteSpace: "nowrap" }}>{v}</span>;
const card: CSSProperties = { borderRadius: "var(--radius-xl)", border: "1px solid var(--border-default)", background: "var(--surface-app)" };
const hair = "1px solid var(--border-subtle)";

type Metric = "dep" | "req" | "lat";
type MetricView = { label: string; series: KpiSeries; fmt: (v: number) => string; col: string; higherIsBetter: boolean };

function metrics(o: Overview): Record<Metric, MetricView> {
  return {
    dep: { label: "Déploiements", series: o.chart.deployments, fmt: (v) => Math.round(v) + "", col: "#a78bfa", higherIsBetter: true },
    req: { label: "Requêtes", series: o.chart.requests, fmt: formatCount, col: "#7dd3fc", higherIsBetter: true },
    lat: { label: "Latence p95", series: o.chart.latency_p95_ms, fmt: formatMs, col: "#86efac", higherIsBetter: false },
  };
}

const DEPLOYMENT_ST: Record<Deployment["status"], [IconName, string, string]> = {
  success: ["CircleCheck", "var(--green-500)", "Réussi"],
  queued: ["CircleExclamationPoint", "#e5b454", "En cours"],
  running: ["CircleExclamationPoint", "#e5b454", "En cours"],
  failed: ["CircleX", "#f87171", "Échec"],
  cancelled: ["CircleMinus", "var(--text-subtle)", "Annulé"],
};

function seg(v: number, n = 4) {
  const on = Math.round((v / 100) * n);
  return (
    <span style={{ display: "flex", gap: 4 }}>
      {Array.from({ length: n }, (_, i) => (
        <span key={i} style={{ width: 28, height: 5, borderRadius: 99, background: i < on ? (v >= 85 ? "#f87171" : "#8b7cf6") : "var(--surface-selected)" }} />
      ))}
    </span>
  );
}

function link(text: string, onClick: (() => void) | null, title?: string) {
  return (
    <button type="button" onClick={onClick ?? undefined} disabled={!onClick} title={title} style={{ display: "flex", alignItems: "center", gap: 4, padding: 0, border: 0, background: "transparent", cursor: onClick ? "pointer" : "default", font: "400 12px/1 var(--font-sans)", color: "var(--text-muted)", opacity: onClick ? 1 : 0.5 }}>
      {text}
      <Icon name="ArrowRight" size={12} />
    </button>
  );
}

/** Compte chargé par le shell, sans nouvelle requête (voir useCanDeploy). */
function useFirstName(): string {
  const me = useQuery({ queryKey: ME_QUERY_KEY, queryFn: fetchMe, retryOnMount: false, refetchOnMount: false });
  return me.data && me.data !== MFA_PENDING ? firstName(me.data.name) : "";
}

export function OverviewPage() {
  const [period, setPeriod] = useState<OverviewPeriod>("30d");
  const overview = useQuery({ queryKey: overviewQueryKey(period), queryFn: ({ signal }) => fetchOverview(period, signal), placeholderData: (prev) => prev });
  const name = useFirstName();
  // Heure de l'affichage (salutation, date, « il y a… »), figée au montage.
  const [now] = useState(() => new Date());
  if (overview.isError && !overview.data) {
    return (
      <div style={{ padding: "20px 32px 48px 34px" }}>
        <div role="alert" style={{ ...card, padding: "18px 20px", font: "400 13px/1.5 var(--font-sans)", color: "var(--text-muted)" }}>
          Vue d'ensemble indisponible pour le moment. Réessayez dans un instant.
        </div>
      </div>
    );
  }
  if (!overview.data) {
    return <div aria-busy="true" style={{ padding: "20px 32px 48px 34px", ...lbl }}>Chargement…</div>;
  }
  return <OverviewView o={overview.data} name={name} period={period} onPeriod={setPeriod} now={now} />;
}

export function OverviewView({ o, name, period, onPeriod, now }: { o: Overview; name: string; period: OverviewPeriod; onPeriod: (p: OverviewPeriod) => void; now: Date }) {
  const navigate = useNavigate();
  const [m, setM] = useState<Metric>("dep");
  const [h, setH] = useState<string | null>(null);
  const MET = metrics(o);
  const M = MET[m];
  const okAll = o.issues.length === 0;
  const issues = o.issues;
  const goIssue = (kind: (typeof issues)[number]["kind"], id: string) => {
    if (kind === "server_unreachable") return;
    void navigate({ to: "/projects/$projectId", params: { projectId: id } });
  };
  return (
    <div style={{ padding: "20px 32px 48px 34px", display: "flex", flexDirection: "column", gap: 14 }}>
      <section aria-label="Vue d'ensemble" style={{ ...card, borderRadius: 20, position: "relative", overflow: "hidden" }}>
        <span style={{ position: "absolute", inset: 0, background: "radial-gradient(55% 120% at 0% 0%,rgba(255,255,255,.06),transparent 70%)", pointerEvents: "none" }} />
        <span style={{ position: "absolute", top: 0, right: 0, bottom: 0, width: "46%", backgroundImage: "radial-gradient(rgba(255,255,255,.07) 1px,transparent 1px)", backgroundSize: "16px 16px", maskImage: "linear-gradient(90deg,transparent,#000 60%)", WebkitMaskImage: "linear-gradient(90deg,transparent,#000 60%)", pointerEvents: "none" }} />
        <div style={{ position: "relative", display: "grid", gridTemplateColumns: "minmax(0,1fr) minmax(0,360px)", gap: 28, padding: "26px 28px" }}>
          <div style={{ display: "flex", flexDirection: "column", gap: 14, minWidth: 0 }}>
            <span style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)", textTransform: "capitalize" }}>{longDate(now)}</span>
            <span style={{ font: "600 26px/1.15 var(--font-sans)", color: "var(--text-strong)", letterSpacing: "-0.02em" }}>{[greeting(now.getHours()), name].filter(Boolean).join(" ")}</span>
            <span style={{ display: "flex", alignItems: "center", gap: 8, font: "400 14px/1.5 var(--font-sans)", color: "var(--text-muted)" }}>
              <span style={{ width: 8, height: 8, borderRadius: 99, flex: "none", background: okAll ? "var(--green-500)" : "#e5b454" }} />
              {okAll ? "Tous vos services fonctionnent normalement." : `${issues.length} point${issues.length > 1 ? "s" : ""} à traiter, le reste fonctionne normalement.`}
            </span>
            <div style={{ display: "flex", alignItems: "center", gap: 0, marginTop: 8 }}>
              {(
                [
                  ["Projets en ligne", o.projects_online, "/ " + o.projects_total],
                  ["Serveurs", o.servers_online, "/ " + o.servers_total],
                  ["Déploiements aujourd'hui", o.deployments_today, ""],
                ] as const
              ).map(([k, v, u], i) => (
                <div key={k} style={{ display: "flex", flexDirection: "column", gap: 9, padding: i ? "0 24px" : "0 24px 0 0", borderLeft: i ? hair : "none" }}>
                  <span style={{ ...lbl, whiteSpace: "nowrap" }}>{k}</span>
                  <span style={{ display: "flex", alignItems: "baseline", gap: 5 }}>
                    <span style={{ font: "600 22px/1 var(--font-sans)", color: "var(--text-strong)" }}>{v}</span>
                    <span style={{ font: "400 13px/1 var(--font-sans)", color: "var(--text-subtle)" }}>{u}</span>
                  </span>
                </div>
              ))}
            </div>
            <div style={{ display: "flex", gap: 8, marginTop: 10 }}>
              <Button variant="inverse" icon="Plus" onClick={() => void navigate({ to: "/projects" })}>Nouveau projet</Button>
              <Button icon="Rocket" onClick={() => void navigate({ to: "/deployments" })}>Déploiements</Button>
            </div>
          </div>
          <div aria-label="À traiter" role="region" style={{ display: "flex", flexDirection: "column", borderRadius: 14, border: "1px solid var(--border-default)", background: "rgba(11,11,13,.7)", overflow: "hidden", alignSelf: "start" }}>
            <div style={{ display: "flex", alignItems: "center", padding: "12px 14px", borderBottom: hair }}>
              <span style={{ flex: 1, font: "600 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>À traiter</span>
              <span style={{ display: "inline-flex", alignItems: "center", height: 20, padding: "0 7px", borderRadius: 6, background: issues.length ? "rgba(229,180,84,.12)" : "rgba(34,197,94,.12)", font: "500 11px/1 var(--font-mono)", color: issues.length ? "#e5b454" : "var(--green-500)" }}>{issues.length}</span>
            </div>
            {issues.length ? (
              issues.map((x) => {
                const v = ISSUE_VIEW[x.kind];
                return (
                  <button
                    key={x.kind + x.id}
                    type="button"
                    onClick={() => goIssue(x.kind, x.id)}
                    onMouseEnter={(e) => (e.currentTarget.style.background = "var(--surface-raised)")}
                    onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}
                    style={{ display: "flex", alignItems: "center", gap: 12, padding: "12px 14px", border: 0, borderTop: hair, background: "transparent", cursor: x.kind === "server_unreachable" ? "default" : "pointer", textAlign: "left" }}
                  >
                    <span style={{ width: 32, height: 32, borderRadius: 9, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", background: "var(--surface-selected)" }}>
                      <Icon name={v.icon} size={15} color={v.color} />
                    </span>
                    <span style={{ display: "flex", flexDirection: "column", gap: 6, flex: 1, minWidth: 0 }}>
                      {mono(x.name, "var(--text-strong)", 13)}
                      <span style={{ ...lbl, fontSize: 11 }}>{v.label}</span>
                    </span>
                    <Icon name="ChevronRight" size={14} color="var(--text-subtle)" />
                  </button>
                );
              })
            ) : (
              <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "16px 14px", font: "400 13px/1 var(--font-sans)", color: "var(--text-muted)" }}>
                <Icon name="CircleCheck" size={15} color="var(--green-500)" />
                Rien à signaler
              </div>
            )}
          </div>
        </div>
      </section>

      <section aria-label="Indicateurs" style={{ ...card, borderRadius: 18, padding: "20px 22px 18px", display: "flex", flexDirection: "column", gap: 20 }}>
        <div role="tablist" style={{ display: "flex", alignItems: "stretch", gap: 0 }}>
          {(Object.entries(MET) as [Metric, MetricView][]).map(([k, v], idx) => {
            const on = m === k;
            const change = kpiChange(v.series);
            const good = (v.series.change_percent ?? 0) >= 0 === v.higherIsBetter;
            return (
              <button key={k} type="button" role="tab" aria-selected={on} onClick={() => setM(k)} style={{ display: "flex", flexDirection: "column", gap: 10, padding: "4px 28px 4px " + (idx ? "28px" : "0"), border: 0, borderLeft: idx ? hair : "none", background: "transparent", cursor: "pointer", textAlign: "left", opacity: on ? 1 : 0.5, transition: "opacity .15s" }}>
                <span style={{ display: "flex", alignItems: "center", gap: 7, ...lbl }}>
                  <span style={{ width: 8, height: 8, borderRadius: 3, background: v.col }} />
                  {v.label}
                </span>
                <span style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
                  <span style={{ font: "600 26px/1 var(--font-sans)", color: "var(--text-strong)", letterSpacing: "-0.01em" }}>{kpiTotal(v.series, v.fmt)}</span>
                  {change && <span style={{ font: "500 12px/1 var(--font-sans)", color: good ? "#86efac" : "#f87171" }}>{change}</span>}
                </span>
              </button>
            );
          })}
          <span style={{ flex: 1 }} />
          <button
            type="button"
            title="Changer de période"
            onClick={() => onPeriod(period === "30d" ? "7d" : "30d")}
            style={{ alignSelf: "flex-start", display: "inline-flex", alignItems: "center", height: 22, padding: "0 8px", border: 0, borderRadius: 6, background: "var(--surface-selected)", cursor: "pointer", font: "500 12px/1 var(--font-mono)", color: "var(--text-body)" }}
          >
            {PERIOD_LABEL[o.period]}
          </button>
        </div>
        {M.series.points.length ? (
          <AreaChart key={m + o.period} series={M.series.points.map((p) => p.value)} labels={M.series.points.map((p) => dayLabel(p.date))} fmt={M.fmt} col={M.col} />
        ) : (
          <div style={{ height: 268, display: "flex", alignItems: "center", justifyContent: "center", ...lbl, fontSize: 13 }}>Aucune mesure pour l'instant : cet indicateur sera mesuré par le proxy dans une prochaine version.</div>
        )}
      </section>

      <div style={{ display: "grid", gridTemplateColumns: "minmax(0,1.4fr) minmax(0,1fr)", gap: 14 }}>
        <section aria-label="Déploiements récents" style={{ ...card, borderRadius: 18, overflow: "hidden" }}>
          <div style={{ display: "flex", alignItems: "center", padding: "16px 20px" }}>
            <span style={{ font: "600 14px/1 var(--font-sans)", color: "var(--text-strong)" }}>Déploiements récents</span>
            <span style={{ flex: 1 }} />
            {link("Tout voir", () => void navigate({ to: "/deployments" }))}
          </div>
          {(o.recent_deployments ?? []).length === 0 && <div style={{ borderTop: hair, padding: "18px 20px", ...lbl, fontSize: 13 }}>Aucun déploiement pour l'instant.</div>}
          {(o.recent_deployments ?? []).map((d) => {
            const st = DEPLOYMENT_ST[d.status];
            const sub = [d.project_name, d.created_by, `#${d.number}`, shortSha(d.commit_sha)].filter(Boolean).join(" · ");
            return (
              <div
                key={d.id}
                onMouseEnter={() => setH(d.id)}
                onMouseLeave={() => setH(null)}
                onClick={() => void navigate({ to: "/deployments/$deploymentId", params: { deploymentId: d.id } })}
                style={{ display: "grid", gridTemplateColumns: "minmax(0,1fr) 110px 90px", gap: 16, alignItems: "center", padding: "14px 20px", borderTop: hair, background: h === d.id ? "var(--surface-raised)" : "transparent", cursor: "pointer" }}
              >
                <div style={{ display: "flex", alignItems: "center", gap: 14, minWidth: 0 }}>
                  <span style={{ width: 40, height: 40, borderRadius: 10, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", background: "var(--surface-selected)" }}>
                    <Icon name="GitCommitHorizontal" size={18} color="var(--text-strong)" />
                  </span>
                  <div style={{ display: "flex", flexDirection: "column", gap: 7, minWidth: 0 }}>
                    <span style={{ font: "500 14px/1.1 var(--font-sans)", color: "var(--text-strong)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{deploymentTitle(d)}</span>
                    <span style={{ ...lbl, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{sub}</span>
                  </div>
                </div>
                <span style={{ display: "flex", alignItems: "center", gap: 7 }}>
                  <Icon name={st[0]} size={15} color={st[1]} />
                  <span style={{ font: "400 13px/1 var(--font-sans)", color: "var(--text-body)" }}>{st[2]}</span>
                </span>
                <span style={{ ...lbl, textAlign: "right", whiteSpace: "nowrap" }}>{timeAgo(d.created_at, now.getTime())}</span>
              </div>
            );
          })}
        </section>
        <section aria-label="Charge des serveurs" style={{ ...card, borderRadius: 18, overflow: "hidden" }}>
          <div style={{ display: "flex", alignItems: "center", padding: "16px 20px" }}>
            <span style={{ font: "600 14px/1 var(--font-sans)", color: "var(--text-strong)" }}>Charge des serveurs</span>
            <span style={{ flex: 1 }} />
            {link("Serveurs", null, soonTooltip("v0.2"))}
          </div>
          {(o.servers ?? []).length === 0 && <div style={{ borderTop: hair, padding: "18px 20px", ...lbl, fontSize: 13 }}>Aucun serveur enregistré.</div>}
          {(o.servers ?? []).map((s) => {
            const off = !s.online;
            const c = off || s.cpu_percent == null ? null : Math.round(s.cpu_percent);
            return (
              <div key={s.id} data-server={s.name} style={{ display: "flex", alignItems: "center", gap: 14, padding: "14px 20px", borderTop: hair }}>
                <div style={{ display: "flex", flexDirection: "column", gap: 9, flex: 1, minWidth: 0 }}>
                  <span style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
                    {mono(s.name, "var(--text-strong)", 13)}
                    {off && <Badge>Hors ligne</Badge>}
                  </span>
                  {off && s.link_error && <span style={{ ...lbl, fontSize: 11, color: "#e5b454" }}>{s.link_error}</span>}
                  <span style={{ display: "flex", alignItems: "center", gap: 10 }}>
                    {seg(c ?? 0)}
                    <span style={{ ...lbl, fontSize: 11 }}>{c == null ? "—" : c + " % CPU"}</span>
                  </span>
                </div>
              </div>
            );
          })}
        </section>
      </div>
    </div>
  );
}
