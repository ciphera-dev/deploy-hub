import { createMemoryHistory } from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it } from "vitest";

import { AppProviders, createAppQueryClient } from "@/app/providers";
import { createAppRouter } from "@/app/router";
import { setCsrfToken } from "@/lib/api/csrf";
import { createdHook, DEPLOY_HOOKS, gitSettings } from "@/mocks/fixtures/git";
import { GITHUB_DISCONNECTED } from "@/mocks/fixtures/github";
import { PROJECT_SUMMARIES, projectDetail } from "@/mocks/fixtures/projects";

const WEB = PROJECT_SUMMARIES[0];
const IMAGE = PROJECT_SUMMARIES[5];
let role = "owner";
let patches: { body: unknown; csrf: string | null }[] = [];
let hookPosts: unknown[] = [];
let hookGets = 0;

const server = setupServer(
  http.get("*/api/me", () => HttpResponse.json({ id: "u1", name: "Alice Martin", email: "alice@example.com", role, csrf_token: "csrf-1" })),
  http.get("*/api/deployments", () => HttpResponse.json([])),
  http.get("*/api/connectors/github", () => HttpResponse.json(GITHUB_DISCONNECTED)),
  http.get("*/api/projects/:id", ({ params }) => {
    const found = PROJECT_SUMMARIES.find((p) => p.id === params.id)!;
    return HttpResponse.json(projectDetail(found));
  }),
  http.get("*/api/projects/:id/git", ({ params }) =>
    params.id === IMAGE.id
      ? HttpResponse.json({ error: { code: "not_git", message: "Ce projet déploie une image : il n'a pas de réglages Git." } }, { status: 409 })
      : HttpResponse.json(gitSettings("ciphera/web", "main")),
  ),
  http.patch("*/api/projects/:id/git", async ({ request }) => {
    const body = (await request.json()) as Record<string, unknown>;
    patches.push({ body, csrf: request.headers.get("x-csrf-token") });
    return HttpResponse.json({ ...gitSettings("ciphera/web", "main"), ...body });
  }),
  http.get("*/api/projects/:id/deploy-hooks", () => {
    hookGets += 1;
    return HttpResponse.json(DEPLOY_HOOKS);
  }),
  http.post("*/api/projects/:id/deploy-hooks", async ({ request }) => {
    const body = (await request.json()) as { name: string };
    hookPosts.push(body);
    return HttpResponse.json(createdHook(body.name), { status: 201 });
  }),
);

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  setCsrfToken("csrf-1");
  role = "owner";
  patches = [];
  hookPosts = [];
  hookGets = 0;
});
afterEach(() => server.resetHandlers());

function mount(url: string) {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [url] }));
  render(<AppProviders router={router} client={createAppQueryClient(router)} />);
  return router;
}

it("réglages chargés ; un chemin surveillé ajouté n'envoie que ce champ, avec le jeton CSRF", async () => {
  mount(`/projects/${WEB.id}/git`);
  const watch = await screen.findByRole("textbox", { name: "Chemins surveillés" }, { timeout: 5000 });
  expect(screen.getByText("apps/web/**")).toBeInTheDocument();
  expect(screen.getByDisplayValue("main")).toBeInTheDocument();
  expect(screen.getByDisplayValue("./")).toBeInTheDocument();
  expect(screen.queryByText("Modifications non enregistrées")).not.toBeInTheDocument();

  fireEvent.change(watch, { target: { value: "docs/**" } });
  fireEvent.keyDown(watch, { key: "Enter" });
  expect(await screen.findByText("Modifications non enregistrées")).toBeInTheDocument();
  screen.getByRole("button", { name: "Enregistrer" }).click();
  await waitFor(() => expect(patches).toHaveLength(1));
  expect(patches[0]).toEqual({ body: { watch_paths: ["apps/web/**", "packages/ui/**", "docs/**"] }, csrf: "csrf-1" });
  expect(await screen.findByText("Paramètres Git enregistrés")).toBeInTheDocument();
});

it("lecteur : réglages en lecture seule, deploy hooks réservés aux administrateurs (aucune requête)", async () => {
  role = "viewer";
  mount(`/projects/${WEB.id}/git`);
  const watch = await screen.findByRole("textbox", { name: "Chemins surveillés" }, { timeout: 5000 });
  expect(watch).toBeDisabled();
  expect(screen.getByDisplayValue("main")).toBeDisabled();
  expect(screen.getByRole("switch", { name: "Déploiement automatique" })).toBeDisabled();
  expect(screen.getByText("Réservé aux administrateurs.")).toBeInTheDocument();
  expect(hookGets).toBe(0);
});

it("deploy hook créé : URL secrète montrée une seule fois, puis la liste sans secret", async () => {
  mount(`/projects/${WEB.id}/git`);
  const hooks = await screen.findByRole("region", { name: "Deploy hooks" }, { timeout: 5000 });
  expect(await within(hooks).findByText(/CI externe/)).toBeInTheDocument();
  expect(within(hooks).queryByText(/dh_/)).not.toBeInTheDocument();

  within(hooks).getByRole("button", { name: "Créer un deploy hook" }).click();
  const dialog = await screen.findByRole("dialog", { name: "Deploy hook créé" });
  expect(hookPosts).toEqual([{ name: "Hook 2" }]);
  const created = createdHook("Hook 2");
  expect(within(dialog).getByTestId("token-value")).toHaveTextContent(`${location.origin}${created.url_path}`);
  expect(within(dialog).getByRole("alert")).toHaveTextContent("il ne sera plus jamais affiché");
  expect(within(dialog).getByText(`curl -X POST '${location.origin}${created.url_path}'`)).toBeInTheDocument();

  within(dialog).getByRole("button", { name: "J'ai copié le secret" }).click();
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  expect(screen.queryByText(new RegExp(created.token))).not.toBeInTheDocument();
});

it("projet image : pas de réglages Git", async () => {
  mount(`/projects/${IMAGE.id}/git`);
  expect(await screen.findByText("Ce projet déploie une image : il n'a pas de réglages Git.", {}, { timeout: 5000 })).toBeInTheDocument();
});
