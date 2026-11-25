import { createHmac } from "node:crypto";
import { readFileSync } from "node:fs";
import { request as httpsRequest } from "node:https";

import { expect, test, type BrowserContext, type Locator, type Page } from "@playwright/test";

import { expectNoCspViolation, watchCsp } from "./csp";

/**
 * Parcours complet sur une instance reelle (job CI e2e-golden-path) :
 * setup -> connexion + enrolement TOTP -> GitHub App simulee -> nouveau projet
 * -> equipe (invitation) -> deploiement reussi -> domaine via Pebble -> HTTPS 200 -> rollback.
 *
 * La machine est preparee par install/tests/golden-path/setup.sh, qui ecrit
 * les variables CIPHERA_E2E_* ; sans CIPHERA_E2E_URL, ce fichier est ignore
 * (playwright.config.ts) et le test est saute.
 */

const env = (name: string): string => {
  const value = process.env[name];
  if (!value) throw new Error(`${name} manquant : lancer install/tests/golden-path/setup.sh (voir README)`);
  return value;
};

const ADMIN = { first: "Ada", last: "Lovelace", email: "ada@ciphera-e2e.org", password: "golden-path-2026-test-secret" };
const REPOSITORY = "golden/app";
/** Proxy du noeud (lie a 127.0.0.1 par setup.sh). */
const PROXY = { host: "127.0.0.1", port: 443 };
const MINUTE = 60_000;

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

// --- TOTP (RFC 6238 : SHA-1, 6 chiffres, fenetres de 30 s) ---------------------

function base32(secret: string): Buffer {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  let bits = "";
  for (const c of secret.replace(/[\s=]/g, "").toUpperCase()) {
    const v = alphabet.indexOf(c);
    if (v < 0) throw new Error("cle TOTP illisible");
    bits += v.toString(2).padStart(5, "0");
  }
  return Buffer.from((bits.match(/.{8}/g) ?? []).map((b) => parseInt(b, 2)));
}

function totp(secret: string, step: number): string {
  const counter = Buffer.alloc(8);
  counter.writeBigUInt64BE(BigInt(step));
  const mac = createHmac("sha1", base32(secret)).update(counter).digest();
  const offset = mac[mac.length - 1]! & 0x0f;
  return String((mac.readUInt32BE(offset) & 0x7fffffff) % 1_000_000).padStart(6, "0");
}

/** Code d'une fenetre posterieure a `after` (un code ne sert qu'une fois), avec au moins 5 s de marge. */
async function freshCode(secret: string, after = -1): Promise<{ code: string; step: number }> {
  for (;;) {
    const now = Date.now();
    const step = Math.floor(now / 30_000);
    const left = 30_000 - (now % 30_000);
    if (step > after && left > 5_000) return { code: totp(secret, step), step };
    await sleep(left + 250);
  }
}

// --- client HTTPS du domaine (seules racines de confiance : celles de Pebble) ----

function fetchApp(host: string, ca: string): Promise<string> {
  return new Promise((resolve) => {
    const req = httpsRequest({ host: PROXY.host, port: PROXY.port, servername: host, path: "/", headers: { host }, ca, agent: false, timeout: 10_000 }, (res) => {
      let body = "";
      res.setEncoding("utf8");
      res.on("data", (chunk: string) => (body += chunk));
      res.on("end", () => resolve(`${res.statusCode} ${body.trim()}`));
    });
    req.on("timeout", () => req.destroy(new Error("délai dépassé")));
    req.on("error", (e) => resolve(`erreur ${e.message}`));
    req.end();
  });
}

async function expectServed(host: string, ca: string, version: string) {
  await expect.poll(() => fetchApp(host, ca), { timeout: 4 * MINUTE, intervals: [2_000] }).toBe(`200 ciphera golden ${version}`);
}

// --- GitHub simule ----------------------------------------------------------------

/** Le navigateur ne joint jamais github.com : ses pages viennent du GitHub simule. */
async function routeGithub(context: BrowserContext, fake: string) {
  await context.route(/^https:\/\/github\.com\//, async (route) => {
    const req = route.request();
    const url = new URL(req.url());
    const type = await req.headerValue("content-type");
    const body = req.postDataBuffer();
    const res = await fetch(fake + url.pathname + url.search, {
      method: req.method(),
      headers: type ? { "content-type": type } : {},
      body: body ? new Uint8Array(body) : undefined,
      redirect: "manual",
    });
    await route.fulfill({ status: res.status, contentType: res.headers.get("content-type") ?? "text/plain", body: Buffer.from(await res.arrayBuffer()) });
  });
}

// --- deploiements -------------------------------------------------------------------

const deploymentRow = (page: Page, n: number): Locator => page.locator("[data-deployment]").filter({ hasText: `#${n} · ` });

