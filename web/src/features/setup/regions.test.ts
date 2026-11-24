import { expect, it } from "vitest";

import { languageOf, REGIONS, slugify } from "./regions";

it("slugify donne un identifiant accepte par le serveur", () => {
  expect(slugify("Acme")).toBe("acme");
  expect(slugify("  Société Générale & Fils !  ")).toBe("societe-generale-fils");
  expect(slugify("---")).toBe("ciphera");
  const long = slugify("a".repeat(30) + " " + "b".repeat(40));
  expect(long.length).toBeLessThanOrEqual(50);
  for (const name of ["Acme", "Ça va ?", "x".repeat(80), "日本"]) expect(slugify(name)).toMatch(/^[a-z0-9]([a-z0-9-]*[a-z0-9])?$/);
});

it("chaque region a un fuseau IANA et une langue prise en charge", () => {
  for (const region of REGIONS) {
    expect(region.timezone).toMatch(/^[A-Za-z]+\/[A-Za-z_]+$/);
    expect(["fr", "en"]).toContain(languageOf(region));
  }
  expect(languageOf(REGIONS.find((r) => r.city === "Londres")!)).toBe("en");
  expect(languageOf(REGIONS[0]!)).toBe("fr");
});
