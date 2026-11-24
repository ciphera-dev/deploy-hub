import { useSyncExternalStore } from "react";

import type { FatalKind } from "@/app/query-client";

// Etat minimal partage entre la politique d'erreur (hors React) et le shell.
let current: FatalKind | null = null;
const listeners = new Set<() => void>();

function emit() {
  for (const l of listeners) l();
}

export const fatalError = {
  get: () => current,
  set(kind: FatalKind) {
    if (current === kind) return;
    current = kind;
    emit();
  },
  clear() {
    if (current === null) return;
    current = null;
    emit();
  },
  subscribe(listener: () => void) {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  },
};

export function useFatalError(): FatalKind | null {
  return useSyncExternalStore(fatalError.subscribe, fatalError.get, fatalError.get);
}

// Erreurs de mutation : evenements ponctuels (toast), jamais un etat qui demonte la page.
const mutationListeners = new Set<(kind: FatalKind) => void>();

export const mutationErrors = {
  notify(kind: FatalKind) {
    for (const l of mutationListeners) l(kind);
  },
  subscribe(listener: (kind: FatalKind) => void) {
    mutationListeners.add(listener);
    return () => {
      mutationListeners.delete(listener);
    };
  },
};

export const MUTATION_ERROR_MESSAGES: Record<FatalKind, string> = {
  forbidden: "Action refusée : votre rôle ne le permet pas.",
  offline: "API injoignable : l'action n'a pas été envoyée.",
  server: "Erreur du serveur : l'action a échoué.",
};
