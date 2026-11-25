import { Link, useNavigate } from "@tanstack/react-router";
import type { CSSProperties, ReactNode } from "react";

import { Badge, Button, Icon, StatusDot } from "@/design-system";
import { StatusIcon } from "@/features/deployments/DeploymentTimeline";
import { deploymentTitle, formatDuration, shortSha, STATUS_VIEW as DEPLOYMENT_STATUS } from "@/features/deployments/format";

import type { Project } from "../api";
import { buildLabel, timeAgo } from "../format";
import type { ProjectOverviewData, Sparkline } from "./api";
import { Bar, Spark } from "./charts";
import { formatCount, formatMs, formatPercent, imageLabel, kpiValue, PROJECT_ICON, replicasLabel, SERVICE_STATE_VIEW } from "./format";

// ProjectOverview.jsx#ProjectOverview : production actuelle, indicateurs,
// services et activité récente. Les indicateurs (requêtes, latence, erreurs,
// disponibilité) et le CPU arrivent avec les métriques : en attendant, « — »
// et des séries vides. Sans domaine configuré, le nom de domaine est remplacé
// par une mention ; le message de commit (non stocké) par le titre du déploiement.

const hair = "1px solid var(--border-subtle)";
const lbl: CSSProperties = { font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" };
const mono = (v: ReactNode, c?: string) => <span style={{ fontFamily: "var(--font-mono)", fontSize: 12, color: c }}>{v}</span>;
const card: CSSProperties = { borderRadius: "var(--radius-xl)", border: "1px solid var(--border-default)", background: "var(--surface-app)" };

type Kpi = [label: string, value: string, data: number[], highlight: ((v: number) => boolean) | null];

function kpis(m: ProjectOverviewData["metrics"]): Kpi[] {
  const row = (label: string, s: Sparkline, format: (v: number) => string, hl: Kpi[3]): Kpi => [label, kpiValue(s, format), s.series, hl];
  return [
    row("Requêtes (24 h)", m.requests, formatCount, null),
    row("Latence p95", m.latency_p95_ms, formatMs, null),
    row("Taux d'erreur", m.error_rate, formatPercent(2), (v) => v > 3),
    row("Disponibilité (30 j)", m.uptime, formatPercent(2), (v) => v < 100),
  ];
}

export function ProjectOverview({ project, overview }: { project: Project; overview: ProjectOverviewData }) {
  const navigate = useNavigate();
  const go = (tab: "logs" | "deployments" | "services") => void navigate({ to: `/projects/$projectId/${tab}`, params: { projectId: project.id } });
  const prod = overview.production;
  const branch = prod?.branch ?? (project.source.type === "git" ? project.source.branch : null);
  const cells: [string, ReactNode][] = [
    ["Commit", <span key="commit" style={{ display: "flex", alignItems: "center", gap: 6 }}><Icon name="GitCommitHorizontal" size={13} color="var(--text-subtle)" />{mono(shortSha(prod?.commit_sha) ?? "—", "var(--text-strong)")}</span>],
    ["Branche", <span key="branch" style={{ display: "flex", alignItems: "center", gap: 6 }}><Icon name="GitBranch" size={13} color="var(--text-subtle)" />{mono(branch ?? "—", "var(--text-strong)")}</span>],
    ["Auteur", prod?.created_by ?? "—"],
    ["Build", buildLabel(project.build)],
    ["Déployé", prod?.finished_at ? `${timeAgo(prod.finished_at)} · ${formatDuration(prod.duration_ms)}` : "—"],
  ];
  return (
    <div style={{ padding: "24px 32px 40px 34px", display: "flex", flexDirection: "column", gap: 20 }}>
      <section aria-label="Production" style={{ ...card, overflow: "hidden" }}>
        <div style={{ display: "flex", alignItems: "flex-start", gap: 16, padding: "22px 22px 20px" }}>
          <StatusIcon status={PROJECT_ICON[overview.status]} size={36} />
          <div style={{ display: "flex", flexDirection: "column", gap: 10, flex: 1, minWidth: 0 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
              <span style={lbl}>Production</span>
              {prod && <Badge tone="positive">Actuel</Badge>}
            </div>
            {overview.domain ? (
              <a href={`https://${overview.domain}`} target="_blank" rel="noreferrer noopener" style={{ display: "flex", alignItems: "center", gap: 8, font: "500 22px/1.1 var(--font-sans)", color: "var(--text-strong)", letterSpacing: "-0.01em", textDecoration: "none" }}>
                {overview.domain}
                <Icon name="ArrowUpRight" size={16} color="var(--text-muted)" />
              </a>
            ) : (
              <span style={{ font: "500 22px/1.1 var(--font-sans)", color: "var(--text-muted)", letterSpacing: "-0.01em" }}>Aucun domaine configuré</span>
            )}
            <span style={{ font: "400 13px/1.4 var(--font-sans)", color: "var(--text-muted)" }}>{prod ? `${deploymentTitle(prod)} · #${prod.number}` : "Aucun déploiement en ligne"}</span>
          </div>
          <div style={{ display: "flex", gap: 6 }}>
            <Button icon="ScrollText" onClick={() => go("logs")}>Logs</Button>
            <Button icon="RotateCcwClock" onClick={() => go("deployments")}>Historique</Button>
          </div>
        </div>
        <div style={{ display: "grid", gridTemplateColumns: "repeat(5,minmax(0,1fr))", borderTop: hair }}>
          {cells.map(([k, v], i) => (
            <div key={k} style={{ display: "flex", flexDirection: "column", gap: 10, padding: "16px 22px", borderLeft: i ? hair : "none", minWidth: 0 }}>
              <span style={lbl}>{k}</span>
              <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{v}</span>
            </div>
          ))}
        </div>
      </section>
      <div style={{ display: "grid", gridTemplateColumns: "repeat(4,minmax(0,1fr))", gap: 12 }}>
        {kpis(overview.metrics).map(([k, v, data, hl]) => (
          <div key={k} aria-label={k} style={{ ...card, padding: "16px 18px", display: "flex", flexDirection: "column", gap: 14 }}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline" }}>
              <span style={lbl}>{k}</span>
            </div>
            <span style={{ font: "500 22px/1 var(--font-sans)", color: "var(--text-strong)", letterSpacing: "-0.01em" }}>{v}</span>
            <Spark data={data} hl={hl} />
          </div>
        ))}
      </div>
      <div style={{ display: "grid", gridTemplateColumns: "minmax(0,1fr) minmax(0,1fr)", gap: 12 }}>
        <section aria-label="Services" style={{ ...card, overflow: "hidden" }}>
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "16px 18px" }}>
            <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>Services</span>
            <Button size="sm" variant="ghost" iconRight="ArrowRight" onClick={() => go("services")}>Tout voir</Button>
          </div>
          {(overview.services ?? []).map((s) => {
            const view = SERVICE_STATE_VIEW[s.state];
            return (
              <div key={s.id} style={{ display: "grid", gridTemplateColumns: "26px minmax(0,1fr) 90px 60px", gap: 12, alignItems: "center", padding: "12px 18px", borderTop: hair }}>
                <span style={{ width: 26, height: 26, borderRadius: 7, display: "flex", alignItems: "center", justifyContent: "center", background: "var(--surface-selected)" }}>
                  <Icon name="Globe" size={13} color="var(--text-body)" />
                </span>
                <div style={{ display: "flex", flexDirection: "column", gap: 5, minWidth: 0 }}>
                  <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>{s.name}</span>
                  <span style={{ ...lbl, fontSize: 11 }}>{imageLabel(project)}</span>
                </div>
                <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                  <span style={{ ...lbl, fontSize: 11, fontFamily: "var(--font-mono)" }}>CPU {s.cpu_percent == null ? "—" : `${Math.round(s.cpu_percent)}%`}</span>
                  <Bar v={s.cpu_percent} />
                </div>
                <span style={{ justifySelf: "end" }} title={view.label}>
                  <StatusDot status={view.dot} label={replicasLabel(s)} />
                </span>
              </div>
            );
          })}
        </section>
        <section aria-label="Activité récente" style={{ ...card, overflow: "hidden" }}>
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "16px 18px" }}>
            <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>Activité récente</span>
            <Button size="sm" variant="ghost" iconRight="ArrowRight" onClick={() => go("deployments")}>Déploiements</Button>
          </div>
          {(overview.activity ?? []).length === 0 && <div style={{ padding: "12px 18px", borderTop: hair, ...lbl }}>Aucun déploiement pour l'instant.</div>}
          {(overview.activity ?? []).map((d) => (
            <Link key={d.id} to="/projects/$projectId/deployments" params={{ projectId: project.id }} style={{ display: "grid", gridTemplateColumns: "22px minmax(0,1fr) auto", gap: 12, alignItems: "center", padding: "12px 18px", borderTop: hair, cursor: "pointer", textDecoration: "none" }}>
              <StatusIcon status={d.status} size={22} />
              <div style={{ display: "flex", flexDirection: "column", gap: 5, minWidth: 0 }}>
                <span style={{ font: "500 13px/1.1 var(--font-sans)", color: "var(--text-strong)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{deploymentTitle(d)}</span>
                <span style={{ ...lbl, fontSize: 11 }}>{DEPLOYMENT_STATUS[d.status].label}</span>
              </div>
              <div style={{ display: "flex", flexDirection: "column", gap: 5, alignItems: "flex-end" }}>
                {mono("#" + d.number, "var(--text-muted)")}
                <span style={{ ...lbl, fontSize: 11 }}>{timeAgo(d.created_at)}</span>
              </div>
            </Link>
          ))}
        </section>
      </div>
    </div>
  );
}
