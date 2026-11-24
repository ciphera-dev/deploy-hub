import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";

import { containersOf } from "@/features/logs/containers";
import { LogViewer } from "@/features/logs/LogViewer";
import { fetchProject, projectQueryKey } from "@/features/projects/api";
import { fetchServices, servicesQueryKey } from "@/features/projects/overview/api";

export const Route = createFileRoute("/_app/projects/$projectId/logs")({
  component: LogsTab,
});

function LogsTab() {
  const { projectId } = Route.useParams();
  // Projet déjà chargé par la mise en page ; services et leur état : même clé que l'onglet Services.
  const project = useQuery({ queryKey: projectQueryKey(projectId), queryFn: () => fetchProject(projectId), refetchOnMount: false });
  const services = useQuery({ queryKey: servicesQueryKey(projectId), queryFn: () => fetchServices(projectId) });
  if (!project.data || !services.data) return null;
  return (
    <div style={{ padding: "24px 32px 40px 34px" }}>
      <LogViewer project={project.data.name} containers={containersOf(services.data)} />
    </div>
  );
}
