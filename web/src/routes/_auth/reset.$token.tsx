import { createFileRoute, redirect } from "@tanstack/react-router";

import { stashToken } from "@/lib/url-token";

/** Lien de réinitialisation : le jeton passe en mémoire, l'URL perd le jeton (même page). */
export const Route = createFileRoute("/_auth/reset/$token")({
  beforeLoad: ({ params }) => {
    stashToken("reset", params.token);
    throw redirect({ to: "/reset", replace: true });
  },
});
