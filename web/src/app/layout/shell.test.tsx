import { createMemoryHistory } from "@tanstack/react-router";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it } from "vitest";

import { fatalError } from "@/app/errors/fatal-error-store";
import { AppProviders, createAppQueryClient } from "@/app/providers";
import { createAppRouter } from "@/app/router";
import { getCsrfToken, setCsrfToken } from "@/lib/api/csrf";
import { ApiError } from "@/lib/api/errors";
import { emptyInstanceOverview } from "@/mocks/fixtures/dashboard";

import { NAV } from "./nav";

function mount(url: string) {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [url] }));
  const client = createAppQueryClient(router);
  render(<AppProviders router={router} client={client} />);
  return { router, client };
}

const navigation = () => screen.findByRole("navigation", { name: "Navigation principale" });

const ME = { id: "u1", name: "Alice Martin", email: "alice@example.com", role: "owner", csrf_token: "csrf-test-alice" };
let meStatus = 200;
let logoutCsrf: (string | null)[] = [];

const server = setupServer(
  // La page Projets portee charge la liste : sans handler la requete echouerait
  // (frame non gere) et declencherait l'ecran d'erreur pendant les tests.
  http.get("*/api/projects", () => HttpResponse.json([])),
  // L'ecran Deploiements charge la liste (navigation vers /deployments).
  http.get("*/api/deployments", () => HttpResponse.json([])),
  // La Vue d'ensemble (/) charge ses donnees.
  http.get("*/api/overview", () => HttpResponse.json(emptyInstanceOverview())),
  http.get("*/api/me", () => (meStatus === 200 ? HttpResponse.json(ME) : HttpResponse.json({ error: { code: "unauthenticated", message: "x" } }, { status: meStatus }))),
  http.post("*/api/auth/logout", ({ request }) => {
    logoutCsrf.push(request.headers.get("x-csrf-token"));
    return new HttpResponse(null, { status: 204 });
  }),
);

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  fatalError.clear();
  meStatus = 200;
  logoutCsrf = [];
  setCsrfToken(null);
});
afterEach(() => {
  fatalError.clear();
  server.resetHandlers();
});

it("la sidebar affiche les sections et libelles FR de la maquette", async () => {
  mount("/");
  const nav = await navigation();
  for (const section of ["Pilotage", "Ressources", "Plateforme"]) expect(within(nav).getByText(section)).toBeInTheDocument();
  for (const group of NAV) for (const item of group.items) expect(within(nav).getByText(item.label)).toBeInTheDocument();
  expect(within(nav).getByText("Bases de données")).toBeInTheDocument();
});

it("l'entree active correspond a l'URL", async () => {
  mount("/projects");
  const nav = await navigation();
  const active = within(nav)
    .getAllByRole("link")
    .filter((a) => a.getAttribute("aria-current") === "page");
  expect(active).toHaveLength(1);
  expect(active[0]).toHaveTextContent("Projets");
  expect(await screen.findByRole("heading", { name: "Projets" })).toBeInTheDocument();
});

it("cliquer une entree navigue sans recharger", async () => {
  const { router } = mount("/");
  const nav = await navigation();
  fireEvent.click(within(nav).getByText("Déploiements"));
  await waitFor(() => expect(router.state.location.pathname).toBe("/deployments"));
  expect(within(nav).getByText("Déploiements").closest("a")).toHaveAttribute("aria-current", "page");
});

it("les entrees v0.2 et v0.3 sont visibles, desactivees, avec « Bientôt » et une infobulle", async () => {
  const { router } = mount("/");
  const nav = await navigation();
  const monitoring = within(nav).getByText("Monitoring").closest("a")!;
  expect(monitoring).toHaveAttribute("aria-disabled", "true");
  expect(monitoring).not.toHaveAttribute("href");
  expect(within(monitoring).getByText("Bientôt")).toBeInTheDocument();
  fireEvent.click(monitoring);
  expect(router.state.location.pathname).toBe("/");
  act(() => monitoring.focus());
  expect((await screen.findAllByText("Disponible en v0.2")).length).toBeGreaterThan(0);
  const tickets = within(nav).getByText("Tickets").closest("a")!;
  act(() => tickets.focus());
  expect((await screen.findAllByText("Disponible en v0.3")).length).toBeGreaterThan(0);
});

