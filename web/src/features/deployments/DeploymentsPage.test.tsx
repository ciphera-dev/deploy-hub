import { createMemoryHistory } from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it } from "vitest";

import { AppProviders, createAppQueryClient } from "@/app/providers";
import { createAppRouter } from "@/app/router";
import { setCsrfToken } from "@/lib/api/csrf";
import { DEPLOYMENTS, queuedDeployment } from "@/mocks/fixtures/deployments";
import { PROJECT_SUMMARIES } from "@/mocks/fixtures/projects";

let role = "owner";
let posts: { path: string; csrf: string | null; body: unknown }[];
let deployments: typeof DEPLOYMENTS;
let listFails = false;
let listHtml = false;
let meHtml = false;

const server = setupServer(
  http.get("*/api/me", () => (meHtml ? new HttpResponse("<!doctype html>", { headers: { "Content-Type": "text/html" } }) : HttpResponse.json({ id: "u1", name: "Alice Martin", email: "alice@example.com", role, csrf_token: "csrf-1" }))),
  http.get("*/api/projects", () => HttpResponse.json(PROJECT_SUMMARIES)),
  http.get("*/api/deployments", ({ request }) => {
    if (listHtml) return new HttpResponse("<!doctype html><html></html>", { headers: { "Content-Type": "text/html" } });
    if (listFails) return HttpResponse.json({ error: { code: "bad", message: "x" } }, { status: 422 });
    const q = new URL(request.url).searchParams;
    return HttpResponse.json(deployments.filter((d) => (!q.get("status") || d.status === q.get("status")) && (!q.get("project") || d.project_id === q.get("project"))));
  }),
  http.get("*/api/deployments/:id", ({ params }) => {
    const found = deployments.find((d) => d.id === params.id);
    return found ? HttpResponse.json(found) : HttpResponse.json({ error: { code: "not_found", message: "Déploiement introuvable." } }, { status: 404 });
  }),
  http.post("*/api/projects/:id/redeploy", async ({ request, params }) => {
    posts.push({ path: new URL(request.url).pathname, csrf: request.headers.get("x-csrf-token"), body: (await request.text()) || null });
    return HttpResponse.json(queuedDeployment(String(params.id)), { status: 201 });
  }),
  http.post("*/api/deployments/:id/cancel", async ({ request, params }) => {
    posts.push({ path: new URL(request.url).pathname, csrf: request.headers.get("x-csrf-token"), body: null });
    return HttpResponse.json({ ...deployments.find((d) => d.id === params.id)!, status: "cancelled" });
  }),
);

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  role = "owner";
  posts = [];
  deployments = DEPLOYMENTS;
  listFails = false;
  listHtml = false;
  meHtml = false;
  setCsrfToken("csrf-1");
});
afterEach(() => server.resetHandlers());

function mount(url: string) {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [url] }));
  render(<AppProviders router={router} client={createAppQueryClient(router)} />);
  return router;
}

const loaded = () => screen.findByText("Déploiement manuel", {}, { timeout: 5000 });
const rowOf = (number: number) => screen.getByText(`#${number} · ${DEPLOYMENTS.find((d) => d.number === number)!.commit_sha!.slice(0, 7)} · main`).closest("[data-deployment]") as HTMLElement;

it("liste : une ligne par déploiement, avec projet, statut et déclencheur", async () => {
  mount("/deployments");
  expect(await screen.findByText("Déploiement manuel", {}, { timeout: 5000 })).toBeInTheDocument();
  expect(document.querySelectorAll("[data-deployment]")).toHaveLength(DEPLOYMENTS.length);
  const failed = within(rowOf(4807));
  expect(failed.getByText("Échec")).toBeInTheDocument();
  expect(failed.getByText("ciphera-web")).toBeInTheDocument();
  expect(within(rowOf(4815)).getByText("billing-api")).toBeInTheDocument();
  expect(within(rowOf(4821)).getByText(/En cours/)).toBeInTheDocument();
});

