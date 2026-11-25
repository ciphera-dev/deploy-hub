import { createMemoryHistory } from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it } from "vitest";

import { AppProviders, createAppQueryClient } from "@/app/providers";
import { createAppRouter } from "@/app/router";
import { setCsrfToken } from "@/lib/api/csrf";
import { emptyOverview } from "@/mocks/fixtures/overview";
import { PROJECT_SUMMARIES, projectDetail } from "@/mocks/fixtures/projects";

const ME = { id: "u1", name: "Alice Martin", email: "alice@example.com", role: "owner", csrf_token: "csrf-1" };
const FAILED = projectDetail(PROJECT_SUMMARIES[1]);

const server = setupServer(
  http.get("*/api/me", () => HttpResponse.json(ME)),
  http.get("*/api/deployments", () => HttpResponse.json([])),
  http.get("*/api/projects", () => HttpResponse.json([])),
  http.get("*/api/projects/:id", ({ params }) => {
    const found = PROJECT_SUMMARIES.find((p) => p.id === params.id);
    return found ? HttpResponse.json(projectDetail(found)) : HttpResponse.json({ error: { code: "not_found", message: "Projet introuvable." } }, { status: 404 });
  }),
  http.get("*/api/projects/:id/overview", () => HttpResponse.json({ ...emptyOverview({ ...FAILED.services[0]!, state: "failed", replicas: { running: 0, desired: 1 } }), status: "failed" })),
);

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => setCsrfToken("csrf-1"));
afterEach(() => server.resetHandlers());

function mount(url: string) {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [url] }));
  render(<AppProviders router={router} client={createAppQueryClient(router)} />);
  return router;
}

it("en-tête : nom, environnement, statut, dépôt ; boutons d'action", async () => {
  mount(`/projects/${FAILED.id}`);
  expect(await screen.findByRole("heading", { name: "billing-api" }, { timeout: 5000 })).toBeInTheDocument();
  expect(screen.getAllByText("Échec du build").length).toBeGreaterThan(0);
  expect(screen.getAllByText("ciphera/billing").length).toBeGreaterThan(0);
  expect(screen.getByRole("combobox")).toHaveTextContent("production");
  // Pas de domaine ; Redémarrer et Redéployer sont actifs pour un propriétaire.
  expect(screen.getByRole("button", { name: "Visiter" })).toBeDisabled();
  await waitFor(() => expect(screen.getByRole("button", { name: "Redéployer" })).toBeEnabled());
  expect(screen.getByRole("button", { name: "Redémarrer" })).toBeEnabled();
});

it("Redémarrer : POST /restart avec le jeton CSRF, refus du serveur affiché", async () => {
  const posts: { path: string; csrf: string | null }[] = [];
  let status = 202;
  server.use(
    http.post("*/api/projects/:id/restart", ({ request }) => {
      posts.push({ path: new URL(request.url).pathname, csrf: request.headers.get("x-csrf-token") });
      return status === 202
        ? HttpResponse.json({ deployment_id: "d1" }, { status: 202 })
        : HttpResponse.json({ error: { code: "nothing_serving", message: "Aucun déploiement en ligne à redémarrer." } }, { status: 409 });
    }),
  );
  mount(`/projects/${FAILED.id}`);
  await screen.findByRole("heading", { name: "billing-api" }, { timeout: 5000 });
  const restart = screen.getByRole("button", { name: "Redémarrer" });
  await waitFor(() => expect(restart).toBeEnabled());
  restart.click();
  await waitFor(() => expect(posts).toHaveLength(1));
  expect(posts[0]).toEqual({ path: `/api/projects/${FAILED.id}/restart`, csrf: "csrf-1" });
  expect(await screen.findByText("Redémarrage demandé")).toBeInTheDocument();
  status = 409;
  restart.click();
  expect(await screen.findByText("Aucun déploiement en ligne à redémarrer.")).toBeInTheDocument();
});

it("onglets : Aperçu, Services, Déploiements, Logs, Variables, Domaines et Git sont des liens", async () => {
  mount(`/projects/${FAILED.id}`);
  await screen.findByRole("heading", { name: "billing-api" }, { timeout: 5000 });
  const nav = screen.getByRole("navigation", { name: "Onglets du projet" });
  const overview = within(nav).getByRole("link", { name: "Aperçu" });
  expect(overview).toHaveAttribute("aria-current", "page");
  expect(within(nav).getAllByRole("link")).toHaveLength(7);
  expect(within(nav).getByRole("link", { name: "Services 1" })).toHaveAttribute("href", `/projects/${FAILED.id}/services`);
  expect(within(nav).getByRole("link", { name: "Déploiements" })).toHaveAttribute("href", `/projects/${FAILED.id}/deployments`);
  expect(within(nav).getByRole("link", { name: "Logs" })).toHaveAttribute("href", `/projects/${FAILED.id}/logs`);
  expect(within(nav).getByRole("link", { name: "Variables" })).toHaveAttribute("href", `/projects/${FAILED.id}/variables`);
  expect(within(nav).getByRole("link", { name: "Domaines" })).toHaveAttribute("href", `/projects/${FAILED.id}/domains`);
  expect(within(nav).getByRole("link", { name: "Git" })).toHaveAttribute("href", `/projects/${FAILED.id}/git`);
  expect(nav.querySelector("[aria-disabled]")).toBeNull();
});

