import { setNonce } from "get-nonce";

/** Marqueur du `index.html` source : le serveur le remplace par le nonce de la réponse. */
const PLACEHOLDER = "__CSP_NONCE__";

/** Nonce posé par le serveur dans `<meta name="csp-nonce">` ; `undefined` hors serveur (dev, tests). */
export function readCspNonce(doc: Document = document): string | undefined {
  const content = doc.querySelector('meta[name="csp-nonce"]')?.getAttribute("content")?.trim();
  return content && content !== PLACEHOLDER ? content : undefined;
}

/**
 * Donne le nonce aux styles injectés par Radix (react-style-singleton) : sans lui,
 * `style-src 'self' 'nonce-…'` les refuse. À appeler avant le premier rendu.
 */
export function applyCspNonce(doc: Document = document): void {
  const nonce = readCspNonce(doc);
  if (nonce) setNonce(nonce);
}
