import { useSyncExternalStore } from "react";

/**
 * Points de rupture de l'ossature (le CSS de index.css utilise les memes valeurs) :
 * mobile < 768 px (barre d'onglets), tablette 768 a 1023 px (rail d'icones), bureau >= 1024 px.
 */
export const MOBILE_QUERY = "(max-width: 767.98px)";
export const TABLET_QUERY = "(min-width: 768px) and (max-width: 1023.98px)";
/** Sous ce seuil, le logo et le nom de l'espace de l'en-tete sont reduits. */
export const NARROW_QUERY = "(max-width: 479.98px)";

/** Etat d'une media query hors rendu ; faux sans matchMedia (rendu serveur, jsdom). */
export function matches(query: string): boolean {
  return typeof window !== "undefined" && typeof window.matchMedia === "function" && window.matchMedia(query).matches;
}

/** Suit une media query. */
export function useMedia(query: string): boolean {
  return useSyncExternalStore(
    (notify) => {
      if (typeof window === "undefined" || typeof window.matchMedia !== "function") return () => undefined;
      const mq = window.matchMedia(query);
      mq.addEventListener("change", notify);
      return () => mq.removeEventListener("change", notify);
    },
    () => matches(query),
    () => false,
  );
}
