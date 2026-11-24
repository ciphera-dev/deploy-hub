import { createMemoryHistory } from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it } from "vitest";

import { AppProviders, createAppQueryClient } from "@/app/providers";
import { createAppRouter } from "@/app/router";
import { setCsrfToken } from "@/lib/api/csrf";
import { DEPLOYMENTS, queuedDeployment } from "@/mocks/fixtures/deployments";
import { PROJECT_SUMMARIES, projectDetail } from "@/mocks/fixtures/projects";

const WEB = PROJECT_SUMMARIES[0];
const WEB_DEPLOYMENTS = DEPLOYMENTS.filter((d) => d.project_id === WEB.id);
const IDLE = WEB_DEPLOYMENTS.filter((d) => d.number !== 4821);

let role = "developer";
let list: typeof DEPLOYMENTS;
let listReply: (() => Response) | null;
let delay = 0;
let activeFails = false;
let posts: { path: string; csrf: string | null; body: unknown }[];

const server = setupServer(
  http.get("*/api/me", () => HttpResponse.json({ id: "u1", name: "Alice Martin", email: "alice@example.com", role, csrf_token: "csrf-1" })),
  http.get("*/api/projects/:id", ({ params }) => {
    const found = PROJECT_SUMMARIES.find((p) => p.id === params.id);
    return found ? HttpResponse.json(projectDetail(found)) : HttpResponse.json({ error: { code: "not_found", message: "Projet introuvable." } }, { status: 404 });
  }),
  http.get("*/api/deployments", async ({ request }) => {
    if (delay) await new Promise((r) => setTimeout(r, delay));
    if (listReply) return listReply();
    if (activeFails && new URL(request.url).searchParams.get("status")) return HttpResponse.json({ error: { code: "bad", message: "x" } }, { status: 422 });
    const status = new URL(request.url).searchParams.get("status");
    return HttpResponse.json(list.filter((d) => !status || d.status === status));
  }),
  http.post("*/api/projects/:id/redeploy", async ({ request, params }) => {
    posts.push({ path: new URL(request.url).pathname, csrf: request.headers.get("x-csrf-token"), body: (await request.text()) || null });
    return HttpResponse.json(queuedDeployment(String(params.id)), { status: 201 });
  }),
  http.post("*/api/deployments/:id/rollback", async ({ request }) => {
    posts.push({ path: new URL(request.url).pathname, csrf: request.headers.get("x-csrf-token"), body: null });
    return HttpResponse.json({ ...queuedDeployment(WEB.id, 4901), trigger: "rollback" }, { status: 201 });
  }),
  http.post("*/api/deployments/:id/cancel", async ({ request }) => {
    posts.push({ path: new URL(request.url).pathname, csrf: request.headers.get("x-csrf-token"), body: null });
    return HttpResponse.json({ ...WEB_DEPLOYMENTS[0], status: "cancelled" });
  }),
);

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  role = "developer";
  list = WEB_DEPLOYMENTS;
  listReply = null;
  delay = 0;
  activeFails = false;
  posts = [];
  setCsrfToken("csrf-1");
});
afterEach(() => server.resetHandlers());

function mount() {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [`/projects/${WEB.id}/deployments`] }));
  render(<AppProviders router={router} client={createAppQueryClient(router)} />);
  return router;
}

const item = (number: number) => document.querySelector(`[data-deployment="${DEPLOYMENTS.find((d) => d.number === number)!.id}"]`) as HTMLElement;
const ready = () => screen.findByRole("region", { name: "Détail du déploiement" }, { timeout: 5000 });
const panel = () => screen.getByRole("region", { name: "Détail du déploiement" });

it("l'onglet Déploiements est un lien actif du projet", async () => {
  mount();
  const nav = await screen.findByRole("navigation", { name: "Onglets du projet" }, { timeout: 5000 });
  const tab = within(nav).getByRole("link", { name: "Déploiements" });
  expect(tab).toHaveAttribute("href", `/projects/${WEB.id}/deployments`);
  expect(tab).toHaveAttribute("aria-current", "page");
});