it("onglets : Tous, En cours, Réussis, Échecs filtrent la liste et affichent leur compte", async () => {
  mount("/deployments");
  await loaded();
  const count = (name: string) => within(screen.getByRole("tab", { name: new RegExp(name) })).getByText(/\d+/).textContent;
  expect([count("Tous"), count("En cours"), count("Réussis"), count("Échecs")]).toEqual(["6", "1", "3", "1"]);
  fireEvent.click(screen.getByRole("tab", { name: /Échecs/ }));
  expect(document.querySelectorAll("[data-deployment]")).toHaveLength(1);
  expect(screen.getByText(/#4807/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("tab", { name: /En cours/ }));
  expect(document.querySelectorAll("[data-deployment]")).toHaveLength(1);
  expect(screen.getByText(/#4821/)).toBeInTheDocument();
});

it("un échec déplié montre sa cause ; un déploiement en file ou en cours peut être annulé, pas un terminé", async () => {
  mount("/deployments");
  await loaded();
  expect(within(rowOf(4821)).getByRole("button", { name: "Annuler" })).toBeInTheDocument();
  for (const n of [4818, 4812, 4809, 4807, 4815]) expect(within(rowOf(n)).queryByRole("button", { name: "Annuler" })).not.toBeInTheDocument();
  fireEvent.click(within(rowOf(4807)).getByText("Échec"));
  expect(await screen.findByText("pnpm install a échoué (ERR_PNPM_LOCKFILE)")).toBeInTheDocument();
});

it("annuler envoie POST /api/deployments/:id/cancel avec le jeton CSRF", async () => {
  mount("/deployments");
  await loaded();
  fireEvent.click(within(rowOf(4821)).getByRole("button", { name: "Annuler" }));
  await waitFor(() => expect(posts).toHaveLength(1));
  expect(posts[0]).toEqual({ path: `/api/deployments/${DEPLOYMENTS[0].id}/cancel`, csrf: "csrf-1", body: null });
});

it("viewer : lecture seule, ni annulation ni déploiement", async () => {
  role = "viewer";
  mount("/deployments");
  await loaded();
  await waitFor(() => expect(screen.queryByRole("button", { name: "Annuler" })).not.toBeInTheDocument());
  expect(screen.queryByRole("button", { name: "Déployer un projet" })).not.toBeInTheDocument();
  // La page de détail reste accessible (lien).
  expect(within(rowOf(4821)).getByRole("link", { name: "Détails" })).toHaveAttribute("href", `/deployments/${DEPLOYMENTS[0].id}`);
});

it("« Déployer un projet » : choisit un projet et envoie POST /api/projects/:id/deployments", async () => {
  mount("/deployments");
  const trigger = await screen.findByRole("button", { name: "Déployer un projet" }, { timeout: 5000 });
  await loaded();
  fireEvent.keyDown(trigger, { key: "Enter" });
  fireEvent.click(await screen.findByRole("menuitem", { name: /billing-api/ }));
  await waitFor(() => expect(posts).toHaveLength(1));
  expect(posts[0]).toEqual({ path: `/api/projects/${PROJECT_SUMMARIES[1].id}/redeploy`, csrf: "csrf-1", body: null });
  expect(await screen.findByText("Déploiement #4900 lancé")).toBeInTheDocument();
});

it("garde « déjà en cours » : un projet qui déploie ne reçoit pas de second déploiement", async () => {
  mount("/deployments");
  const trigger = await screen.findByRole("button", { name: "Déployer un projet" }, { timeout: 5000 });
  await screen.findByText(/#4821/);
  fireEvent.keyDown(trigger, { key: "Enter" });
  fireEvent.click(await screen.findByRole("menuitem", { name: /ciphera-web/ }));
  expect(await screen.findByText("Un déploiement est déjà en cours")).toBeInTheDocument();
  expect(posts).toHaveLength(0);
});

it("détail par URL : en-tête, frise, détails et zone de logs sans fausses lignes", async () => {
  mount(`/deployments/${DEPLOYMENTS[1].id}`);
  expect(await screen.findByRole("heading", { name: "Déploiement manuel" }, { timeout: 5000 })).toBeInTheDocument();
  expect(screen.getByText("Actuel")).toBeInTheDocument();
  expect(screen.getByText("#4818")).toBeInTheDocument();
  expect(screen.getByRole("list", { name: "Étapes du déploiement" })).toBeInTheDocument();
  const details = screen.getByRole("region", { name: "Détails du déploiement" });
  expect(within(details).getByText("Projet")).toBeInTheDocument();
  expect(within(details).getByText("ciphera-web")).toBeInTheDocument();
  expect(within(details).getByText("1 min 36 s")).toBeInTheDocument();
  expect(within(details).getByText("Alex Martin")).toBeInTheDocument();
  const logs = screen.getByRole("region", { name: "Logs de build" });
  expect(logs).toHaveTextContent("Aucun log");
  // Un déploiement réussi et servi : ni annulation ni rollback (c'est déjà la version en ligne).
  expect(screen.queryByRole("button", { name: "Annuler" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Rollback" })).not.toBeInTheDocument();
  expect(screen.getByRole("link", { name: "Retour aux déploiements" })).toHaveAttribute("href", "/deployments");
});

it("détail : rollback proposé seulement si l'image est conservée et que ce n'est pas la version servie", async () => {
  mount(`/deployments/${DEPLOYMENTS[2].id}`);
  expect(await screen.findByRole("button", { name: "Rollback" }, { timeout: 5000 })).toBeInTheDocument();
});

it("détail : déploiement inconnu", async () => {
  mount("/deployments/10000000-0000-4000-8000-0000000000ff");
  expect(await screen.findByText("Déploiement introuvable.", {}, { timeout: 5000 })).toBeInTheDocument();
});

it("liste en échec : message d'erreur à la place du tableau", async () => {
  listFails = true;
  mount("/deployments");
  expect(await screen.findByRole("alert", {}, { timeout: 5000 })).toHaveTextContent("Chargement des déploiements impossible pour le moment.");
});

it("ligne dépliable au clavier : bouton avec aria-expanded, Entrée et Espace", async () => {
  mount("/deployments");
  await loaded();
  const toggle = within(rowOf(4807)).getByRole("button", { name: /Détails de #4807/ });
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  fireEvent.keyDown(toggle, { key: "Enter" });
  expect(toggle).toHaveAttribute("aria-expanded", "true");
  fireEvent.keyDown(toggle, { key: " " });
  expect(toggle).toHaveAttribute("aria-expanded", "false");
});

it("aucune API derrière (HTML partout, comme l'e2e) : l'en-tête et le message d'erreur restent affichés", async () => {
  listHtml = true;
  meHtml = true;
  mount("/deployments");
  expect(await screen.findByRole("alert", {}, { timeout: 5000 })).toHaveTextContent("Chargement des déploiements impossible pour le moment.");
  expect(screen.getByRole("heading", { name: "Déploiements" })).toBeInTheDocument();
});

it("aucune API derrière : le lien profond garde l'entrée active de la navigation", async () => {
  listHtml = true;
  meHtml = true;
  mount("/deployments");
  expect(await screen.findByRole("heading", { name: "Déploiements" }, { timeout: 5000 })).toBeInTheDocument();
  await new Promise((r) => setTimeout(r, 1500));
  expect(screen.getByRole("heading", { name: "Déploiements" })).toBeInTheDocument();
  expect(within(screen.getByRole("navigation", { name: "Navigation principale" })).getByRole("link", { name: /Déploiements/ })).toHaveAttribute("aria-current", "page");
});
