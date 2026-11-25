import { expect, test } from "@playwright/test";

import { expectNoCspViolation, watchCsp } from "./csp";

// Limite : `vite preview` ne pose aucune CSP ; ce controle ne voit rien ici, la couverture reelle vient du golden-path.
test.beforeEach(async ({ page }) => {
  await watchCsp(page);
});

test.afterEach(({ page }) => {
  expectNoCspViolation(page);
});

const nav = (page: import("@playwright/test").Page) => page.getByRole("navigation", { name: "Navigation principale" });

test("le shell affiche la sidebar FR et l'entree active suit l'URL", async ({ page }) => {
  await page.goto("/");
  for (const label of ["Pilotage", "Ressources", "Plateforme", "Vue d'ensemble", "Projets", "Déploiements", "Serveurs", "Domaines", "Paramètres"]) {
    await expect(nav(page).getByText(label, { exact: true })).toBeVisible();
  }
  await expect(nav(page).getByRole("link", { name: /Vue d'ensemble/ })).toHaveAttribute("aria-current", "page");

  await nav(page).getByRole("link", { name: "Projets" }).click();
  await expect(page).toHaveURL(/\/projects$/);
  await expect(nav(page).getByRole("link", { name: "Projets" })).toHaveAttribute("aria-current", "page");
  await expect(page.getByRole("heading", { name: "Projets" })).toBeVisible();
});

test("un lien profond recharge la bonne page", async ({ page }) => {
  await page.goto("/deployments");
  await expect(page.getByRole("heading", { name: "Déploiements" })).toBeVisible();
  await expect(nav(page).getByRole("link", { name: /Déploiements/ })).toHaveAttribute("aria-current", "page");
});

test("Ctrl+B replie et deplie la sidebar", async ({ page }) => {
  await page.goto("/");
  await expect(nav(page).getByText("Projets", { exact: true })).toBeVisible();
  await page.keyboard.press("Control+b");
  await expect(nav(page).getByText("Projets", { exact: true })).toHaveCount(0);
  await page.keyboard.press("Control+b");
  await expect(nav(page).getByText("Projets", { exact: true })).toBeVisible();
});

test("une entree v0.2 est desactivee avec « Bientôt » et son infobulle", async ({ page }) => {
  await page.goto("/");
  const monitoring = nav(page).getByRole("link", { name: /Monitoring/ });
  await expect(monitoring).toHaveAttribute("aria-disabled", "true");
  await expect(monitoring).toContainText("Bientôt");
  await monitoring.hover();
  await expect(page.getByRole("tooltip").getByText("Disponible en v0.2")).toBeVisible();
  await monitoring.click({ force: true });
  await expect(page).toHaveURL(/\/$/);
});

test("une URL inconnue affiche le 404", async ({ page }) => {
  await page.goto("/cette/page/nexiste/pas");
  await expect(page.getByRole("heading", { name: "Page introuvable" })).toBeVisible();
  await expect(page.getByText("/cette/page/nexiste/pas")).toBeVisible();
  await page.getByRole("link", { name: /Retour au dashboard/ }).click();
  await expect(page).toHaveURL(/\/$/);
});

test("/login?expired=1 affiche la session expiree", async ({ page }) => {
  await page.goto("/login?expired=1");
  await expect(page.getByRole("heading", { name: "Session expirée" })).toBeVisible();
});

test("aucune ressource tierce n'est chargee", async ({ page }) => {
  const external: string[] = [];
  page.on("request", (req) => {
    const url = new URL(req.url());
    if (!["localhost", "127.0.0.1"].includes(url.hostname) && url.protocol.startsWith("http")) external.push(req.url());
  });
  await page.goto("/");
  await expect(nav(page)).toBeVisible();
  await page.waitForLoadState("networkidle");
  expect(external).toEqual([]);
});

test("le menu du compte ouvre le profil et grise les entrees a venir", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Compte" }).click();
  await expect(page.getByRole("menuitem", { name: /Équipe/ })).toHaveAttribute("aria-disabled", "true");
  await page.getByRole("menuitem", { name: "Mon profil" }).click();
  await expect(page).toHaveURL(/\/profile$/);
});
