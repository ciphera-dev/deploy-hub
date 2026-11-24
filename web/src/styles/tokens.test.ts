import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const sources = import.meta.glob("/src/**/*.{css,ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

const interdits = ["fonts.googleapis", "unpkg.com", "jsdelivr", "flagcdn"];

describe("tokens et ressources tierces", () => {
  it("la lecture brute des sources fonctionne (garde anti-test vide)", () => {
    expect(Object.keys(sources).length).toBeGreaterThan(5);
    expect(sources["/src/styles/tokens/fonts.css"]).toContain("@fontsource");
  });

  it("index.css contient le bloc @theme", () => {
    expect(sources["/src/styles/index.css"]).toContain("@theme");
  });

  it("aucun fichier ne charge de ressource tierce", () => {
    const fautifs = Object.entries(sources)
      .filter(([chemin]) => !chemin.endsWith("tokens.test.ts"))
      .filter(([, contenu]) => interdits.some((i) => contenu.includes(i)))
      .map(([chemin]) => chemin);
    expect(fautifs).toEqual([]);
  });

  it("index.html ne charge aucune ressource tierce", () => {
    const html = readFileSync("index.html", "utf8");
    expect(html).toContain("<div id=\"root\">");
    expect(interdits.filter((i) => html.includes(i))).toEqual([]);
  });
});
