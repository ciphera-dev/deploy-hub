import { createRootRoute, Outlet } from "@tanstack/react-router";

import { NotFound } from "@/app/errors/NotFound";
import { ServerError } from "@/app/errors/ServerError";

export const Route = createRootRoute({
  component: Outlet,
  notFoundComponent: NotFound,
  // Erreur de rendu non rattrapee : ecran 500 avec relance de la route.
  errorComponent: ({ reset }) => <ServerError onRetry={reset} />,
});