/** Attend l'issue du deploiement #n dans la liste (qui se rafraichit seule) et exige « Réussi ». */
async function expectDeploymentSucceeded(page: Page, n: number) {
  if (!new URL(page.url()).pathname.endsWith("/deployments")) await page.goto("/deployments");
  const row = deploymentRow(page, n);
  await expect(row).toBeVisible({ timeout: MINUTE });
  await expect.poll(async () => (await row.textContent())?.match(/Réussi|Échec|Annulé/)?.[0] ?? "en cours", { timeout: 15 * MINUTE, intervals: [3_000] }).not.toBe("en cours");
  await expect(row).toContainText("Réussi");
}

test("parcours complet : setup, TOTP, GitHub, déploiement, HTTPS, rollback", async ({ page, context }) => {
  test.skip(!process.env.CIPHERA_E2E_URL, "instance absente (CIPHERA_E2E_URL)");
  test.setTimeout(45 * MINUTE);
  const token = env("CIPHERA_E2E_SETUP_TOKEN");
  const fakeGithub = env("CIPHERA_E2E_GITHUB");
  const domain = env("CIPHERA_E2E_DOMAIN");
  const acmeRoots = readFileSync(env("CIPHERA_E2E_ACME_ROOTS"), "utf8");
  await routeGithub(context, fakeGithub);
  // La CSP du panneau (style-src avec nonce) ne doit etre violee a aucune etape.
  await watchCsp(page);
  const nav = page.getByRole("navigation", { name: "Navigation principale" });

  await test.step("assistant d'installation", async () => {
    await page.goto(`/setup?token=${encodeURIComponent(token)}`);
    await expect(page.getByText("Toutes les vérifications sont passées.")).toBeVisible({ timeout: MINUTE });
    await page.getByRole("button", { name: "Continuer" }).click();
    await page.getByLabel("Prénom").fill(ADMIN.first);
    await page.getByLabel("Nom", { exact: true }).fill(ADMIN.last);
    await page.getByLabel("E-mail").fill(ADMIN.email);
    await page.getByLabel("Mot de passe", { exact: true }).fill(ADMIN.password);
    await page.getByLabel("Confirmer le mot de passe").fill(ADMIN.password);
    await page.getByRole("button", { name: "Continuer" }).click();
    await page.getByLabel("Nom de l'organisation").fill("Golden");
    await page.getByRole("button", { name: "Continuer" }).click();
    await page.getByRole("button", { name: "Installer" }).click();
    await expect(nav).toBeVisible({ timeout: MINUTE });
  });

  let totpSecret = "";
  let lastStep = -1;
  await test.step("enrôlement TOTP", async () => {
    await page.goto("/profile");
    await page.getByRole("navigation", { name: "Profil" }).getByRole("button", { name: "Sécurité" }).click();
    const toggle = page.getByRole("switch", { name: "Application d'authentification" });
    await toggle.click();
    const dialog = page.getByRole("dialog", { name: "Activer la vérification en deux étapes" });
    await dialog.getByLabel("Mot de passe").fill(ADMIN.password);
    await dialog.getByRole("button", { name: "Continuer" }).click();
    const key = dialog.locator("code");
    await expect(key).toHaveText(/^[A-Z2-7 ]{16,}$/);
    totpSecret = (await key.textContent())!.replace(/\s/g, "");
    const { code, step } = await freshCode(totpSecret);
    lastStep = step;
    await dialog.getByLabel("Code à 6 chiffres").fill(code);
    await dialog.getByRole("button", { name: "Vérifier" }).click();
    await page.getByRole("dialog", { name: "Codes de secours" }).getByRole("button", { name: "J'ai enregistré mes codes" }).click();
    await expect(toggle).toBeChecked();
  });

  await test.step("déconnexion puis connexion avec le code TOTP", async () => {
    await page.getByRole("button", { name: new RegExp(`${ADMIN.first} ${ADMIN.last}`) }).click();
    await page.getByRole("menuitem", { name: "Se déconnecter" }).click();
    await expect(page).toHaveURL(/\/login/);
    await page.getByLabel("E-mail").fill(ADMIN.email);
    await page.getByLabel("Mot de passe", { exact: true }).fill(ADMIN.password);
    await page.getByRole("button", { name: "Se connecter" }).click();
    await expect(page.getByLabel("Chiffre 1")).toBeVisible();
    const { code } = await freshCode(totpSecret, lastStep);
    for (const [i, digit] of [...code].entries()) await page.getByLabel(`Chiffre ${i + 1}`).fill(digit);
    await expect(nav).toBeVisible({ timeout: MINUTE });
  });

  await test.step("équipe : inviter un lecteur, voir l'invitation en attente, l'annuler", async () => {
    await page.goto("/settings/team");
    await page.getByRole("button", { name: "Inviter" }).click();
    const dialog = page.getByRole("dialog", { name: "Inviter un membre" });
    await dialog.getByLabel("E-mail").fill("lecteur@ciphera-e2e.org");
    await dialog.getByRole("radio", { name: "Lecteur" }).click();
    await dialog.getByRole("button", { name: "Envoyer l'invitation" }).click();
    const link = page.getByRole("dialog", { name: "Invitation créée" });
    await expect(link.getByLabel("Lien", { exact: true })).toContainText("/invite/");
    await link.getByRole("button", { name: "Terminé" }).click();
    await expect(page.getByText("lecteur@ciphera-e2e.org")).toBeVisible();
    await page.getByRole("button", { name: "Annuler l'invitation de lecteur@ciphera-e2e.org" }).click();
    await expect(page.getByText("lecteur@ciphera-e2e.org")).toBeHidden();
  });

  await test.step("GitHub App simulée : création puis installation", async () => {
    await page.goto("/connectors");
    await page.getByRole("button", { name: "Ajouter un connecteur" }).click();
    await page.getByRole("dialog", { name: "Connecter GitHub" }).getByRole("button", { name: "Autoriser sur GitHub" }).click();
    await expect(page).toHaveURL(/^https:\/\/github\.com\/settings\/apps\/new\?state=/);
    await page.getByRole("link", { name: "Create GitHub App for golden" }).click();
    await expect(page.getByText("GitHub connecté : installez l'App sur vos dépôts")).toBeVisible({ timeout: MINUTE });

    const popup = page.waitForEvent("popup");
    await page.getByRole("link", { name: "Installer", exact: true }).click();
    const install = await popup;
    await expect(install).toHaveURL(/^https:\/\/github\.com\/apps\/[a-z0-9-]+\/installations\/new$/);
    await install.getByRole("link", { name: "Install", exact: true }).click();
    await expect(install.getByText("GitHub App installée")).toBeVisible({ timeout: MINUTE });
    await install.close();
  });

  await test.step("nouveau projet depuis le dépôt et premier déploiement", async () => {
    await nav.getByRole("link", { name: "Projets" }).click();
    await page.getByRole("button", { name: "Importer un dépôt" }).click();
    const dialog = page.getByRole("dialog", { name: "Nouveau projet" });
    await dialog.getByRole("button", { name: `Importer ${REPOSITORY}` }).click({ timeout: MINUTE });
    await dialog.getByRole("button", { name: /^Dockerfile/ }).click();
    await expect(dialog.getByLabel("Nom du projet")).toHaveValue("app");
    await dialog.getByRole("button", { name: "Créer le projet" }).click();
    await expect(dialog.getByText(/^(Projet en ligne|Échec du déploiement|Déploiement annulé)$/)).toBeVisible({ timeout: 15 * MINUTE });
    await expect(dialog.getByText("Projet en ligne")).toBeVisible();
    await dialog.getByRole("button", { name: "Ouvrir le projet" }).click();
    await expect(page.getByRole("heading", { name: "app", exact: true })).toBeVisible();
  });

  await test.step("domaine : DNS vérifié, certificat émis par Pebble", async () => {
    await page.getByRole("navigation", { name: "Onglets du projet" }).getByRole("link", { name: "Domaines" }).click();
    await page.getByRole("button", { name: "Ajouter un domaine" }).click();
    const dialog = page.getByRole("dialog", { name: "Ajouter un domaine" });
    await dialog.getByLabel("Domaine", { exact: true }).fill(domain);
    await dialog.getByLabel("Port interne").fill("8080");
    await dialog.getByRole("button", { name: "Continuer" }).click();
    await dialog.getByRole("button", { name: "Vérifier le DNS" }).click();
    await expect(dialog.getByText(/^DNS propagé/)).toBeVisible({ timeout: MINUTE });
    await dialog.getByRole("button", { name: "Ajouter le domaine" }).click();
    await expect(dialog).toBeHidden();
    const row = page.getByTestId(`domain-${domain}`);
    await expect(async () => {
      await page.reload();
      await expect(row).toContainText("HTTPS actif", { timeout: 5_000 });
    }).toPass({ timeout: 5 * MINUTE, intervals: [5_000] });
  });

  // Le port interne s'applique au prochain deploiement (decisions.md, Task 30) :
  // le depot avance sur v2, puis « Redéployer » sert le domaine en HTTPS.
  await test.step("nouveau commit, redéploiement et HTTPS 200", async () => {
    const advanced = await fetch(`${fakeGithub}/_e2e/advance`, { method: "POST" });
    expect(advanced.ok).toBe(true);
    await page.getByRole("button", { name: "Redéployer", exact: true }).click();
    await expect(page.getByText("Déploiement #2 lancé")).toBeVisible();
    await expectDeploymentSucceeded(page, 2);
    await expectServed(domain, acmeRoots, "v2");
  });

  await test.step("rollback vers le premier déploiement", async () => {
    await deploymentRow(page, 1).getByRole("button", { name: "Actions de #1" }).click();
    await page.getByRole("menuitem", { name: "Revenir à cette version" }).click();
    await page.getByRole("dialog", { name: "Revenir à #1 ?" }).getByRole("button", { name: "Rollback" }).click();
    await expect(page.getByText("Rollback vers #1 lancé (#3)")).toBeVisible();
    await expectDeploymentSucceeded(page, 3);
    await expectServed(domain, acmeRoots, "v1");
  });

  expectNoCspViolation(page);
});
