import { useQuery } from "@tanstack/react-query";
import { useState } from "react";

import { Button, Menu, Tabs } from "@/design-system";
import { fetchProjects, PROJECTS_QUERY_KEY } from "@/features/projects/api";

import { deploymentsQueryKey, fetchDeployments, isActive, pollWhileActive, type Deployment } from "./api";
import { DeploymentsTable } from "./DeploymentsTable";
import { useDeploymentActions } from "./useDeploymentActions";

// Pages2.jsx#DeploymentsPage : bandeau, onglets et liste de tous les projets.
// Écart : le graphe des 3 métriques (déploiements / jour, taux de réussite,
// durée moyenne) est laissé de côté, faute de série d'agrégats dans l'API v0.1
// (Task 28/31). Les projets de « Déployer un projet » viennent de GET /api/projects.

type Tab = "all" | "run" | "ok" | "err";

const FILTERS: Record<Tab, (d: Deployment) => boolean> = {
  all: () => true,
  run: isActive,
  ok: (d) => d.status === "success",
  err: (d) => d.status === "failed",
};

const WEEK_MS = 7 * 86_400_000;

export function DeploymentsPage() {
  const [tab, setTab] = useState<Tab>("all");
  const deployments = useQuery({ queryKey: deploymentsQueryKey(), queryFn: () => fetchDeployments(), refetchInterval: (q) => pollWhileActive(q.state.data) });
  const projects = useQuery({ queryKey: PROJECTS_QUERY_KEY, queryFn: fetchProjects });
  const actions = useDeploymentActions(undefined);
  const all = deployments.data ?? [];
  const running = all.filter(FILTERS.run).length;
  const failedThisWeek = all.filter((d) => d.status === "failed" && deployments.dataUpdatedAt - Date.parse(d.created_at) < WEEK_MS).length;
  const count = (t: Tab) => all.filter(FILTERS[t]).length;
  return (
    <div style={{ padding: "20px 32px 40px 34px", display: "flex", flexDirection: "column", gap: 14 }}>
      <div style={{ borderRadius: 18, border: "1px solid var(--border-default)", background: "var(--surface-app)", position: "relative", overflow: "hidden", display: "flex", alignItems: "center", gap: 24, padding: "26px 28px" }}>
        <span style={{ position: "absolute", inset: 0, background: "radial-gradient(50% 140% at 100% 0%,rgba(56,189,248,.14),transparent 70%)", pointerEvents: "none" }} />
        <span style={{ position: "relative", display: "flex", flexDirection: "column", gap: 10, flex: 1, minWidth: 0 }}>
          <span style={{ font: "600 18px/1.2 var(--font-sans)", color: "var(--text-strong)" }}>Chaque push en ligne en moins d'une minute</span>
          <span style={{ font: "400 13px/1.55 var(--font-sans)", color: "var(--text-muted)", maxWidth: 520 }}>
            Build, image et bascule sans coupure. {running ? `${running} déploiement${running > 1 ? "s" : ""} en cours` : "Aucun déploiement en cours"} · {failedThisWeek} échec{failedThisWeek > 1 ? "s" : ""} cette semaine.
          </span>
        </span>
        {actions.canDeploy && (
          <div style={{ position: "relative" }}>
            <Menu
              label="Choisir un projet"
              width={300}
              trigger={
                <Button variant="inverse" icon="Rocket">
                  Déployer un projet
                </Button>
              }
              items={(projects.data ?? []).map((p) => ({ icon: "Layers" as const, label: p.name, disabled: !actions.canRedeploy, onSelect: () => void actions.redeploy(p.id) }))}
            />
          </div>
        )}
      </div>
      <div style={{ paddingLeft: 12, marginTop: 6 }}>
        <Tabs
          tabs={[
            { value: "all", label: "Tous", count: count("all") },
            { value: "run", label: "En cours", count: count("run") },
            { value: "ok", label: "Réussis", count: count("ok") },
            { value: "err", label: "Échecs", count: count("err") },
          ]}
          value={tab}
          onChange={(v) => setTab(v as Tab)}
        />
      </div>
      {deployments.isError ? (
        <div role="alert" style={{ padding: "12px 0", textAlign: "center", font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" }}>
          Chargement des déploiements impossible pour le moment.
        </div>
      ) : deployments.isSuccess ? (
        <DeploymentsTable deployments={all.filter(FILTERS[tab])} actions={actions} />
      ) : (
        <div role="status" style={{ padding: "12px 0", textAlign: "center", font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" }}>
          Chargement des déploiements…
        </div>
      )}
    </div>
  );
}
