import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";

import { fetchProject, projectQueryKey } from "@/features/projects/api";
import { ProjectGit } from "@/features/projects/git/ProjectGit";

export const Route = createFileRoute("/_app/projects/$projectId/git")({
  component: GitTab,
});

function GitTab() {
  const { projectId } = Route.useParams();
  // Projet déjà chargé par la mise en page.
  const project = useQuery({ queryKey: projectQueryKey(projectId), queryFn: () => fetchProject(projectId), refetchOnMount: false });
  if (!project.data) return null;
  return (
    <div style={{ padding: "4px 32px 40px 34px" }}>
      <ProjectGit project={project.data} />
    </div>
  );
}
