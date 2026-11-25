import { createMemoryHistory } from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse, type JsonBodyType } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";

import { AppProviders, createAppQueryClient } from "@/app/providers";
import { createAppRouter } from "@/app/router";
import { clearHeldTokens, heldToken } from "@/lib/url-token";

const PASSWORD = "correct horse battery";
const CHECKS = [
  { id: "database", ok: true, warning: false, detail: "Base de données joignable, migrations appliquées." },
  { id: "tls", ok: true, warning: false, detail: "Certificat HTTPS du panneau présent." },
  { id: "data_dir", ok: true, warning: false, detail: "Dossier de données accessible en écriture, 118 Go libres." },
  { id: "docker", ok: true, warning: true, detail: "Docker introuvable (/var/run/docker.sock)." },
];

interface Seen {
  tokens: (string | null)[];
  completeBody: unknown;
}
let seen: Seen;
let initialized = false;
let checks = CHECKS;
let completeStatus = 201;

const json = (body: JsonBodyType, status = 200) => HttpResponse.json(body, { status });

const server = setupServer(
  http.get("*/api/setup/status", () => json({ initialized })),
  http.post("*/api/setup/checks", ({ request }) => {
    seen.tokens.push(request.headers.get("x-setup-token"));
    return json({ checks });
  }),
  http.post("*/api/setup/complete", async ({ request }) => {
    seen.tokens.push(request.headers.get("x-setup-token"));
    seen.completeBody = await request.json();
    if (completeStatus !== 201) return json({ error: { code: "x", message: "x" } }, completeStatus);
    return json({ user: { id: "1", name: "Alice Martin", email: "alice@example.com", role: "owner" } }, 201);
  }),
);

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  seen = { tokens: [], completeBody: null };
  initialized = false;
  checks = CHECKS;
  completeStatus = 201;
  clearHeldTokens();
  window.history.replaceState(null, "", "/");
});
afterEach(() => {
  server.resetHandlers();
  vi.restoreAllMocks();
});

function mount(url = "/setup?token=tok-123") {
  window.history.replaceState(null, "", url);
  const router = createAppRouter(createMemoryHistory({ initialEntries: [url] }));
  const client = createAppQueryClient(router);
  render(<AppProviders router={router} client={client} />);
  return { router };
}

const type = (label: string, value: string) => fireEvent.change(screen.getByLabelText(label), { target: { value } });
const next = (name = "Continuer") => fireEvent.click(screen.getByRole("button", { name }));

async function reachAdmin() {
  await screen.findByText("Toutes les vérifications sont passées.");
  next();
  await screen.findByRole("heading", { name: "Compte administrateur" });
}

function fillAdmin(password = PASSWORD) {
  type("Prénom", "Alice");
  type("Nom", "Martin");
  type("E-mail", "alice@example.com");
  type("Mot de passe", password);
  type("Confirmer le mot de passe", password);
}

it("la soumission appelle /api/setup/complete avec le jeton puis redirige vers /", async () => {
  const { router } = mount();
  await reachAdmin();
  fillAdmin();
  next();
  await screen.findByRole("heading", { name: "Votre organisation" });
  type("Nom de l'organisation", "Acme Cloud");
  next();
  await screen.findByRole("heading", { name: "Accès au panel" });
  next("Installer");

  await waitFor(() => expect(router.state.location.pathname).toBe("/"));
  expect(seen.completeBody).toEqual({
    admin: { name: "Alice Martin", email: "alice@example.com", password: PASSWORD },
    organization: { name: "Acme Cloud", slug: "acme-cloud", timezone: "Europe/Paris", language: "fr" },
  });
  // Le jeton accompagne les verifications et la creation.
  expect(seen.tokens).toEqual(["tok-123", "tok-123"]);
  // Consommé : il ne reste pas en mémoire.
  expect(heldToken("setup")).toBeUndefined();
});

it("le jeton est retire de la barre d'adresse et n'est stocke nulle part", async () => {
  const spy = vi.spyOn(window.history, "replaceState");
  mount();
  await screen.findByText("Toutes les vérifications sont passées.");
  expect(window.location.search).toBe("");
  expect(spy.mock.calls.some(([, , url]) => String(url) === "/setup")).toBe(true);
  for (const store of [localStorage, sessionStorage]) {
    expect(JSON.stringify({ ...store })).not.toContain("tok-123");
  }
});

it("setup_fragment_read : le jeton du fragment #token= est lu puis retiré de l'adresse", async () => {
  const spy = vi.spyOn(window.history, "replaceState");
  const { router } = mount("/setup#token=tok-frag");
  await screen.findByText("Toutes les vérifications sont passées.");
  expect(seen.tokens).toEqual(["tok-frag"]);
  expect(window.location.hash).toBe("");
  expect(window.location.href).not.toContain("tok-frag");
  expect(router.state.location.hash).toBe("");
  expect(router.state.location.href).not.toContain("tok-frag");
  expect(spy.mock.calls.some(([, , url]) => String(url) === "/setup")).toBe(true);
  for (const store of [localStorage, sessionStorage]) {
    expect(JSON.stringify({ ...store })).not.toContain("tok-frag");
  }
});

