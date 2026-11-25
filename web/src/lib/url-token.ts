// Jeton d'un lien à usage unique (réinitialisation, invitation, vérification d'adresse,
// installation). La route `/…/$token` (ou `/setup#token=…`) le range ici puis redirige (remplacement d'entrée d'historique,
// par le routeur) vers la même page sans jeton : il quitte la barre d'adresse, l'URL
// copiée-collée et la restauration d'onglet. Il reste en mémoire le temps de la page ;
// un rechargement le perd (le lien est à rouvrir depuis l'e-mail). L'historique global
// du navigateur garde la visite de l'URL d'origine : le jeton reste à usage unique.

export type TokenKind = "reset" | "invite" | "verify-email" | "setup";

const held = new Map<TokenKind, string>();

/** Garde le jeton lu dans l'URL. */
export function stashToken(kind: TokenKind, token: string): void {
  held.set(kind, token);
}

/** Jeton gardé en mémoire, `undefined` après un rechargement. */
export function heldToken(kind: TokenKind): string | undefined {
  return held.get(kind);
}

/** Oublie le jeton d'un lien, une fois utilisé. */
export function forgetToken(kind: TokenKind): void {
  held.delete(kind);
}

/** Oublie tous les jetons (tests). */
export function clearHeldTokens(): void {
  held.clear();
}
