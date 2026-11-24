import { useQuery, useQueryClient } from "@tanstack/react-query";
import { createFileRoute, Navigate, useNavigate } from "@tanstack/react-router";
import { useState } from "react";

import { ErrorScreen } from "@/app/errors/ErrorScreen";
import { Button } from "@/design-system";
import { fetchMe, logout, ME_QUERY_KEY, MFA_PENDING } from "@/features/auth/api";
import { EnrollTotp } from "@/features/profile/security/EnrollTotp";
import { ApiError } from "@/lib/api/errors";

/** Enrolement impose par l'organisation : seul ecran ouvert a une session sans 2FA. */
export const Route = createFileRoute("/_auth/2fa-setup")({
  component: ForcedEnrollmentPage,
});

function ForcedEnrollmentPage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [signOutError, setSignOutError] = useState("");
  // GET /api/me d'abord (rechargement de la page) : il retient le jeton CSRF
  // exige par l'enrolement et la deconnexion, et dit si l'enrolement est
  // toujours impose.
  const me = useQuery({ queryKey: ME_QUERY_KEY, queryFn: fetchMe });

  const done = () => {
    // Le shell relit GET /api/me : le compte n'est plus restreint a l'enrolement.
    queryClient.removeQueries({ queryKey: ME_QUERY_KEY });
    void navigate({ to: "/", replace: true });
  };
  const signOut = async () => {
    setSignOutError("");
    try {
      await logout();
    } catch (error) {
      // 401 : la session est deja fermee (expiree ou revoquee).
      if (!(error instanceof ApiError && error.status === 401)) {
        // La session reste ouverte : ne jamais laisser croire le contraire.
        setSignOutError("Déconnexion impossible pour le moment. Réessayez.");
        return;
      }
    }
    // Plus aucune donnee de la session ne doit rester en cache.
    queryClient.clear();
    await navigate({ to: "/login", replace: true });
  };

  if (me.isPending) return null;
  if (me.isError) {
    return (
      <ErrorScreen icon="ServerCrash" title="Impossible de charger votre compte" description="La vérification de votre session a échoué. Réessayez dans un instant.">
        <Button variant="inverse" icon="RotateCw" disabled={me.isFetching} onClick={() => void me.refetch()}>
          Réessayer
        </Button>
      </ErrorScreen>
    );
  }
  if (me.data === null) return <Navigate to="/login" replace />;
  if (me.data === MFA_PENDING) return <Navigate to="/2fa" replace />;
  if (!me.data.mfa_enrollment_required) return <Navigate to="/" replace />;
  return <EnrollTotp forced onDone={done} onClose={() => undefined} onSignOut={() => void signOut()} signOutError={signOutError} />;
}
