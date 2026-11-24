import { createRouter, type RouterHistory } from "@tanstack/react-router";

import { routeTree } from "@/routeTree.gen";

export function createAppRouter(history?: RouterHistory) {
  return createRouter({ routeTree, history, defaultPreload: "intent" });
}

export const router = createAppRouter();
export type AppRouter = typeof router;

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
  interface StaticDataRouteOption {
    /** Titre de page affiche par DashboardHeader */
    title?: string;
  }
}
