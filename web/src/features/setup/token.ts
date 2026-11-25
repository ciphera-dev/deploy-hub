/**
 * Jeton d'installation lu dans l'adresse : fragment `#token=…` (forme actuelle),
 * sinon l'ancien `?token=…`. `hash` peut commencer ou non par `#`.
 */
export function tokenFromLocation(hash: string, search: unknown): string | undefined {
  const fromHash = new URLSearchParams(hash.replace(/^#/, "")).get("token");
  if (fromHash) return fromHash;
  const fromSearch =
    typeof search === "object" && search !== null ? (search as { token?: unknown }).token : undefined;
  return typeof fromSearch === "string" && fromSearch ? fromSearch : undefined;
}

/** Retire le jeton de la barre d'adresse (historique compris) sans recharger la page. */
export function stripTokenFromAddressBar(): void {
  const url = new URL(window.location.href);
  const inSearch = url.searchParams.has("token");
  const inHash = new URLSearchParams(url.hash.replace(/^#/, "")).has("token");
  if (!inSearch && !inHash) return;
  url.searchParams.delete("token");
  window.history.replaceState(window.history.state, "", url.pathname + url.search + (inHash ? "" : url.hash));
}
