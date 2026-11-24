import { createMemoryHistory } from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it } from "vitest";

import { AppProviders, createAppQueryClient } from "@/app/providers";
import { createAppRouter } from "@/app/router";
import { setCsrfToken } from "@/lib/api/csrf";
import { WEB_SERVICES } from "@/mocks/fixtures/overview";
import { PROJECT_SUMMARIES, projectDetail } from "@/mocks/fixtures/projects";

const WEB = projectDetail(PROJECT_SUMMARIES[0]);
let role = "owner";
let refusal: { status: number; message: string } | null = null;
const posts: { path: string; csrf: string | null }[] = [];
const patches: { path: string; csrf: string | null; body: unknown }[] = [];

const server = setupServer(
  http.get("*/api/me", () => HttpResponse.json({ id: "u1", name: "Alice Martin", email: "alice@example.com", role, csrf_token: "csrf-1" })),
  http.get("*/api/deployments", () => HttpResponse.json([])),
  http.get("*/api/projects/:id", () => HttpResponse.json(WEB)),
  http.get("*/api/projects/:id/services", () => HttpResponse.json(WEB_SERVICES)),
  http.patch("*/api/projects/:id/services/:sid", async ({ request }) => {
    const body = await request.json();
    patches.push({ path: new URL(request.url).pathname, csrf: request.headers.get("x-csrf-token"), body });
    return HttpResponse.json({ ...WEB_SERVICES[0], ...(body as object), runs_as_root: false });
  }),
  http.post("*/api/projects/:id/services/:sid/:action", ({ request }) => {
    posts.push({ path: new URL(request.url).pathname, csrf: request.headers.get("x-csrf-token") });
    return refusal ? HttpResponse.json({ error: { code: "nothing_serving", message: refusal.message } }, { status: refusal.status }) : HttpResponse.json({ deployment_id: "d1" }, { status: 202 });
  }),
);

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  setCsrfToken("csrf-1");
  role = "owner";
  refusal = null;
  posts.length = 0;
  patches.length = 0;
});
afterEach(() => server.resetHandlers());

function mount() {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [`/projects/${WEB.id}/services`] }));
  render(<AppProviders router={router} client={createAppQueryClient(router)} />);
  return router;
}

const card = (name: string) => screen.getByRole("article", { name });

it("cartes rendues à partir des fixtures : état, image, réseau privé, CPU et mémoire", async () => {
  mount();
  await screen.findByRole("article", { name: "web" }, { timeout: 5000 });
  expect(within(card("web")).getByText("En ligne")).toBeInTheDocument();
  expect(within(card("web")).getByText("Railpack")).toBeInTheDocument();
  expect(within(card("web")).getByText("web.internal:3000")).toBeInTheDocument();
  expect(within(card("web")).getByText("34 %")).toBeInTheDocument();
  expect(within(card("web")).getByText("256 Mo")).toBeInTheDocument();
  expect(within(card("web")).getByLabelText("Réplicas 1 / 1")).toBeInTheDocument();
  expect(within(card("worker")).getByText("Arrêté")).toBeInTheDocument();
  expect(within(card("worker")).getByText("worker.internal")).toBeInTheDocument();
  expect(within(card("worker")).getAllByText("—")).toHaveLength(2);
  expect(within(card("postgres")).getByText("Échec du build")).toBeInTheDocument();
  // Onglet Services actif, avec le nombre de services du projet.
  const nav = screen.getByRole("navigation", { name: "Onglets du projet" });
  expect(within(nav).getByRole("link", { name: /Services/ })).toHaveAttribute("aria-current", "page");
});

it("v0.2 : terminal, ressources, réplicas et ajout de service portés mais inactifs", async () => {
  mount();
  await screen.findByRole("article", { name: "web" }, { timeout: 5000 });
  for (const name of ["Retirer un réplica", "Ajouter un réplica"]) {
    const button = within(card("web")).getByRole("button", { name });
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("title", "Disponible en v0.2");
  }
  expect(screen.getByRole("button", { name: "Ajouter un service" })).toBeDisabled();
  expect(screen.getByRole("button", { name: /Nouveau service/ })).toBeDisabled();
  fireEvent.keyDown(within(card("web")).getByRole("button", { name: "Actions" }), { key: "Enter" });
  expect(await screen.findByRole("menuitem", { name: /Ouvrir un terminal/ })).toHaveAttribute("data-disabled");
  expect(screen.getByRole("menuitem", { name: /Ressources & limites/ })).toHaveAttribute("data-disabled");
  expect(screen.getByRole("menuitem", { name: "Arrêter" })).not.toHaveAttribute("data-disabled");
});

it("Redémarrer et Arrêter : POST avec le jeton CSRF, refus du serveur affiché", async () => {
  mount();
  await screen.findByRole("article", { name: "web" }, { timeout: 5000 });
  const restart = within(card("web")).getByRole("button", { name: "Redémarrer" });
  await waitFor(() => expect(restart).toBeEnabled());
  fireEvent.click(restart);
  await waitFor(() => expect(posts).toHaveLength(1));
  expect(posts[0]).toEqual({ path: `/api/projects/${WEB.id}/services/${WEB_SERVICES[0]!.id}/restart`, csrf: "csrf-1" });
  expect(await screen.findByText("Redémarrage de web demandé")).toBeInTheDocument();

  fireEvent.keyDown(within(card("web")).getByRole("button", { name: "Actions" }), { key: "Enter" });
  fireEvent.click(await screen.findByRole("menuitem", { name: "Arrêter" }));
  await waitFor(() => expect(posts).toHaveLength(2));
  expect(posts[1]).toEqual({ path: `/api/projects/${WEB.id}/services/${WEB_SERVICES[0]!.id}/stop`, csrf: "csrf-1" });
  expect(await screen.findByText("Arrêt de web demandé")).toBeInTheDocument();

  refusal = { status: 503, message: "Le serveur du projet est injoignable. Réessayez plus tard." };
  fireEvent.click(within(card("web")).getByRole("button", { name: "Redémarrer" }));
  expect(await screen.findByText("Le serveur du projet est injoignable. Réessayez plus tard.")).toBeInTheDocument();
});