it("aperçu : statut du projet, build et service", async () => {
  mount(`/projects/${FAILED.id}`);
  await screen.findByRole("heading", { name: "billing-api" }, { timeout: 5000 });
  const prod = await screen.findByRole("region", { name: "Production" });
  expect(within(prod).getByText("Dockerfile")).toBeInTheDocument();
  expect(within(prod).getByText("main")).toBeInTheDocument();
  expect(within(screen.getByRole("region", { name: "Services" })).getByText("billing-api")).toBeInTheDocument();
});

it("« Projets » ramène à la liste", async () => {
  mount(`/projects/${FAILED.id}`);
  const back = await screen.findByRole("link", { name: "Projets" }, { timeout: 5000 });
  expect(back).toHaveAttribute("href", "/projects");
});

it("projet inconnu : message, sans écran d'erreur", async () => {
  mount("/projects/00000000-0000-4000-8000-0000000000ff");
  expect(await screen.findByText("Projet introuvable.")).toBeInTheDocument();
});

it("Supprimer : volumes gardés par défaut, case cochée envoyée, retour à la liste", async () => {
  const deletes: { path: string; purge: string | null; csrf: string | null }[] = [];
  server.use(
    http.delete("*/api/projects/:id", ({ request }) => {
      const url = new URL(request.url);
      deletes.push({ path: url.pathname, purge: url.searchParams.get("purge_volumes"), csrf: request.headers.get("x-csrf-token") });
      return new HttpResponse(null, { status: 204 });
    }),
  );
  const router = mount(`/projects/${FAILED.id}`);
  await screen.findByRole("heading", { name: "billing-api" }, { timeout: 5000 });
  (await screen.findByRole("button", { name: "Supprimer" })).click();
  const dialog = await screen.findByRole("dialog");
  const purge = within(dialog).getByRole("checkbox", { name: /Supprimer aussi les volumes \(données\)/ });
  expect(purge).not.toBeChecked();
  const confirm = within(dialog).getByRole("button", { name: "Supprimer le projet" });
  expect(confirm).toBeDisabled();
  fireEvent.change(within(dialog).getByLabelText("Nom du projet"), { target: { value: "billing-api" } });
  expect(confirm).toBeEnabled();
  confirm.click();
  await waitFor(() => expect(deletes).toHaveLength(1));
  expect(deletes[0]).toEqual({ path: `/api/projects/${FAILED.id}`, purge: "false", csrf: "csrf-1" });
  await waitFor(() => expect(router.state.location.pathname).toBe("/projects"));
  expect(await screen.findByText("Projet supprimé")).toBeInTheDocument();
});

it("Supprimer : case cochée, purge_volumes=true", async () => {
  const purges: (string | null)[] = [];
  server.use(
    http.delete("*/api/projects/:id", ({ request }) => {
      purges.push(new URL(request.url).searchParams.get("purge_volumes"));
      return new HttpResponse(null, { status: 204 });
    }),
  );
  mount(`/projects/${FAILED.id}`);
  await screen.findByRole("heading", { name: "billing-api" }, { timeout: 5000 });
  (await screen.findByRole("button", { name: "Supprimer" })).click();
  const dialog = await screen.findByRole("dialog");
  within(dialog).getByRole("checkbox", { name: /Supprimer aussi les volumes/ }).click();
  fireEvent.change(within(dialog).getByLabelText("Nom du projet"), { target: { value: "billing-api" } });
  within(dialog).getByRole("button", { name: "Supprimer le projet" }).click();
  await waitFor(() => expect(purges).toEqual(["true"]));
});

it("Supprimer : absent pour un développeur", async () => {
  server.use(http.get("*/api/me", () => HttpResponse.json({ ...ME, role: "developer" })));
  mount(`/projects/${FAILED.id}`);
  await screen.findByRole("heading", { name: "billing-api" }, { timeout: 5000 });
  await waitFor(() => expect(screen.getByRole("button", { name: "Redéployer" })).toBeEnabled());
  expect(screen.queryByRole("button", { name: "Supprimer" })).toBeNull();
});
