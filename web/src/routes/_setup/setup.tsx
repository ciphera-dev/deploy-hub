import { createFileRoute, redirect } from "@tanstack/react-router";
import { useEffect, useState } from "react";

import { SetupWizard } from "@/features/setup/SetupWizard";
import { stripTokenFromAddressBar, tokenFromLocation } from "@/features/setup/token";
import { heldToken, stashToken } from "@/lib/url-token";

// Route /setup : l'adresse imprimee par `ciphera init` (ou `ciphera admin setup-token`)
// porte le jeton dans le fragment (#token=...), jamais envoye au serveur ni journalise.
// L'ancien ?token= reste accepte. Le jeton est range en memoire puis la page est
// rechargee sans lui (remplacement d'entree d'historique).
export const Route = createFileRoute("/_setup/setup")({
  beforeLoad: ({ location }) => {
    const token = tokenFromLocation(location.hash, location.search);
    if (token) {
      stashToken("setup", token);
      throw redirect({ to: "/setup", replace: true });
    }
  },
  component: SetupPage,
});

function SetupPage() {
  // Lu une seule fois et garde en memoire : jamais dans localStorage ni sessionStorage.
  const [kept] = useState<string | null>(() => heldToken("setup") ?? null);
  useEffect(stripTokenFromAddressBar, []);
  return <SetupWizard token={kept} />;
}
