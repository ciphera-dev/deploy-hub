/** Copie un texte ; `true` seulement si le navigateur l'a réellement copié. */
export async function copyText(text: string): Promise<boolean> {
  const clip = typeof navigator !== "undefined" ? navigator.clipboard : undefined;
  if (!clip) return false;
  try {
    await clip.writeText(text);
    return true;
  } catch {
    return false;
  }
}