it("timeline : un élément par déploiement du projet, le plus récent sélectionné, « Actuel » sur la version servie", async () => {
  mount();
  await ready();
  expect(document.querySelectorAll("[data-deployment]")).toHaveLength(WEB_DEPLOYMENTS.length);
  expect(within(item(4818)).getByText("Actuel")).toBeInTheDocument();
  expect(within(item(4812)).queryByText("Actuel")).not.toBeInTheDocument();
  expect(item(4821)).toHaveAttribute("aria-current", "true");
  // Le panneau montre le déploiement sélectionné avec sa frise en 6 étapes.
  expect(within(panel()).getByRole("list", { name: "Étapes du déploiement" })).toBeInTheDocument();
  fireEvent.click(item(4807));
  expect(item(4807)).toHaveAttribute("aria-current", "true");
  expect(within(panel()).getByRole("alert")).toHaveTextContent("pnpm install a échoué (ERR_PNPM_LOCKFILE)");
});

it("en cours : Annuler visible, ni Rollback ni Redéployer ; terminé : l'inverse", async () => {
  mount();
  await ready();
  expect(within(panel()).getByRole("button", { name: "Annuler" })).toBeInTheDocument();
  expect(within(panel()).queryByRole("button", { name: "Rollback" })).not.toBeInTheDocument();
  fireEvent.click(item(4812));
  expect(within(panel()).queryByRole("button", { name: "Annuler" })).not.toBeInTheDocument();
  expect(within(panel()).getByRole("button", { name: "Rollback" })).toBeInTheDocument();
  // La version servie ne se « rollback » pas vers elle-même ; un échec n'a pas d'image à rejouer.
  fireEvent.click(item(4818));
  expect(within(panel()).queryByRole("button", { name: "Rollback" })).not.toBeInTheDocument();
  fireEvent.click(item(4807));
  expect(within(panel()).queryByRole("button", { name: "Rollback" })).not.toBeInTheDocument();
  expect(within(panel()).getByRole("button", { name: "Redéployer" })).toBeInTheDocument();
});

it("garde « déjà en cours » : Redéployer n'envoie rien tant qu'un déploiement tourne", async () => {
  mount();
  await ready();
  fireEvent.click(item(4812));
  fireEvent.click(await within(panel()).findByRole("button", { name: "Redéployer" }));
  expect(await screen.findByText("Un déploiement est déjà en cours")).toBeInTheDocument();
  expect(posts).toHaveLength(0);
});

it("redéployer envoie POST /api/projects/:id/deployments avec l'environnement et le jeton CSRF", async () => {
  list = IDLE;
  mount();
  await ready();
  fireEvent.click(await within(panel()).findByRole("button", { name: "Redéployer" }));
  await waitFor(() => expect(posts).toHaveLength(1));
  expect(posts[0]).toEqual({ path: `/api/projects/${WEB.id}/redeploy`, csrf: "csrf-1", body: null });
  expect(await screen.findByText("Déploiement #4900 lancé")).toBeInTheDocument();
});

it("le bouton « Redéployer » de l'en-tête du projet envoie le même corps", async () => {
  list = IDLE;
  mount();
  await screen.findByRole("heading", { name: "ciphera-web" }, { timeout: 5000 });
  await ready();
  const button = screen.getAllByRole("button", { name: "Redéployer" }).find((b) => !panel().contains(b))!;
  await waitFor(() => expect(button).toBeEnabled());
  fireEvent.click(button);
  await waitFor(() => expect(posts).toHaveLength(1));
  expect(posts[0].body).toBeNull();
});

it("rollback : envoie POST /api/deployments/:id/rollback", async () => {
  list = IDLE;
  mount();
  await ready();
  fireEvent.click(item(4812));
  fireEvent.click(within(panel()).getByRole("button", { name: "Rollback" }));
  // Confirmation avant de basculer la production (Modal de la maquette).
  fireEvent.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Rollback" }));
  await waitFor(() => expect(posts).toHaveLength(1));
  expect(posts[0]).toEqual({ path: `/api/deployments/${IDLE.find((d) => d.number === 4812)!.id}/rollback`, csrf: "csrf-1", body: null });
});

