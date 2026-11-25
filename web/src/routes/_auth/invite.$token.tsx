import { createFileRoute, redirect } from "@tanstack/react-router";

import { stashToken } from "@/lib/url-token";

/** Lien d'invitation : le jeton passe en mémoire, l'URL perd le jeton (même page). */
export const Route = createFileRoute("/_auth/invite/$token")({
  beforeLoad: ({ params }) => {
    stashToken("invite", params.token);
    throw redirect({ to: "/invite", replace: true });
  },
});