it("⌘B puis Ctrl+B replient et deplient la sidebar", async () => {
  mount("/");
  const nav = await navigation();
  expect(within(nav).getByText("Projets")).toBeInTheDocument();
  fireEvent.keyDown(window, { key: "b", metaKey: true });
  expect(within(nav).queryByText("Projets")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Déplier le menu" })).toBeInTheDocument();
  fireEvent.keyDown(window, { key: "b", ctrlKey: true });
  expect(within(nav).getByText("Projets")).toBeInTheDocument();
});

it("⌘B est ignore dans un champ de saisie", async () => {
  mount("/");
  const nav = await navigation();
  const field = document.createElement("input");
  document.body.append(field);
  fireEvent.keyDown(field, { key: "b", metaKey: true });
  expect(within(nav).getByText("Projets")).toBeInTheDocument();
  field.remove();
});

it("une route inconnue affiche le 404 porte de la maquette", async () => {
  mount("/inconnu/page");
  expect(await screen.findByRole("heading", { name: "Page introuvable" })).toBeInTheDocument();
  expect(screen.getByText("/inconnu/page")).toBeInTheDocument();
  expect(screen.getByText("Retour au dashboard")).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Aller à une page"), { target: { value: "zzz" } });
  expect(screen.getByText("Aucune page ne correspond.")).toBeInTheDocument();
});

it("une erreur 401 mene a /login?expired=1 et affiche la session expiree", async () => {
  const { router, client } = mount("/projects");
  await screen.findByRole("heading", { name: "Projets" });
  await client.fetchQuery({ queryKey: ["x"], retry: false, queryFn: () => Promise.reject(new ApiError(401)) }).catch(() => undefined);
  await waitFor(() => expect(router.state.location.href).toBe("/login?expired=1"));
  expect(await screen.findByRole("heading", { name: "Session expirée" })).toBeInTheDocument();
});

it("401 vide le cache de requetes avant la redirection", async () => {
  const { router, client } = mount("/projects");
  await screen.findByRole("heading", { name: "Projets" });
  client.setQueryData(["secret"], { donnees: "autre utilisateur" });
  await client.fetchQuery({ queryKey: ["x"], retry: false, queryFn: () => Promise.reject(new ApiError(401, "/api/projects")) }).catch(() => undefined);
  await waitFor(() => expect(router.state.location.pathname).toBe("/login"));
  expect(client.getQueryData(["secret"])).toBeUndefined();
});

it("401 sur /api/auth/* ne redirige pas (mot de passe errone)", async () => {
  const { router, client } = mount("/projects");
  await screen.findByRole("heading", { name: "Projets" });
  await client.fetchQuery({ queryKey: ["l"], retry: false, queryFn: () => Promise.reject(new ApiError(401, "/api/auth/login")) }).catch(() => undefined);
  expect(router.state.location.pathname).toBe("/projects");
});

it("un 401 deja sur /login ne relance pas de redirection ni ne vide le cache", async () => {
  const { router, client } = mount("/login");
  await screen.findByRole("heading", { name: "Connectez-vous à Ciphera" });
  client.setQueryData(["keep"], 1);
  await client.fetchQuery({ queryKey: ["x"], retry: false, queryFn: () => Promise.reject(new ApiError(401, "/api/me")) }).catch(() => undefined);
  expect(router.state.location.href).toBe("/login");
  expect(client.getQueryData(["keep"])).toBe(1);
});

it("une mutation en erreur 500 affiche un toast et garde la page montee", async () => {
  const { client } = mount("/projects");
  await screen.findByRole("heading", { name: "Projets" });
  await client
    .getMutationCache()
    .build(client, { mutationFn: () => Promise.reject(new ApiError(500, "/api/projects")) })
    .execute(undefined)
    .catch(() => undefined);
  expect(await screen.findByText("Erreur du serveur : l'action a échoué.")).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Projets" })).toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: "Erreur du serveur" })).not.toBeInTheDocument();
});

it("les entrees Serveurs, Domaines et Clés API sont desactivees (v0.2)", async () => {
  mount("/");
  const nav = await navigation();
  for (const label of ["Serveurs", "Domaines"]) {
    const link = within(nav).getByText(label).closest("a")!;
    expect(link).toHaveAttribute("aria-disabled", "true");
    expect(within(link).getByText("Bientôt")).toBeInTheDocument();
  }
});

it.each([
  ["403", new ApiError(403), "Accès refusé"],
  ["reseau", new TypeError("Failed to fetch"), "API injoignable"],
  ["500", new ApiError(500), "Erreur du serveur"],
])("erreur %s : l'ecran d'erreur remplace la page", async (_name, error, title) => {
  const { client } = mount("/projects");
  await screen.findByRole("heading", { name: "Projets" });
  await client.fetchQuery({ queryKey: ["y"], retry: false, queryFn: () => Promise.reject(error) }).catch(() => undefined);
  expect(await screen.findByRole("heading", { name: title })).toBeInTheDocument();
});

it("l'ecran d'erreur disparait au changement de page", async () => {
  const { router } = mount("/projects");
  await screen.findByRole("heading", { name: "Projets" });
  act(() => fatalError.set("forbidden"));
  expect(await screen.findByRole("heading", { name: "Accès refusé" })).toBeInTheDocument();
  await act(() => router.navigate({ to: "/deployments" }));
  expect(await screen.findByRole("heading", { name: "Déploiements" })).toBeInTheDocument();
});

it("le menu du compte affiche l'utilisateur de GET /api/me", async () => {
  mount("/");
  await navigation();
  expect(await screen.findByText("Alice Martin")).toBeInTheDocument();
  expect(screen.getByText("alice@example.com")).toBeInTheDocument();
  expect(getCsrfToken()).toBe("csrf-test-alice");
});

it("sans session (401 sur /api/me), le shell redirige vers /login sans bandeau d'expiration", async () => {
  meStatus = 401;
  const { router } = mount("/projects");
  await waitFor(() => expect(router.state.location.href).toBe("/login"));
  expect(await screen.findByRole("heading", { name: "Connectez-vous à Ciphera" })).toBeInTheDocument();
  expect(screen.queryByText("Session expirée")).not.toBeInTheDocument();
  expect(screen.queryByRole("navigation", { name: "Navigation principale" })).not.toBeInTheDocument();
});

it("session en attente de double authentification (401 mfa_required) : le shell redirige vers /2fa", async () => {
  server.use(http.get("*/api/me", () => HttpResponse.json({ error: { code: "mfa_required", message: "x" } }, { status: 401 })));
  const { router } = mount("/projects");
  await waitFor(() => expect(router.state.location.pathname).toBe("/2fa"));
  expect(await screen.findByRole("heading", { name: "Vérification en deux étapes" })).toBeInTheDocument();
  expect(screen.queryByRole("navigation", { name: "Navigation principale" })).not.toBeInTheDocument();
});

it("« Se déconnecter » appelle la deconnexion avec le jeton CSRF puis mene a /login", async () => {
  const { router, client } = mount("/projects");
  await screen.findByText("Alice Martin");
  fireEvent.keyDown(screen.getByRole("button", { name: /Alice Martin/ }), { key: "ArrowDown" });
  fireEvent.click(await screen.findByRole("menuitem", { name: /Se déconnecter/ }));
  await waitFor(() => expect(router.state.location.pathname).toBe("/login"));
  expect(logoutCsrf).toEqual(["csrf-test-alice"]);
  expect(getCsrfToken()).toBeNull();
  expect(client.getQueryData(["me"])).toBeUndefined();
});

it("GET /api/me en echec (4xx) : un observateur de la page n'entraine pas de boucle demontage / rechargement", async () => {
  // Erreur non fatale (ni 401 ni 5xx) : le shell reste monte avec un compte
  // inconnu ; la page Profil observe aussi `me` et relance la requete en se
  // montant. Le shell ne doit pas se demonter pendant ce rechargement.
  let calls = 0;
  server.use(
    http.get("*/api/me", () => {
      calls += 1;
      return HttpResponse.json({ error: { code: "rate_limited", message: "x" } }, { status: 429 });
    }),
  );
  mount("/profile");
  await new Promise((r) => setTimeout(r, 1000));
  expect(calls).toBeLessThanOrEqual(2);
  expect(screen.getByRole("navigation", { name: "Profil" })).toBeInTheDocument();
  expect(screen.getByRole("navigation", { name: "Navigation principale" })).toBeInTheDocument();
});
