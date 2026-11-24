import { Link } from "@tanstack/react-router";
import { useState, type CSSProperties } from "react";

import { Badge, Button, Icon, IconButton, initials, KeyValueList, Modal, StatusDot } from "@/design-system";
import { downloadDeploymentLogs, LogViewer } from "@/features/logs/LogViewer";
import { timeAgo } from "@/features/projects/format";

import { isActive, type Deployment } from "./api";
import { DeploymentTimeline, StatusIcon } from "./DeploymentTimeline";
import { deploymentTitle, formatDuration, shortSha, STATUS_VIEW, TRIGGER_VIEW } from "./format";
import { useDeploymentActions } from "./useDeploymentActions";

// ProjectDeployments.jsx : panneau de droite (en-tête, actions, frise, logs).
// Écarts : pas de message de commit ni d'avatar (la charge utile ne les porte
// pas) ; « Visiter » absent (aucun domaine) ; le libellé « BuildKit · Railpack »
// est omis (la méthode de build n'est pas dans la charge utile) ; les logs
// viennent du flux en direct du déploiement (LogViewer). `details` ajoute la
// liste des détails de la page partageable.

const hair = "1px solid var(--border-subtle)";
const lbl: CSSProperties = { font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" };
const mono = (v: string, color?: string) => <span style={{ fontFamily: "var(--font-mono)", fontSize: 12, color }}>{v}</span>;

export function Author({ name }: { name: string }) {
  return (
    <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
      <span aria-hidden="true" style={{ width: 20, height: 20, borderRadius: 99, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", background: "var(--surface-selected)", color: "var(--text-body)", font: "600 9px/1 var(--font-sans)" }}>
        {initials(name)}
      </span>
      {name}
    </span>
  );
}

/** Confirmation avant de basculer la production vers une image conservée (ProjectDeployments.jsx#Modal). */
export function RollbackModal({ deployment: d, onClose, onConfirm }: { deployment: Deployment; onClose: () => void; onConfirm: () => void }) {
  const sha = shortSha(d.commit_sha);
  return (
    <Modal
      title={`Revenir à #${d.number} ?`}
      description={`Le trafic de production sera basculé vers l'image de #${d.number}${sha ? ` (${sha})` : ""} sans rebuild. Les variables d'environnement actuelles sont utilisées, pas celles de #${d.number}.`}
      onClose={onClose}
      width={480}
      footer={
        <>
          <Button onClick={onClose}>Annuler</Button>
          <Button variant="inverse" icon="Undo2" onClick={onConfirm}>
            Rollback
          </Button>
        </>
      }
    />
  );
}

export function DeploymentActions({ deployment: d }: { deployment: Deployment }) {
  const actions = useDeploymentActions(d.project_id);
  const [confirm, setConfirm] = useState(false);
  if (!actions.canDeploy) return null;
  return (
    <div style={{ display: "flex", gap: 6, flex: "none" }}>
      {isActive(d) && (
        <Button icon="Square" disabled={actions.busy || d.cancel_requested} onClick={() => void actions.cancel(d)}>
          {d.cancel_requested ? "Annulation demandée" : "Annuler"}
        </Button>
      )}
      {d.status === "success" && d.can_rollback && !d.current && (
        <Button icon="Undo2" disabled={actions.busy} onClick={() => setConfirm(true)}>
          Rollback
        </Button>
      )}
      {!isActive(d) && (
        <Button variant="inverse" icon="RotateCw" disabled={actions.busy || !actions.canRedeploy} title={actions.redeployHint} onClick={() => void actions.redeploy()}>
          Redéployer
        </Button>
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

function detailRows(d: Deployment) {
  const sha = shortSha(d.commit_sha);
  return [
    { label: "Projet", value: <Link to="/projects/$projectId" params={{ projectId: d.project_id }} style={{ color: "var(--text-strong)" }}>{d.project_name}</Link> },
    { label: "Environnement", value: d.environment },
    { label: "Déclencheur", value: TRIGGER_VIEW[d.trigger].label },
    ...(sha ? [{ label: "Commit", value: mono(sha, "var(--text-strong)") }] : []),
    ...(d.branch ? [{ label: "Branche", value: mono(d.branch, "var(--text-strong)") }] : []),
    ...(d.image ? [{ label: "Image", value: mono(d.image, "var(--text-strong)") }] : []),
    ...(d.rollback_of ? [{ label: "Rollback de", value: <Link to="/deployments/$deploymentId" params={{ deploymentId: d.rollback_of }} style={{ color: "var(--text-strong)" }}>Déploiement d'origine</Link> }] : []),
    { label: "Créé par", value: d.created_by ?? "—" },
    { label: "Créé", value: timeAgo(d.created_at) },
    { label: "Démarré", value: d.started_at ? timeAgo(d.started_at) : "—" },
    { label: "Terminé", value: d.finished_at ? timeAgo(d.finished_at) : "—" },
    { label: "Durée", value: formatDuration(d.duration_ms) },
  ];
}

export function DeploymentDetail({ deployment: d, details = false, link = false }: { deployment: Deployment; details?: boolean; link?: boolean }) {
  const status = STATUS_VIEW[d.status];
  const sha = shortSha(d.commit_sha);
  return (
    <div style={{ display: "flex", flexDirection: "column", borderRadius: "var(--radius-xl)", border: "1px solid var(--border-default)", overflow: "hidden" }}>
      <div style={{ display: "flex", alignItems: "flex-start", gap: 16, padding: "20px 20px 18px" }}>
        <StatusIcon status={d.status} size={36} />
        <div style={{ display: "flex", flexDirection: "column", gap: 9, flex: 1, minWidth: 0 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <h2 style={{ margin: 0, font: "500 16px/1.2 var(--font-sans)", color: "var(--text-strong)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{deploymentTitle(d)}</h2>
            <Badge>{d.environment}</Badge>
            {d.current && <Badge tone="positive">Actuel</Badge>}
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap", ...lbl }}>
            <StatusDot status={status.dot} label={d.cancel_requested && d.status === "running" ? "Annulation demandée" : status.label} />
            {mono(`#${d.number}`, "var(--text-muted)")}
            {sha && (
              <span style={{ display: "flex", alignItems: "center", gap: 5 }}>
                <Icon name="GitCommitHorizontal" size={12} />
                {mono(sha)}
              </span>
            )}
            {d.created_by && <Author name={d.created_by} />}
            <span>
              {timeAgo(d.created_at)}
              {d.duration_ms != null && ` · ${formatDuration(d.duration_ms)}`}
            </span>
            {link && (
              <Link to="/deployments/$deploymentId" params={{ deploymentId: d.id }} aria-label={`Ouvrir la page du déploiement #${d.number}`} style={{ display: "flex", alignItems: "center", gap: 4, color: "var(--text-muted)", textDecoration: "none" }}>
                <Icon name="ExternalLink" size={12} />
                Page
              </Link>
            )}
          </div>
        </div>
        <DeploymentActions deployment={d} />
      </div>
      <div style={{ padding: "18px 20px 20px", borderTop: hair }}>
        <DeploymentTimeline deployment={d} />
      </div>
      {details && (
        <section aria-label="Détails du déploiement" style={{ padding: "18px 20px 20px", borderTop: hair }}>
          <KeyValueList title="Détails" rows={detailRows(d)} />
        </section>
      )}
      <LogViewer
        deployment={d.id}
        layout={(body, reconnecting) => (
          <>
            <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "12px 14px 12px 20px", borderTop: hair }}>
              <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>Logs de build</span>
              {isActive(d) &&
                (reconnecting ? (
                  <span style={{ display: "flex", alignItems: "center", gap: 6, ...lbl }}>
                    <span style={{ width: 6, height: 6, borderRadius: 99, background: "var(--viz-2)" }}></span>reconnexion…
                  </span>
                ) : (
                  <span style={{ display: "flex", alignItems: "center", gap: 6, ...lbl }}>
                    <span className="live-ping" style={{ width: 6, height: 6, borderRadius: 99, background: "var(--green-500)" }}></span>en direct
                  </span>
                ))}
              <span style={{ flex: 1 }}></span>
              <IconButton size={28} variant="ghost" icon="Download" label="Télécharger" onClick={() => downloadDeploymentLogs(d.id)} />
            </div>
            <section aria-label="Logs de build" style={{ padding: "0 14px 14px" }}>
              {body}
            </section>
          </>
        )}
      />
    </div>
  );
}
