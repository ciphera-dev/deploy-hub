import { Link } from "@tanstack/react-router";
import { useState, type CSSProperties } from "react";

import { Button, Icon, Menu, useToast, type IconName } from "@/design-system";
import { timeAgo } from "@/features/projects/format";
import { copyText } from "@/lib/clipboard";

import { isActive, type Deployment, type DeploymentStatus } from "./api";
import { Author, RollbackModal } from "./DeploymentDetail";
import { deploymentTitle, formatDuration, formatStepSeconds, shortSha, stageLabel, STATUS_VIEW } from "./format";
import type { useDeploymentActions } from "./useDeploymentActions";

// Pages2.jsx#DeploymentsPage : en-tête de colonnes triables et ligne factorisée
// (DeployRow), réutilisable par d'autres listes. Écarts : pas de message de
// commit (titre issu du déclencheur), pas d'avancement en % (étape courante à
// la place), « Voir le commit » absent (le dépôt n'est pas dans la charge
// utile), « Logs » devient « Détails » (page partageable).

type Actions = ReturnType<typeof useDeploymentActions>;

const lbl: CSSProperties = { font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" };
const COLS = "28px minmax(0,1.7fr) minmax(0,1fr) minmax(0,1fr) 120px 190px";

const ST_ICON: Record<DeploymentStatus, { icon: IconName; color: string }> = {
  success: { icon: "CircleCheck", color: "var(--green-500)" },
  queued: { icon: "CircleExclamationPoint", color: "#e5b454" },
  running: { icon: "CircleExclamationPoint", color: "#e5b454" },
  failed: { icon: "CircleX", color: "#f87171" },
  cancelled: { icon: "CircleMinus", color: "var(--text-subtle)" },
};

export type SortKey = "number" | "project_name" | "status" | "created_at";

export function DeploymentsTable({ deployments, actions }: { deployments: Deployment[]; actions: Actions }) {
  const [expanded, setExpanded] = useState<string | null>(null);
  const [sort, setSort] = useState<[SortKey, 1 | -1]>(["number", -1]);
  const rows = [...deployments].sort((x, y) => (x[sort[0]] === y[sort[0]] ? 0 : x[sort[0]] > y[sort[0]] ? 1 : -1) * sort[1]);
  const head = (key: SortKey, label: string) => (
    <button
      type="button"
      onClick={() => setSort((s) => [key, s[0] === key ? (-s[1] as 1 | -1) : 1])}
      style={{ display: "flex", alignItems: "center", gap: 6, padding: 0, border: 0, background: "transparent", cursor: "pointer", font: "500 12px/1 var(--font-sans)", color: sort[0] === key ? "var(--text-strong)" : "var(--text-muted)" }}
    >
      {label}
      <Icon name="ArrowDownUp" size={12} />
    </button>
  );
  return (
    <div style={{ borderRadius: "var(--radius-xl)", border: "1px solid var(--border-default)", background: "var(--surface-app)" }}>
      <div style={{ display: "grid", gridTemplateColumns: COLS, gap: 16, padding: "14px 16px" }}>
        <span />
        {head("number", "Détails")}
        {head("project_name", "Projet")}
        {head("status", "Statut")}
        {head("created_at", "Date")}
        <span />
      </div>
      {rows.map((d) => (
        <DeployRow key={d.id} deployment={d} open={expanded === d.id} onToggle={() => setExpanded((x) => (x === d.id ? null : d.id))} actions={actions} />
      ))}
      {!rows.length && <div style={{ padding: "28px 16px", borderTop: "1px solid var(--border-subtle)", ...lbl }}>Aucun déploiement.</div>}
    </div>
  );
}

export function DeployRow({ deployment: d, open, onToggle, actions }: { deployment: Deployment; open: boolean; onToggle: () => void; actions: Actions }) {
  const [hover, setHover] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const { flash } = useToast();
  const st = ST_ICON[d.status];
  const sha = shortSha(d.commit_sha);
  const meta = [`#${d.number}`, sha, d.branch].filter(Boolean).join(" · ");
  const stage = d.stage && isActive(d) ? ` · ${stageLabel(d.stage)}` : "";
  const failedTime = d.status === "failed";
  const step = (key: string) => {
    const s = d.steps.find((x) => x.stage === key);
    return s ? (s.status === "running" ? "…" : formatStepSeconds(s.duration_ms)) : "—";
  };
  return (
    <div data-deployment={d.id} style={{ borderTop: "1px solid var(--border-subtle)" }} onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}>
      <div onClick={onToggle} style={{ display: "grid", gridTemplateColumns: COLS, gap: 16, alignItems: "center", padding: "14px 16px", cursor: "pointer", background: hover && !open ? "var(--surface-raised)" : "transparent" }}>
        <span
          role="button"
          tabIndex={0}
          aria-expanded={open}
          aria-label={`Détails de #${d.number}`}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              onToggle();
            }
          }}
          style={{ display: "flex" }}
        >
          <Icon name="ChevronDown" size={15} color="var(--text-muted)" style={{ transform: open ? "rotate(180deg)" : "none", transition: "transform var(--dur-fast)" }} />
        </span>
        <div style={{ display: "flex", alignItems: "center", gap: 12, minWidth: 0 }}>
          <span style={{ width: 40, height: 40, borderRadius: 10, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", background: "var(--surface-selected)", border: "1px solid var(--border-default)" }}>
            <Icon name="GitCommitHorizontal" size={18} color="var(--text-strong)" />
          </span>
          <div style={{ display: "flex", flexDirection: "column", gap: 7, minWidth: 0 }}>
            <span style={{ font: "500 13px/1.2 var(--font-sans)", color: "var(--text-strong)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{deploymentTitle(d)}</span>
            <span style={{ ...lbl, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{meta}</span>
          </div>
        </div>
        <span style={{ font: "400 12px/1 var(--font-mono)", color: "var(--text-body)", minWidth: 0, overflow: "hidden", textOverflow: "ellipsis" }}>{d.project_name}</span>
        <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <Icon name={st.icon} size={16} color={st.color} />
          <span style={{ font: "400 13px/1.2 var(--font-sans)", color: "var(--text-body)" }}>
            {STATUS_VIEW[d.status].label}
            {stage}
          </span>
        </span>
        <span>
          <span style={{ display: "inline-flex", alignItems: "center", height: 22, padding: "0 8px", borderRadius: 6, background: failedTime ? "rgba(229,180,84,.12)" : "var(--surface-selected)", font: "500 12px/1 var(--font-mono)", color: failedTime ? "#e5b454" : "var(--text-body)" }}>{timeAgo(d.created_at)}</span>
        </span>
        <div style={{ display: "flex", gap: 6, justifyContent: "flex-end", alignItems: "center" }} onClick={(e) => e.stopPropagation()}>
          {actions.canDeploy && isActive(d) && (
            <Button size="sm" disabled={actions.busy || d.cancel_requested} onClick={() => void actions.cancel(d)}>
              {d.cancel_requested ? "Annulation demandée" : "Annuler"}
            </Button>
          )}
          <Link to="/deployments/$deploymentId" params={{ deploymentId: d.id }} style={{ display: "inline-flex", alignItems: "center", height: 28, padding: "0 10px", borderRadius: "var(--radius-md)", background: "var(--surface-control)", border: "1px solid var(--border-strong)", boxShadow: "var(--shadow-control)", color: "var(--text-body)", font: "500 12px/1 var(--font-sans)", textDecoration: "none" }}>
            Détails
          </Link>
          <Menu
            label={`Actions de #${d.number}`}
            width={230}
            items={[
              { icon: "RotateCw", label: "Redéployer", disabled: !actions.canRedeploy, onSelect: () => void actions.redeploy(d.project_id) },
              { icon: "Undo2", label: "Revenir à cette version", disabled: !actions.canDeploy || !(d.status === "success" && d.can_rollback && !d.current), onSelect: () => setConfirm(true) },
              {
                icon: "Copy",
                label: "Copier le SHA",
                disabled: !d.commit_sha,
                onSelect: () => {
                  if (d.commit_sha) void copyText(d.commit_sha).then((ok) => flash(ok ? "SHA copié" : "Copie impossible"));
                },
              },
            ]}
          />
        </div>
      </div>
      {open && (
        <div style={{ padding: "0 16px 16px" }}>
          {d.status === "failed" && d.error && (
            <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "14px 18px", borderRadius: 12, background: "rgba(239,68,68,.05)", border: "1px solid rgba(239,68,68,.18)", marginBottom: 10 }}>
              <Icon name="CircleX" size={15} color="#f87171" />
              <span style={{ flex: 1, font: "400 12px/1.4 var(--font-sans)", color: "var(--text-body)" }}>{d.error}</span>
              {actions.canDeploy && (
                <Button size="sm" disabled={actions.busy || !actions.canRedeploy} onClick={() => void actions.redeploy(d.project_id)}>
                  Relancer
                </Button>
              )}
            </div>
          )}
          <div style={{ display: "grid", gridTemplateColumns: "repeat(5,minmax(0,1fr))", gap: 24, padding: "16px 22px 18px", borderRadius: 12, background: "var(--surface-raised)" }}>
            {(
              [
                ["Auteur", d.created_by ? <Author key="a" name={d.created_by} /> : "—"],
                ["Durée", formatDuration(d.duration_ms)],
                ["Clonage", step("clone")],
                ["Build", step("build")],
                ["Image", step("image")],
              ] as const
            ).map(([k, v]) => (
              <div key={k} style={{ display: "flex", flexDirection: "column", gap: 10, minWidth: 0 }}>
                <span style={{ font: "400 13px/1 var(--font-sans)", color: "var(--text-body)" }}>{k}</span>
                <span style={{ font: "500 14px/1 var(--font-sans)", color: "var(--text-strong)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{v}</span>
              </div>
            ))}
          </div>
        </div>
      )}
      {confirm && (
        <RollbackModal
          deployment={d}
          onClose={() => setConfirm(false)}
          onConfirm={() => {
            setConfirm(false);
            void actions.rollback(d);
          }}
        />
      )}
    </div>
  );
}