it("l'ancien ?token= reste accepté puis est retiré de l'adresse", async () => {
  const { router } = mount("/setup?token=tok-old");
  await screen.findByText("Toutes les vérifications sont passées.");
  expect(seen.tokens).toEqual(["tok-old"]);
  expect(router.state.location.href).not.toContain("tok-old");
  expect(window.location.href).not.toContain("tok-old");
});

it("un jeton expiré (24 h) explique comment en obtenir un nouveau", async () => {
  server.use(http.post("*/api/setup/checks", () => json({ error: { code: "setup_token_expired", message: "x" } }, 401)));
  mount();
  expect(await screen.findByText(/Lien d'installation expiré/)).toBeInTheDocument();
  expect(screen.getByText(/ciphera admin setup-token/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Continuer" })).toBeDisabled();
});

it("un assistant deja termine redirige vers /login", async () => {
  initialized = true;
  const { router } = mount();
  await waitFor(() => expect(router.state.location.pathname).toBe("/login"));
  expect(seen.tokens).toEqual([]);
});

it("un mot de passe de moins de 12 caracteres bloque l'etape administrateur", async () => {
  mount();
  await reachAdmin();
  fillAdmin("onzecaracte");
  expect(screen.getByRole("button", { name: "Continuer" })).toBeDisabled();
  type("Mot de passe", PASSWORD);
  expect(screen.getByRole("button", { name: "Continuer" })).toBeDisabled();
  expect(screen.getByText("Les mots de passe ne correspondent pas.")).toBeInTheDocument();
  type("Confirmer le mot de passe", PASSWORD);
  expect(screen.getByRole("button", { name: "Continuer" })).toBeEnabled();
});

it("une verification en echec bloque la suite ; un avertissement ne bloque pas", async () => {
  checks = [{ ...CHECKS[0]!, ok: false, detail: "Base de données injoignable." }, ...CHECKS.slice(1)];
  mount();
  expect(await screen.findByText("Base de données injoignable.")).toBeInTheDocument();
  expect(screen.getByText("Échec")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Continuer" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Relancer" })).toBeInTheDocument();
});

it("un avertissement (Docker absent) laisse continuer", async () => {
  mount();
  expect(await screen.findByText("Attention")).toBeInTheDocument();
  await waitFor(() => expect(screen.getByRole("button", { name: "Continuer" })).toBeEnabled());
});

it("un jeton refuse (401) s'affiche sans rediriger vers la connexion", async () => {
  server.use(http.post("*/api/setup/checks", () => json({ error: { code: "invalid_setup_token", message: "x" } }, 401)));
  const { router } = mount();
  expect(await screen.findByText(/Jeton d'installation invalide/)).toBeInTheDocument();
  expect(router.state.location.pathname).toBe("/setup");
  expect(screen.getByRole("button", { name: "Continuer" })).toBeDisabled();
});

it("sans jeton dans l'adresse, l'assistant l'explique et reste bloque", async () => {
  mount("/setup");
  expect(await screen.findByText(/Lien d'installation incomplet/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Continuer" })).toBeDisabled();
  expect(seen.tokens).toEqual([]);
});

it("l'etape Acces ne propose que « Domaine plus tard » (Tailscale masque)", async () => {
  mount();
  await reachAdmin();
  fillAdmin();
  next();
  await screen.findByRole("heading", { name: "Votre organisation" });
  type("Nom de l'organisation", "Acme");
  next();
  await screen.findByRole("heading", { name: "Accès au panel" });
  expect(screen.getAllByRole("button", { name: /Domaine plus tard|Tailscale|VPN/i })).toHaveLength(1);
  expect(screen.queryByText(/tailscale/i)).not.toBeInTheDocument();
});

it("un refus 422 a la creation affiche l'erreur et permet de revenir", async () => {
  completeStatus = 422;
  const { router } = mount();
  await reachAdmin();
  fillAdmin();
  next();
  await screen.findByRole("heading", { name: "Votre organisation" });
  type("Nom de l'organisation", "Acme");
  next();
  await screen.findByRole("heading", { name: "Accès au panel" });
  next("Installer");
  expect(await screen.findByText(/Certains champs sont invalides/)).toBeInTheDocument();
  expect(router.state.location.pathname).toBe("/setup");
  fireEvent.click(screen.getByRole("button", { name: "Retour" }));
  expect(await screen.findByRole("heading", { name: "Accès au panel" })).toBeInTheDocument();
});

it("un 410 a la creation (assistant deja termine) redirige vers /login", async () => {
  completeStatus = 410;
  const { router } = mount();
  await reachAdmin();
  fillAdmin();
  next();
  await screen.findByRole("heading", { name: "Votre organisation" });
  type("Nom de l'organisation", "Acme");
  next();
  await screen.findByRole("heading", { name: "Accès au panel" });
  next("Installer");
  await waitFor(() => expect(router.state.location.pathname).toBe("/login"));
});
