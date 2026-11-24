import { useQuery } from "@tanstack/react-query";
import { createFileRoute, Link } from "@tanstack/react-router";

import { Icon } from "@/design-system";
import { deploymentQueryKey, fetchDeployment, isActive, POLL_MS } from "@/features/deployments/api";
import { DeploymentDetail } from "@/features/deployments/DeploymentDetail";
import { ApiError } from "@/lib/api/errors";

// Détail partageable par URL : /deployments/:id (viewer et plus).
export const Route = createFileRoute("/_app/deployments/$deploymentId")({
  component: DeploymentPage,
});

const back = (
  <Link to="/deployments" aria-label="Retour aux déploiements" style={{ alignSelf: "flex-start", display: "flex", alignItems: "center", gap: 6, color: "var(--text-subtle)", font: "400 12px/1 var(--font-sans)", textDecoration: "none" }}>
    <Icon name="ArrowLeft" size={13} />
    Déploiements
  </Link>
);

function DeploymentPage() {
  const { deploymentId } = Route.useParams();
  const query = useQuery({ queryKey: deploymentQueryKey(deploymentId), queryFn: () => fetchDeployment(deploymentId), refetchInterval: (q) => (q.state.data && isActive(q.state.data) ? POLL_MS : false) });
  const missing = query.error instanceof ApiError && query.error.status === 404;
  return (
    <div style={{ padding: "20px 32px 40px 34px", display: "flex", flexDirection: "column", gap: 16 }}>
      {back}
      {query.data ? (
        <DeploymentDetail deployment={query.data} details />
      ) : query.isError ? (
        <span style={{ font: "400 13px/1.5 var(--font-sans)", color: "var(--text-muted)" }}>{missing ? "Déploiement introuvable." : "Chargement du déploiement impossible pour le moment."}</span>
      ) : null}
    </div>
  );
}
