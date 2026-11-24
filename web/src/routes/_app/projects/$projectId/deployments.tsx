import { createFileRoute } from "@tanstack/react-router";

import { ProjectDeployments } from "@/features/deployments/ProjectDeployments";

export const Route = createFileRoute("/_app/projects/$projectId/deployments")({
  component: DeploymentsTab,
});

function DeploymentsTab() {
  const { projectId } = Route.useParams();
  return <ProjectDeployments projectId={projectId} />;
}