it("annuler envoie POST /api/deployments/:id/cancel", async () => {
  mount();
  await ready();
  fireEvent.click(within(panel()).getByRole("button", { name: "Annuler" }));
  await waitFor(() => expect(posts).toHaveLength(1));
  expect(posts[0].path).toBe(`/api/deployments/${WEB_DEPLOYMENTS[0].id}/cancel`);
});

it("viewer : lecture seule (ni Annuler, ni Rollback, ni Redéployer)", async () => {
  role = "viewer";
  mount();
  await ready();
  fireEvent.click(item(4812));
  await waitFor(() => expect(screen.getByRole("button", { name: "Redéployer", hidden: false })).toBeDisabled());
  expect(within(panel()).queryByRole("button", { name: "Annuler" })).not.toBeInTheDocument();
  expect(within(panel()).queryByRole("button", { name: "Rollback" })).not.toBeInTheDocument();
  expect(within(panel()).queryByRole("button", { name: "Redéployer" })).not.toBeInTheDocument();
});

it("projet sans déploiement : message, rien d'inventé", async () => {
  list = [];
  mount();
  expect(await screen.findByText("Aucun déploiement pour ce projet.", {}, { timeout: 5000 })).toBeInTheDocument();
});

it("liste en échec : message d'erreur, pas de zone blanche", async () => {
  listReply = () => HttpResponse.json({ error: { code: "bad", message: "x" } }, { status: 422 });
  mount();
  expect(await screen.findByRole("alert", {}, { timeout: 5000 })).toHaveTextContent("Chargement des déploiements impossible pour le moment.");
});

it("liste en chargement : état de chargement affiché", async () => {
  delay = 300;
  mount();
  expect(await screen.findByText("Chargement des déploiements…", {}, { timeout: 5000 })).toBeInTheDocument();
  await ready();
});

it("garde : Redéployer reste inactif tant que la liste charge, avec une explication", async () => {
  delay = 400;
  mount();
  const button = await screen.findByRole("button", { name: "Redéployer" }, { timeout: 5000 });
  expect(button).toBeDisabled();
  expect(button).toHaveAttribute("title", "Vérification des déploiements en cours…");
  await ready();
  await waitFor(() => expect(screen.getAllByRole("button", { name: "Redéployer" }).every((b) => !b.hasAttribute("disabled"))).toBe(true));
});

it("garde : liste en échec, Redéployer reste inactif", async () => {
  listReply = () => HttpResponse.json({ error: { code: "bad", message: "x" } }, { status: 422 });
  mount();
  await screen.findByRole("alert", {}, { timeout: 5000 });
  const button = screen.getByRole("button", { name: "Redéployer" });
  expect(button).toBeDisabled();
  expect(button).toHaveAttribute("title", "Liste des déploiements indisponible.");
});

it("garde : le serveur est interrogé au clic (filtre status), pas seulement la liste affichée", async () => {
  list = IDLE;
  mount();
  await ready();
  // Un déploiement démarre entre l'affichage de la liste et le clic.
  list = [...WEB_DEPLOYMENTS];
  const button = screen.getAllByRole("button", { name: "Redéployer" }).find((b) => !panel().contains(b))!;
  await waitFor(() => expect(button).toBeEnabled());
  fireEvent.click(button);
  expect(await screen.findByText("Un déploiement est déjà en cours")).toBeInTheDocument();
  expect(posts).toHaveLength(0);
});

it("deux clics rapides sur Redéployer : un seul POST", async () => {
  list = IDLE;
  mount();
  await ready();
  const button = screen.getAllByRole("button", { name: "Redéployer" }).find((b) => !panel().contains(b))!;
  await waitFor(() => expect(button).toBeEnabled());
  delay = 200;
  fireEvent.click(button);
  fireEvent.click(button);
  await waitFor(() => expect(posts).toHaveLength(1));
  await new Promise((r) => setTimeout(r, 600));
  expect(posts).toHaveLength(1);
});

it("vérification serveur en échec : message, aucun POST", async () => {
  list = IDLE;
  mount();
  await ready();
  activeFails = true;
  fireEvent.click(within(panel()).getByRole("button", { name: "Redéployer" }));
  expect(await screen.findByText("Vérification impossible. Réessayez.")).toBeInTheDocument();
  expect(posts).toHaveLength(0);
});
