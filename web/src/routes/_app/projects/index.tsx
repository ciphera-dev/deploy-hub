import { createFileRoute, useNavigate } from "@tanstack/react-router";

import { ProjectsList } from "@/features/projects/ProjectsList";

export const Route = createFileRoute("/_app/projects/")({
  staticData: { title: "Projets" },
  component: ProjectsPage,
});

function ProjectsPage() {
  const navigate = useNavigate();
  return <ProjectsList open={(projectId) => void navigate({ to: "/projects/$projectId", params: { projectId } })} />;
}
