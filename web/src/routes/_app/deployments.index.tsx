import { createFileRoute } from "@tanstack/react-router";

import { DeploymentsPage } from "@/features/deployments/DeploymentsPage";

export const Route = createFileRoute("/_app/deployments/")({
  component: DeploymentsPage,
});
