// Ligne `.env` d'une valeur publique, entre guillemets si besoin : même
// format que l'export du serveur (relu par `PUT …/variables/bulk`).
const PLAIN = /^[A-Za-z0-9_\-./:@%+,=?&~*^!$[\]{}()<>;|]*$/;
const BACKSLASH = String.fromCharCode(92);
const ESCAPES: Record<string, string> = {
  [BACKSLASH]: BACKSLASH + BACKSLASH,
  '"': BACKSLASH + '"',
  "\n": BACKSLASH + "n",
  "\r": BACKSLASH + "r",
};

export function dotenvLine(key: string, value: string): string {
  if (PLAIN.test(value)) return `${key}=${value}`;
  const quoted = Array.from(value, (c) => ESCAPES[c] ?? c).join("");
  return `${key}="${quoted}"`;
}
