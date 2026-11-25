import { createFileRoute, redirect } from "@tanstack/react-router";

import { stashToken } from "@/lib/url-token";

/** Lien de vérification d'adresse : le jeton passe en mémoire, l'URL perd le jeton (même page). */
export const Route = createFileRoute("/_auth/verify-email/$token")({
  beforeLoad: ({ params }) => {
    stashToken("verify-email", params.token);
    throw redirect({ to: "/verify-email", replace: true });
  },
});
