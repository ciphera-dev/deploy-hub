// Session expiree detectee hors de react-query (appels directs de l'API depuis des
// formulaires) : l'application branche ici la meme politique que pour un 401 de requete
// (cache vide, redirection vers la page de session expiree).

let handler: (() => void) | null = null;

export function setSessionExpiredHandler(fn: (() => void) | null): void {
  handler = fn;
}

export function notifySessionExpired(): void {
  handler?.();
}

export const SESSION_EXPIRED_MESSAGE = "Votre session a expiré. Reconnectez-vous.";
