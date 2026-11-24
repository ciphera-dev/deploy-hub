import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";

import { ProjectDomains } from "@/features/domains/ProjectDomains";
import { fetchProject, projectQueryKey } from "@/features/projects/api";

export const Route = createFileRoute("/_app/projects/$projectId/domains")({
  component: DomainsTab,
});

function DomainsTab() {
  const { projectId } = Route.useParams();
  // Projet déjà chargé par la mise en page.
  const project = useQuery({ queryKey: projectQueryKey(projectId), queryFn: () => fetchProject(projectId), refetchOnMount: false });
  if (!project.data) return null;
  return (
    <div style={{ padding: "16px 32px 40px 34px" }}>
      <ProjectDomains project={project.data} />
    </div>
  );
}
