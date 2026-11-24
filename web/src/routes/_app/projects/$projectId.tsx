import { useQuery } from "@tanstack/react-query";
import { createFileRoute, Link, Outlet } from "@tanstack/react-router";

import { fetchProject, projectQueryKey } from "@/features/projects/api";
import { ProjectDetail } from "@/features/projects/ProjectDetail";
import { ApiError } from "@/lib/api/errors";

// Pas de titre statique : ProjectDetail porte son propre en-tete.
export const Route = createFileRoute("/_app/projects/$projectId")({
  component: ProjectLayout,
});

function ProjectLayout() {
  const { projectId } = Route.useParams();
  const project = useQuery({ queryKey: projectQueryKey(projectId), queryFn: () => fetchProject(projectId) });
  if (project.isPending) return null;
  if (!project.data) {
    const missing = project.error instanceof ApiError && project.error.status === 404;
    return (
      <div style={{ padding: "24px 32px", display: "flex", flexDirection: "column", gap: 12, font: "400 13px/1.5 var(--font-sans)", color: "var(--text-muted)" }}>
        <span>{missing ? "Projet introuvable." : "Chargement du projet impossible pour le moment."}</span>
        <Link to="/projects" style={{ color: "var(--text-strong)" }}>
          Retour aux projets
        </Link>
      </div>
    );
  }
  return (
    <ProjectDetail project={project.data}>
      <Outlet />
    </ProjectDetail>
  );
}
