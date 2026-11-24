import { createFileRoute, Outlet } from "@tanstack/react-router";

// Layout commun de la liste et de la page de détail (titre d'en-tête partagé).
export const Route = createFileRoute("/_app/deployments")({
  staticData: { title: "Déploiements" },
  component: Outlet,
});
