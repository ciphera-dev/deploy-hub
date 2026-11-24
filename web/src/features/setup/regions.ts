/** Villes proposees par l'assistant (maquette install.html) : fuseau IANA et langue de l'instance. */
export interface Region {
  city: string;
  country: string;
  /** Code pays affiche dans la pastille */
  cc: string;
  timezone: string;
}

export const REGIONS: readonly Region[] = [
  { city: "Paris", country: "France", cc: "FR", timezone: "Europe/Paris" },
  { city: "Lyon", country: "France", cc: "FR", timezone: "Europe/Paris" },
  { city: "Marseille", country: "France", cc: "FR", timezone: "Europe/Paris" },
  { city: "Zurich", country: "Suisse", cc: "CH", timezone: "Europe/Zurich" },
  { city: "Genève", country: "Suisse", cc: "CH", timezone: "Europe/Zurich" },
  { city: "Lausanne", country: "Suisse", cc: "CH", timezone: "Europe/Zurich" },
  { city: "Bruxelles", country: "Belgique", cc: "BE", timezone: "Europe/Brussels" },
  { city: "Luxembourg", country: "Luxembourg", cc: "LU", timezone: "Europe/Luxembourg" },
  { city: "Francfort", country: "Allemagne", cc: "DE", timezone: "Europe/Berlin" },
  { city: "Berlin", country: "Allemagne", cc: "DE", timezone: "Europe/Berlin" },
  { city: "Londres", country: "Royaume-Uni", cc: "GB", timezone: "Europe/London" },
  { city: "Amsterdam", country: "Pays-Bas", cc: "NL", timezone: "Europe/Amsterdam" },
  { city: "Montréal", country: "Canada", cc: "CA", timezone: "America/Toronto" },
  { city: "New York", country: "États-Unis", cc: "US", timezone: "America/New_York" },
  { city: "Casablanca", country: "Maroc", cc: "MA", timezone: "Africa/Casablanca" },
  { city: "Dakar", country: "Sénégal", cc: "SN", timezone: "Africa/Dakar" },
];

const FRENCH_SPEAKING = new Set(["FR", "CH", "BE", "LU", "CA", "MA", "SN"]);

/** Langue de l'instance deduite du pays de la region (`fr` ou `en`). */
export function languageOf(region: Region): "fr" | "en" {
  return FRENCH_SPEAKING.has(region.cc) ? "fr" : "en";
}

/** Identifiant d'organisation : minuscules, chiffres et tirets, 50 caracteres au plus. */
export function slugify(name: string): string {
  const slug = name
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 50)
    .replace(/-+$/g, "");
  return slug || "ciphera";
}