it("service arrêté : Arrêter inactif, Redémarrer le relance ; rien de servi : actions inactives", async () => {
  mount();
  await screen.findByRole("article", { name: "worker" }, { timeout: 5000 });
  await waitFor(() => expect(within(card("worker")).getByRole("button", { name: "Redémarrer" })).toBeEnabled());
  expect(within(card("postgres")).getByRole("button", { name: "Redémarrer" })).toBeDisabled();
  fireEvent.keyDown(within(card("worker")).getByRole("button", { name: "Actions" }), { key: "Enter" });
  expect(await screen.findByRole("menuitem", { name: "Arrêter" })).toHaveAttribute("data-disabled");
});

it("lecteur : Redémarrer et Arrêter inactifs, aucun POST", async () => {
  role = "viewer";
  mount();
  await screen.findByRole("article", { name: "web" }, { timeout: 5000 });
  const restart = within(card("web")).getByRole("button", { name: "Redémarrer" });
  expect(restart).toBeDisabled();
  expect(restart).toHaveAttribute("title", "Réservé aux rôles développeur et plus");
  fireEvent.keyDown(within(card("web")).getByRole("button", { name: "Actions" }), { key: "Enter" });
  expect(await screen.findByRole("menuitem", { name: "Arrêter" })).toHaveAttribute("data-disabled");
  expect(posts).toHaveLength(0);
});

it("badge root sur le service qui tourne en root, absent des autres ; il ouvre les réglages", async () => {
  mount();
  await screen.findByRole("article", { name: "web" }, { timeout: 5000 });
  expect(within(card("web")).getByText("root")).toBeInTheDocument();
  expect(within(card("worker")).queryByText("root")).not.toBeInTheDocument();
  await waitFor(() => expect(within(card("web")).getByRole("button", { name: /S'exécute en root/ })).toBeEnabled());
  fireEvent.click(within(card("web")).getByRole("button", { name: /S'exécute en root/ }));
  expect(await screen.findByRole("dialog", { name: /Sécurité du conteneur/ })).toBeInTheDocument();
});

it("formulaire de sécurité : format vérifié, PATCH avec le jeton CSRF", async () => {
  mount();
  await screen.findByRole("article", { name: "web" }, { timeout: 5000 });
  fireEvent.keyDown(within(card("web")).getByRole("button", { name: "Actions" }), { key: "Enter" });
  await waitFor(() => expect(screen.getByRole("menuitem", { name: "Sécurité" })).not.toHaveAttribute("data-disabled"));
  fireEvent.click(screen.getByRole("menuitem", { name: "Sécurité" }));
  const dialog = await screen.findByRole("dialog", { name: /Sécurité du conteneur/ });
  const save = within(dialog).getByRole("button", { name: "Enregistrer" });
  const user = within(dialog).getByLabelText(/Utilisateur/);
  fireEvent.change(user, { target: { value: "root" } });
  expect(save).toBeDisabled();
  expect(within(dialog).getByText(/Format attendu/)).toBeInTheDocument();
  fireEvent.change(user, { target: { value: "1000:1000" } });
  expect(save).toBeEnabled();
  fireEvent.click(within(dialog).getByRole("switch", { name: "Racine en lecture seule" }));
  fireEvent.click(save);
  await waitFor(() => expect(patches).toHaveLength(1));
  expect(patches[0]).toEqual({ path: `/api/projects/${WEB.id}/services/${WEB_SERVICES[0]!.id}`, csrf: "csrf-1", body: { run_as: "1000:1000", read_only_rootfs: true } });
  expect(await screen.findByText(/prochain déploiement/)).toBeInTheDocument();
});

it("lecteur : réglages de sécurité inactifs", async () => {
  role = "viewer";
  mount();
  await screen.findByRole("article", { name: "web" }, { timeout: 5000 });
  fireEvent.keyDown(within(card("web")).getByRole("button", { name: "Actions" }), { key: "Enter" });
  expect(await screen.findByRole("menuitem", { name: "Sécurité" })).toHaveAttribute("data-disabled");
  expect(patches).toHaveLength(0);
});

it("ops : peut redémarrer mais la sécurité et le badge root restent en lecture seule", async () => {
  role = "ops";
  mount();
  await screen.findByRole("article", { name: "web" }, { timeout: 5000 });
  await waitFor(() => expect(within(card("web")).getByRole("button", { name: "Redémarrer" })).toBeEnabled());
  expect(within(card("web")).getByRole("button", { name: /S'exécute en root/ })).toBeDisabled();
  fireEvent.keyDown(within(card("web")).getByRole("button", { name: "Actions" }), { key: "Enter" });
  expect(await screen.findByRole("menuitem", { name: "Sécurité" })).toHaveAttribute("data-disabled");
  expect(patches).toHaveLength(0);
});
