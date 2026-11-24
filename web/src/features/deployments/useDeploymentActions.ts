import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useRef, useState } from "react";

import { useToast } from "@/design-system";
import { fetchMe, ME_QUERY_KEY, MFA_PENDING } from "@/features/auth/api";

import { activeQueryKey, cancelDeployment, createDeployment, DEPLOYMENTS_QUERY_KEY, hasActiveDeployment, deploymentsQueryKey, fetchDeployments, pollWhileActive, rollbackDeployment, type ActionResult, type Deployment } from "./api";
import { inProgress } from "./format";

/** Tout rôle sauf `viewer` écrit sur les déploiements (matrice fixe de Team.jsx : owner, admin, ops, developer). */
export function useCanDeploy(): boolean {
  // Lecture seule du compte charge par le shell : ne relance jamais la requete (une requete en echec
  // repasserait en attente et le shell, qui ne rend rien avant de connaitre la session, bouclerait).
  const me = useQuery({ queryKey: ME_QUERY_KEY, queryFn: fetchMe, retryOnMount: false, refetchOnMount: false });
  return !!me.data && me.data !== MFA_PENDING && me.data.role !== "viewer";
}

export const GUARD_MESSAGE = "Un déploiement est déjà en cours";

/** Actions sur les déploiements (ProjectDeployments.jsx : redeploy, cancel, rollback), avec le garde « déjà en cours ». */
export function useDeploymentActions(projectId: string | undefined) {
  const client = useQueryClient();
  const { flash } = useToast();
  const canDeploy = useCanDeploy();
  const [busy, setBusy] = useState(false);
  // Garde synchrone : `busy` (état React) n'est visible qu'au rendu suivant, un second clic le précèderait.
  const inFlight = useRef(false);
  const guarded = async <T,>(fn: () => Promise<T | null>): Promise<T | null> => {
    if (inFlight.current) return null;
    inFlight.current = true;
    setBusy(true);
    try {
      return await fn();
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  };
  // Même clé que la liste du projet : une seule requête.
  const list = useQuery({ queryKey: deploymentsQueryKey(projectId), queryFn: () => fetchDeployments(projectId), refetchInterval: (q) => pollWhileActive(q.state.data) });
  const runningFor = (id: string | undefined) => inProgress(list.data?.filter((d) => d.project_id === id));

  const run = async (call: () => Promise<ActionResult>, done: (d: Deployment) => string) => {
    const result = await call();
    flash(result.ok ? done(result.deployment) : result.message, result.ok ? undefined : 5000);
    if (result.ok) await client.invalidateQueries({ queryKey: DEPLOYMENTS_QUERY_KEY });
    return result;
  };

  return {
    canDeploy,
    /** Déploiement en file ou en cours sur le projet. */
    running: runningFor(projectId),
    busy,
    /** Redéploie le projet ; sans effet (message) si un déploiement est déjà en cours. */
    /** « Redéployer » possible : rôle suffisant et liste connue (état inconnu = on n'agit pas). */
    canRedeploy: canDeploy && list.isSuccess,
    /** Explication quand l'état est inconnu (chargement ou erreur). */
    redeployHint: list.isSuccess ? undefined : list.isError ? "Liste des déploiements indisponible." : "Vérification des déploiements en cours…",
    redeploy: (id = projectId) =>
      guarded(async () => {
      if (!id || !list.isSuccess) return null;
      // Le serveur fait foi : un déploiement a pu démarrer depuis le dernier rafraîchissement.
      let active: boolean;
      try {
        active = runningFor(id) || (await client.fetchQuery({ queryKey: activeQueryKey(id), queryFn: () => hasActiveDeployment(id), staleTime: 0 }));
      } catch {
        flash("Vérification impossible. Réessayez.", 5000);
        return null;
      }
      if (active) {
        flash(GUARD_MESSAGE);
        return null;
      }
      return run(() => createDeployment(id), (d) => `Déploiement #${d.number} lancé`);
      }),
    cancel: (d: Deployment) => guarded(() => run(() => cancelDeployment(d.id), (x) => (x.status === "cancelled" ? `#${x.number} annulé` : `Annulation de #${x.number} demandée`))),
    rollback: (d: Deployment) => guarded(() => run(() => rollbackDeployment(d.id), (x) => `Rollback vers #${d.number} lancé (#${x.number})`)),
  };
}
