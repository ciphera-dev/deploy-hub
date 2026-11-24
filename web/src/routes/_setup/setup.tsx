import { createFileRoute } from "@tanstack/react-router";
import { useEffect, useState } from "react";

import { SetupWizard } from "@/features/setup/SetupWizard";
import { stripTokenFromAddressBar } from "@/features/setup/token";

// Route /setup : l'adresse imprimee par `ciphera init` porte le jeton (?token=...).
export const Route = createFileRoute("/_setup/setup")({
  validateSearch: (search: Record<string, unknown>): { token?: string } =>
    typeof search.token === "string" && search.token ? { token: search.token } : {},
  component: SetupPage,
});

function SetupPage() {
  const { token } = Route.useSearch();
  // Lu une seule fois et garde en memoire : jamais dans localStorage ni sessionStorage.
  const [kept] = useState<string | null>(token ?? null);
  useEffect(stripTokenFromAddressBar, []);
  return <SetupWizard token={kept} />;
}
