import { useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useRef, useState, type CSSProperties, type ReactNode } from "react";

import { Button, Icon, Select, StatusDot, useToast } from "@/design-system";
import { useDeploymentActions } from "@/features/deployments/useDeploymentActions";

import { projectQueryKey, type Project } from "./api";
import { restartProject } from "./git/api";
import { sourceLabel, STATUS_VIEW } from "./format";

// Projects.jsx#ProjectDetail : en-tete et onglets. Les onglets deviennent des
// routes enfants ; ceux dont l'ecran arrive plus tard sont portes mais
// inactifs (ni lien, ni 404). Visiter est porte et desactive (aucun domaine) ;
// Redémarrer redemarre le deploiement servi et Redéployer lance un deploiement
// manuel (developer et plus, garde « déjà en cours » cote serveur aussi).

const TAB_STYLE: CSSProperties = { height: 38, padding: 0, background: "none", border: 0, marginBottom: -1, font: "500 13px/1 var(--font-sans)", display: "flex", alignItems: "center", gap: 6, textDecoration: "none" };
const COUNT: CSSProperties = { font: "400 11px/1 var(--font-sans)", color: "var(--text-faint)" };

/** Onglets de la maquette ; `to` absent : l'ecran arrive avec une tache suivante. */
const TABS = ["Services", "Déploiements", "Logs", "Variables", "Domaines", "Git"] as const;

const ACTIVE: CSSProperties = { ...TAB_STYLE, borderBottom: "1px solid var(--gray-950)", color: "var(--text-strong)" };
const IDLE: CSSProperties = { ...TAB_STYLE, borderBottom: "1px solid transparent", color: "var(--text-subtle)" };

export function ProjectTabs({ project }: { project: Project }) {
  return (
    <nav aria-label="Onglets du projet" style={{ display: "flex", gap: 22, borderBottom: "1px solid var(--border-subtle)" }}>
      <Link to="/projects/$projectId" params={{ projectId: project.id }} activeOptions={{ exact: true }} style={IDLE} activeProps={{ style: ACTIVE }}>
        Aperçu
      </Link>
      {TABS.map((label) =>
        label === "Services" ? (
          <Link key={label} to="/projects/$projectId/services" params={{ projectId: project.id }} style={IDLE} activeProps={{ style: ACTIVE }}>
            {label}
            <span style={COUNT}>{project.services.length}</span>
          </Link>
        ) : label === "Déploiements" ? (
          <Link key={label} to="/projects/$projectId/deployments" params={{ projectId: project.id }} style={IDLE} activeProps={{ style: ACTIVE }}>
            {label}
          </Link>
        ) : label === "Logs" ? (
          <Link key={label} to="/projects/$projectId/logs" params={{ projectId: project.id }} style={IDLE} activeProps={{ style: ACTIVE }}>
            {label}
          </Link>
        ) : label === "Variables" ? (
          <Link key={label} to="/projects/$projectId/variables" params={{ projectId: project.id }} style={IDLE} activeProps={{ style: ACTIVE }}>
            {label}
          </Link>
        ) : label === "Domaines" ? (
          <Link key={label} to="/projects/$projectId/domains" params={{ projectId: project.id }} style={IDLE} activeProps={{ style: ACTIVE }}>
            {label}
          </Link>
        ) : label === "Git" ? (
          <Link key={label} to="/projects/$projectId/git" params={{ projectId: project.id }} style={IDLE} activeProps={{ style: ACTIVE }}>
            {label}
          </Link>
        ) : (
          <span key={label} aria-disabled="true" title="Bientôt disponible" style={{ ...TAB_STYLE, borderBottom: "1px solid transparent", color: "var(--text-faint)", cursor: "not-allowed" }}>
            {label}
          </span>
        ),
      )}
    </nav>
  );
}

export function ProjectDetail({ project, children }: { project: Project; children: ReactNode }) {
  const actions = useDeploymentActions(project.id);
  const { flash } = useToast();
  const client = useQueryClient();
  const [restarting, setRestarting] = useState(false);
  const restartInFlight = useRef(false);
  const restart = async () => {
    if (restartInFlight.current) return;
    restartInFlight.current = true;
    setRestarting(true);
    try {
      const result = await restartProject(project.id);
      flash(result.ok ? "Redémarrage demandé" : result.message, result.ok ? undefined : 5000);
      // Un service arrêté repart : aperçu et services relus.
      if (result.ok) await client.invalidateQueries({ queryKey: projectQueryKey(project.id) });
    } finally {
      restartInFlight.current = false;
      setRestarting(false);
    }
  };
  const status = STATUS_VIEW[project.status];
  const environments = project.environments.map((e) => e.name);
  return (
    <div>
      <div style={{ padding: "20px 32px 0 46px", display: "flex", flexDirection: "column", gap: 20 }}>
        <Link to="/projects" style={{ alignSelf: "flex-start", display: "flex", alignItems: "center", gap: 6, color: "var(--text-subtle)", font: "400 12px/1 var(--font-sans)", textDecoration: "none" }}>
          <Icon name="ArrowLeft" size={13} />
          Projets
        </Link>
        <div style={{ display: "flex", alignItems: "flex-end", justifyContent: "space-between", gap: 24 }}>
          <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
              <h1 style={{ margin: 0, font: "500 22px/1.1 var(--font-sans)", color: "var(--text-strong)" }}>{project.name}</h1>
              <Select options={environments} value={environments[0]} />
            </div>
            <div style={{ display: "flex", gap: 18, alignItems: "center", font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" }}>
              <StatusDot status={status.dot} label={status.label} />
              <span style={{ display: "flex", gap: 6, alignItems: "center" }}>
                <Icon name={project.source.type === "git" ? "GitBranch" : "Container"} size={13} />
                <span style={{ fontFamily: "var(--font-mono)", fontSize: 12 }}>{sourceLabel(project.source)}</span>
              </span>
            </div>
          </div>
          <div style={{ display: "flex", gap: 8 }}>
            <Button icon="ExternalLink" disabled title="Aucun domaine configuré">
              Visiter
            </Button>
            <Button icon="RotateCcw" disabled={!actions.canDeploy || restarting} title={actions.canDeploy ? undefined : "Réservé aux rôles développeur et plus"} onClick={() => void restart()}>
              Redémarrer
            </Button>
            <Button variant="inverse" icon="Rocket" disabled={!actions.canRedeploy || actions.busy} title={actions.canDeploy ? actions.redeployHint : "Réservé aux rôles développeur et plus"} onClick={() => void actions.redeploy()}>
              Redéployer
            </Button>
          </div>
        </div>
        <ProjectTabs project={project} />
      </div>
      {children}
    </div>
  );
}
