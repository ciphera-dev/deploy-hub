import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";

import { fetchProject, projectQueryKey } from "@/features/projects/api";
import { ProjectVariables } from "@/features/variables/ProjectVariables";

export const Route = createFileRoute("/_app/projects/$projectId/variables")({
  component: VariablesTab,
});

function VariablesTab() {
  const { projectId } = Route.useParams();
  // Projet déjà chargé par la mise en page.
  const project = useQuery({ queryKey: projectQueryKey(projectId), queryFn: () => fetchProject(projectId), refetchOnMount: false });
  if (!project.data) return null;
  return (
    <div style={{ padding: "16px 32px 40px 34px" }}>
      <ProjectVariables project={project.data} />
    </div>
  );
}
