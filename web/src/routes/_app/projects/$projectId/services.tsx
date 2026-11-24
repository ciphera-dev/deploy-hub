import { useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";

import { POLL_MS } from "@/features/deployments/api";
import { fetchProject, projectQueryKey } from "@/features/projects/api";
import { fetchServices, overviewQueryKey, servicesQueryKey, type ProjectService } from "@/features/projects/overview/api";
import { ProjectServices } from "@/features/projects/overview/ProjectServices";

export const Route = createFileRoute("/_app/projects/$projectId/services")({
  component: ServicesTab,
});

/** Rafraîchi tant qu'un service attend son déploiement. */
const pollWhileDeploying = (list: ProjectService[] | undefined): number | false => (list?.some((s) => s.state === "deploying") ? POLL_MS : false);

function ServicesTab() {
  const { projectId } = Route.useParams();
  const client = useQueryClient();
  // Projet déjà chargé par la mise en page (même clé : une seule requête).
  const project = useQuery({ queryKey: projectQueryKey(projectId), queryFn: () => fetchProject(projectId), refetchOnMount: false });
  const services = useQuery({ queryKey: servicesQueryKey(projectId), queryFn: () => fetchServices(projectId), refetchInterval: (q) => pollWhileDeploying(q.state.data) });
  const changed = () => {
    void client.invalidateQueries({ queryKey: servicesQueryKey(projectId) });
    void client.invalidateQueries({ queryKey: overviewQueryKey(projectId) });
  };
  if (services.isError) return <p role="alert" style={{ padding: "24px 32px 40px 34px", margin: 0, font: "400 13px/1.4 var(--font-sans)", color: "var(--text-muted)" }}>Chargement des services impossible pour le moment.</p>;
  if (!project.data || !services.data) return <p role="status" style={{ padding: "24px 32px 40px 34px", margin: 0, font: "400 13px/1.4 var(--font-sans)", color: "var(--text-subtle)" }}>Chargement des services…</p>;
  return <ProjectServices project={project.data} services={services.data} onChanged={changed} />;
}
