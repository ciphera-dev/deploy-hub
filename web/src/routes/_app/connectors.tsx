import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useCallback } from "react";

import { ConnectorsPage } from "@/features/connectors/ConnectorsPage";

// Retour de GitHub : `?code=…&state=…` (flux manifest) ou `?installation_id=…`
// (installation de l'App). Valeurs bornees a un jeu de caracteres etroit.
type ConnectorsSearch = { code?: string; state?: string; installation_id?: string };

const token = (value: unknown) => (typeof value === "string" && /^[A-Za-z0-9_-]{1,128}$/.test(value) ? value : undefined);
const digits = (value: unknown) => {
  const text = typeof value === "number" ? String(value) : value;
  return typeof text === "string" && /^\d{1,20}$/.test(text) ? text : undefined;
};

export const Route = createFileRoute("/_app/connectors")({
  staticData: { title: "Connecteurs" },
  validateSearch: (search: Record<string, unknown>): ConnectorsSearch => {
    const code = token(search.code);
    const state = token(search.state);
    const installation = digits(search.installation_id);
    return { ...(code ? { code } : {}), ...(state ? { state } : {}), ...(installation ? { installation_id: installation } : {}) };
  },
  component: ConnectorsRoute,
});

function ConnectorsRoute() {
  const search = Route.useSearch();
  const navigate = useNavigate();
  const clearSearch = useCallback(() => void navigate({ to: "/connectors", search: {}, replace: true }), [navigate]);
  return <ConnectorsPage search={search} clearSearch={clearSearch} />;
}
