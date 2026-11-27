import { useSyncExternalStore } from "react";

/** Suit une media query (faux sans matchMedia, par exemple sous jsdom). */
export function useMedia(query: string): boolean {
  return useSyncExternalStore(
    (notify) => {
      if (typeof window.matchMedia !== "function") return () => undefined;
      const mq = window.matchMedia(query);
      mq.addEventListener("change", notify);
      return () => mq.removeEventListener("change", notify);
    },
    () => typeof window.matchMedia === "function" && window.matchMedia(query).matches,
    () => false,
  );
}

/** Sous ce seuil, la barre laterale devient un tiroir. */
export const MOBILE_QUERY = "(max-width: 767.98px)";
/** Sous ce seuil, le logo et le nom de l'espace de l'en-tete sont reduits. */
export const NARROW_QUERY = "(max-width: 479.98px)";
