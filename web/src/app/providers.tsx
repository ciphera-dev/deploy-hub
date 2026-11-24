import { QueryClientProvider, type QueryClient } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";

import { ToastProvider } from "@/design-system";

import { fatalError, mutationErrors } from "./errors/fatal-error-store";
import { createQueryClient } from "./query-client";
import { router, type AppRouter } from "./router";

/** Politique d'erreur de l'application, branchee sur un routeur donne. */
export function createAppQueryClient(target: AppRouter): QueryClient {
  const client: QueryClient = createQueryClient({
    onUnauthorized: () => {
      // Deja sur la connexion : rien a faire (l'ecran gere ses propres erreurs).
      if (target.state.location.pathname === "/login") return;
      // Plus aucune donnee de la session precedente ne doit rester en cache.
      client.clear();
      fatalError.clear();
      void target.navigate({ to: "/login", search: { expired: 1 } });
    },
    onFatal: (kind) => fatalError.set(kind),
    onMutationError: (kind) => mutationErrors.notify(kind),
  });
  return client;
}

export const queryClient = createAppQueryClient(router);

export interface AppProvidersProps {
  router?: AppRouter;
  client?: QueryClient;
}

export function AppProviders({ router: r = router, client = queryClient }: AppProvidersProps) {
  return (
    <QueryClientProvider client={client}>
      <ToastProvider>
        <RouterProvider router={r} />
      </ToastProvider>
    </QueryClientProvider>
  );
}
