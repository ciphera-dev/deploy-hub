import { useQuery } from "@tanstack/react-query";
import { useState, type CSSProperties } from "react";

import { Badge, Icon } from "@/design-system";
import { timeAgo } from "@/features/projects/format";

import { deploymentsQueryKey, fetchDeployments, pollWhileActive, type Deployment } from "./api";
import { DeploymentDetail } from "./DeploymentDetail";
import { StatusIcon } from "./DeploymentTimeline";
import { deploymentTitle, TRIGGER_VIEW } from "./format";

// ProjectDeployments.jsx : timeline des déploiements du projet à gauche, détail
// du déploiement sélectionné à droite. Écarts : pas d'onglets d'environnement
// (production seul en v0.1), ni message de commit, avatar ou lien « Visiter »
// (absents de l'API) ; l'avancement vient de l'API (rafraîchie toutes les 3 s
// tant qu'un déploiement tourne), les logs en direct arrivent avec la Task 26.

const lbl: CSSProperties = { font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" };

function DeployItem({ deployment: d, on, last, onSelect }: { deployment: Deployment; on: boolean; last: boolean; onSelect: () => void }) {
  const [hover, setHover] = useState(false);
  const trigger = TRIGGER_VIEW[d.trigger];
  return (
    <div
      role="button"
      tabIndex={0}
      data-deployment={d.id}
      aria-current={on ? "true" : undefined}
      onClick={onSelect}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onSelect();
        }
      }}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{ position: "relative", display: "grid", gridTemplateColumns: "28px minmax(0,1fr)", gap: 14, padding: "14px 14px 14px 12px", borderRadius: "var(--radius-lg)", cursor: "pointer", background: on ? "var(--surface-raised)" : hover ? "rgba(255,255,255,.02)" : "transparent", border: "1px solid " + (on ? "var(--border-strong)" : "transparent") }}
    >
      {!last && <span style={{ position: "absolute", left: 25, top: 44, bottom: -14, width: 1, background: "var(--border-default)" }} />}
      <StatusIcon status={d.status} />
      <div style={{ display: "flex", flexDirection: "column", gap: 8, minWidth: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
          <span style={{ font: "500 13px/1.2 var(--font-sans)", color: "var(--text-strong)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", flex: 1 }}>{deploymentTitle(d)}</span>
          {d.current && <Badge tone="positive">Actuel</Badge>}
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 10, font: "400 11px/1 var(--font-sans)", color: "var(--text-subtle)", minWidth: 0, whiteSpace: "nowrap" }}>
          <span style={{ font: "400 12px/1 var(--font-mono)", color: "var(--text-muted)" }}>#{d.number}</span>
          {d.branch && (
            <span style={{ display: "flex", alignItems: "center", gap: 4 }}>
              <Icon name="GitBranch" size={11} />
              <span style={{ font: "400 12px/1 var(--font-mono)" }}>{d.branch}</span>
            </span>
          )}
          <span style={{ display: "flex", alignItems: "center", gap: 4 }}>
            <Icon name={trigger.icon} size={11} />
            {trigger.label}
          </span>
          <span style={{ flex: 1 }} />
          <span>{timeAgo(d.created_at)}</span>
        </div>
      </div>
    </div>
  );
}

export function ProjectDeployments({ projectId }: { projectId: string }) {
  const [sel, setSel] = useState<string | null>(null);
  const query = useQuery({ queryKey: deploymentsQueryKey(projectId), queryFn: () => fetchDeployments(projectId), refetchInterval: (q) => pollWhileActive(q.state.data) });
  const list = query.data ?? [];
  const current = list.find((d) => d.id === sel) ?? list[0];
  return (
    <div style={{ padding: "24px 32px 40px 34px" }}>
      {query.isError ? (
        <div role="alert" style={{ ...lbl, padding: "16px 12px" }}>
          Chargement des déploiements impossible pour le moment.
        </div>
      ) : !query.isSuccess ? (
        <div role="status" style={{ ...lbl, padding: "16px 12px" }}>
          Chargement des déploiements…
        </div>
      ) : !current ? (
        <div style={{ ...lbl, padding: "16px 12px" }}>Aucun déploiement pour ce projet.</div>
      ) : (
        <div style={{ display: "grid", gridTemplateColumns: "minmax(300px,380px) minmax(0,1fr)", gap: 24, alignItems: "start" }}>
          <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
            {list.map((d, i) => (
              <DeployItem key={d.id} deployment={d} on={d.id === current.id} last={i === list.length - 1} onSelect={() => setSel(d.id)} />
            ))}
          </div>
          <section aria-label="Détail du déploiement">
            <DeploymentDetail deployment={current} link />
          </section>
        </div>
      )}
    </div>
  );
}
