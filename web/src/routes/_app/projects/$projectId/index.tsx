import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";

import { POLL_MS } from "@/features/deployments/api";
import { fetchProject, projectQueryKey } from "@/features/projects/api";
import { fetchOverview, overviewQueryKey, type ProjectOverviewData } from "@/features/projects/overview/api";
import { ProjectOverview } from "@/features/projects/overview/ProjectOverview";

export const Route = createFileRoute("/_app/projects/$projectId/")({
  component: OverviewTab,
});

/** Rafraîchi tant qu'un déploiement est en cours (statut, production, activité). */
const pollWhileBuilding = (o: ProjectOverviewData | undefined): number | false => (o?.status === "building" ? POLL_MS : false);

function OverviewTab() {
  const { projectId } = Route.useParams();
  // Projet déjà chargé par la mise en page (même clé : une seule requête).
  const project = useQuery({ queryKey: projectQueryKey(projectId), queryFn: () => fetchProject(projectId), refetchOnMount: false });
  const overview = useQuery({ queryKey: overviewQueryKey(projectId), queryFn: () => fetchOverview(projectId), refetchInterval: (q) => pollWhileBuilding(q.state.data) });
  if (overview.isError) return <p role="alert" style={{ padding: "24px 32px 40px 34px", margin: 0, font: "400 13px/1.4 var(--font-sans)", color: "var(--text-muted)" }}>Chargement de l'aperçu impossible pour le moment.</p>;
  if (!project.data || !overview.data) return <p role="status" style={{ padding: "24px 32px 40px 34px", margin: 0, font: "400 13px/1.4 var(--font-sans)", color: "var(--text-subtle)" }}>Chargement de l'aperçu…</p>;
  return <ProjectOverview project={project.data} overview={overview.data} />;
}
