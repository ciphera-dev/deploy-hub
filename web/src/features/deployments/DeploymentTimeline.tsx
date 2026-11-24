import type { CSSProperties } from "react";

import { Icon, type IconName } from "@/design-system";

import type { Deployment, DeploymentStatus } from "./api";
import "./deployments.css";
import { formatStepSeconds, stageState, STAGES, type StageState } from "./format";

// ProjectDeployments.jsx#StatusIcon et #Pipeline : pastille de statut et frise
// en 6 étapes (Clonage, Build, Image, Démarrage, Healthcheck, Bascule). Une
// étape absente de `steps` n'a pas commencé : elle reste grisée. La cause d'un
// échec (étape ou déploiement, y compris un échec présumé) est annoncée sous la frise.

const lbl: CSSProperties = { font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" };

const STATUS_ICON: Record<DeploymentStatus, { icon: IconName; color: string; bg: string }> = {
  queued: { icon: "Clock", color: "var(--text-muted)", bg: "var(--surface-selected)" },
  running: { icon: "Loader", color: "var(--text-strong)", bg: "var(--surface-selected)" },
  success: { icon: "Check", color: "#0b0b0b", bg: "var(--green-500)" },
  failed: { icon: "X", color: "#fff", bg: "var(--red-500)" },
  cancelled: { icon: "Minus", color: "var(--text-muted)", bg: "var(--surface-selected)" },
};

export function StatusIcon({ status, size = 28 }: { status: DeploymentStatus; size?: number }) {
  const m = STATUS_ICON[status];
  return (
    <span style={{ width: size, height: size, borderRadius: 99, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", background: m.bg }}>
      <Icon name={m.icon} size={size * 0.5} color={m.color} className={status === "running" ? "dep-spin" : undefined} />
    </span>
  );
}

const MARK: Record<StageState, { icon: IconName; color?: string }> = {
  done: { icon: "CircleCheck", color: "var(--text-muted)" },
  failed: { icon: "CircleX", color: "var(--red-500)" },
  cancelled: { icon: "CircleMinus", color: "var(--text-muted)" },
  running: { icon: "Loader" },
  pending: { icon: "CircleDashed", color: "var(--text-faint)" },
};

export function DeploymentTimeline({ deployment: d }: { deployment: Deployment }) {
  const failedStage = STAGES.find((s) => stageState(d, s.key).state === "failed");
  const failedStep = failedStage ? stageState(d, failedStage.key).step : undefined;
  const cause = d.status === "failed" ? (failedStep?.message ?? d.error) : null;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      <ol aria-label="Étapes du déploiement" style={{ margin: 0, padding: 0, listStyle: "none", display: "grid", gridTemplateColumns: `repeat(${STAGES.length},minmax(0,1fr))`, gap: 4 }}>
        {STAGES.map((s) => {
          const { state, step } = stageState(d, s.key);
          const color = state === "failed" ? "var(--red-500)" : state === "done" ? "var(--viz-1)" : state === "running" ? "var(--gray-900)" : state === "cancelled" ? "var(--viz-3)" : "var(--viz-4)";
          const mark = MARK[state];
          return (
            <li key={s.key} data-state={state} style={{ display: "flex", flexDirection: "column", gap: 8, minWidth: 0 }}>
              <div style={{ height: 4, borderRadius: 99, background: "var(--viz-4)", overflow: "hidden" }}>
                <div className={state === "running" ? "dep-bar-run" : undefined} style={{ height: "100%", width: state === "running" ? "60%" : state === "pending" ? 0 : "100%", background: color, borderRadius: 99 }} />
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 6, font: "500 12px/1 var(--font-sans)", color: state === "pending" ? "var(--text-faint)" : "var(--text-body)" }}>
                <Icon name={mark.icon} size={12} color={mark.color} className={state === "running" ? "dep-spin" : undefined} />
                {s.label}
              </div>
              <span style={{ ...lbl, fontSize: 11, fontFamily: "var(--font-mono)" }}>{state === "running" ? "…" : state === "pending" ? "—" : formatStepSeconds(step?.duration_ms)}</span>
            </li>
          );
        })}
      </ol>
      {cause && (
        <div role="alert" style={{ display: "flex", alignItems: "center", gap: 12, padding: "14px 18px", borderRadius: 12, background: "rgba(239,68,68,.05)", border: "1px solid rgba(239,68,68,.18)" }}>
          <Icon name="CircleX" size={15} color="#f87171" />
          <span style={{ flex: 1, font: "400 12px/1.4 var(--font-sans)", color: "var(--text-body)" }}>{failedStage ? `${failedStage.label} : ${cause}` : cause}</span>
        </div>
      )}
    </div>
  );
}
