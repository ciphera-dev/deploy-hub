/** Retire le jeton de la barre d'adresse (historique compris) sans recharger la page. */
export function stripTokenFromAddressBar(): void {
  const url = new URL(window.location.href);
  if (!url.searchParams.has("token")) return;
  url.searchParams.delete("token");
  window.history.replaceState(window.history.state, "", url.pathname + url.search + url.hash);
}

