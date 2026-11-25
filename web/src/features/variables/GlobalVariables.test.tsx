import { createMemoryHistory } from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";

import { AppProviders, createAppQueryClient } from "@/app/providers";
import { createAppRouter } from "@/app/router";
import { setCsrfToken } from "@/lib/api/csrf";
import { PROJECT_SUMMARIES } from "@/mocks/fixtures/projects";
import { exportedGlobals, GLOBAL_VARIABLES } from "@/mocks/fixtures/variables";

let role = "owner";
let gets = 0;
let posts: { body: unknown; csrf: string | null }[] = [];
let exports: (string | null)[] = [];

const server = setupServer(
  http.get("*/api/me", () => HttpResponse.json({ id: "u1", name: "Alice Martin", email: "alice@example.com", role, csrf_token: "csrf-1" })),
  http.get("*/api/projects", () => HttpResponse.json(PROJECT_SUMMARIES)),
  http.get("*/api/variables/export", ({ request }) => {
    const env = new URL(request.url).searchParams.get("env");
    exports.push(env);
    return new HttpResponse(exportedGlobals(env), { headers: { "content-type": "text/plain; charset=utf-8" } });
  }),
  http.get("*/api/variables", () => {
    gets += 1;
    return HttpResponse.json(GLOBAL_VARIABLES);
  }),
  http.post("*/api/variables", async ({ request }) => {
    posts.push({ body: await request.json(), csrf: request.headers.get("x-csrf-token") });
    return HttpResponse.json(GLOBAL_VARIABLES[0], { status: 201 });
  }),
);

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  setCsrfToken("csrf-1");
  role = "owner";
  gets = 0;
  posts = [];
  exports = [];
});
afterEach(() => server.resetHandlers());

function mount() {
  const router = createAppRouter(createMemoryHistory({ initialEntries: ["/variables"] }));
  render(<AppProviders router={router} client={createAppQueryClient(router)} />);
}

it("liste : secrets en écriture seule, portée environnements × projets, priorité", async () => {
  mount();
  const secret = await screen.findByTestId("gvar-RESEND_API_KEY", {}, { timeout: 5000 });
  expect(within(secret).getByText("Écriture seule")).toBeInTheDocument();
  expect(within(secret).queryByRole("button", { name: "Copier" })).not.toBeInTheDocument();
  const bucket = screen.getByTestId("gvar-S3_BUCKET");
  expect(bucket).toHaveTextContent("2");
  expect(screen.getByText("1. Variable du projet")).toBeInTheDocument();
  // Onglet Staging : filtre par environnement.
  screen.getByRole("button", { name: /^Staging/ }).click();
  await waitFor(() => expect(screen.queryByTestId("gvar-RESEND_API_KEY")).not.toBeInTheDocument());
  expect(screen.getAllByTestId("gvar-LOG_LEVEL")).toHaveLength(1);
});

it("nouvelle variable : POST avec le jeton CSRF et la sélection de projets", async () => {
  mount();
  await screen.findByTestId("gvar-SENTRY_DSN", {}, { timeout: 5000 });
  screen.getByRole("button", { name: "Nouvelle variable" }).click();
  const dialog = await screen.findByRole("dialog", { name: "Nouvelle variable globale" });
  fireEvent.change(within(dialog).getByLabelText("Clé"), { target: { value: "api_url" } });
  fireEvent.change(within(dialog).getByLabelText("Valeur"), { target: { value: "https://api.ciphera.dev" } });
  within(dialog).getByRole("button", { name: "Preview" }).click();
  within(dialog).getByRole("button", { name: "Sélection" }).click();
  (await within(dialog).findByRole("button", { name: "billing-api" })).click();
  await waitFor(() => expect(within(dialog).getByRole("button", { name: "billing-api" })).toHaveAttribute("aria-pressed", "true"));
  const save = within(dialog).getByRole("button", { name: "Enregistrer" });
  await waitFor(() => expect(save).toBeEnabled());
  save.click();
  await waitFor(() => expect(posts).toHaveLength(1));
  expect(posts[0]).toEqual({
    body: { key: "API_URL", value: "https://api.ciphera.dev", secret: false, environments: ["production", "staging"], all_projects: false, projects: [PROJECT_SUMMARIES[1].id] },
    csrf: "csrf-1",
  });
});

it("secret : « Tous les projets » est désactivé, avec explication", async () => {
  mount();
  await screen.findByTestId("gvar-SENTRY_DSN", {}, { timeout: 5000 });
  screen.getByRole("button", { name: "Nouvelle variable" }).click();
  const dialog = await screen.findByRole("dialog", { name: "Nouvelle variable globale" });
  const all = within(dialog).getByRole("button", { name: "Tous les projets" });
  expect(all).toBeEnabled();
  expect(all).toHaveAttribute("aria-pressed", "true");
  within(dialog).getByRole("switch", { name: "Secret" }).click();
  await waitFor(() => expect(all).toBeDisabled());
  expect(all).toHaveAttribute("aria-pressed", "false");
  expect(within(dialog).getByText("Un secret nomme ses projets : il n'est jamais envoyé à tous les projets, y compris ceux de demain.")).toBeInTheDocument();
  expect(within(dialog).getByRole("button", { name: "Sélection" })).toHaveAttribute("aria-pressed", "true");
  // Sans projet choisi, l'enregistrement reste impossible.
  fireEvent.change(within(dialog).getByLabelText("Clé"), { target: { value: "TOKEN" } });
  fireEvent.change(within(dialog).getByLabelText("Valeur"), { target: { value: "s3cret-value" } });
  expect(within(dialog).getByRole("button", { name: "Enregistrer" })).toBeDisabled();
  (await within(dialog).findByRole("button", { name: "billing-api" })).click();
  await waitFor(() => expect(within(dialog).getByRole("button", { name: "Enregistrer" })).toBeEnabled());
});

it("export : fichier du serveur, sans secrets", async () => {
  const createObjectURL = vi.fn((blob: Blob) => {
    void blob;
    return "blob:env";
  });
  Object.assign(URL, { createObjectURL, revokeObjectURL: vi.fn() });
  const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
  mount();
  await screen.findByTestId("gvar-SENTRY_DSN", {}, { timeout: 5000 });
  screen.getByRole("button", { name: "Exporter" }).click();
  await waitFor(() => expect(createObjectURL).toHaveBeenCalledTimes(1));
  expect(exports).toEqual([null]);
  const file = await createObjectURL.mock.calls[0][0].text();
  expect(file).toContain("SENTRY_DSN=");
  expect(file).not.toContain("RESEND_API_KEY");
  expect(await screen.findByText("5 variables exportées")).toBeInTheDocument();
  click.mockRestore();
});

it("import .env : une variable par ligne, tous projets et environnements", async () => {
  mount();
  await screen.findByTestId("gvar-SENTRY_DSN", {}, { timeout: 5000 });
  screen.getByRole("button", { name: "Importer .env" }).click();
  const dialog = await screen.findByRole("dialog", { name: "Importer un fichier .env" });
  fireEvent.change(within(dialog).getByLabelText("Contenu .env"), { target: { value: "A_KEY=1\n# note\nB_KEY=\"deux\"\n" } });
  within(dialog).getByRole("button", { name: "Importer 2" }).click();
  await waitFor(() => expect(posts).toHaveLength(2));
  expect(posts.map((p) => p.body)).toEqual([
    { key: "A_KEY", value: "1", secret: false, environments: ["production", "preview", "staging"], all_projects: true, projects: [] },
    { key: "B_KEY", value: "deux", secret: false, environments: ["production", "preview", "staging"], all_projects: true, projects: [] },
  ]);
});

it("développeur : réservé aux administrateurs, aucune requête", async () => {
  role = "developer";
  mount();
  expect(await screen.findByText("Réservé aux administrateurs.", {}, { timeout: 5000 })).toBeInTheDocument();
  expect(gets).toBe(0);
});

it("import .env en secrets : la liste explicite des projets, jamais « tous »", async () => {
  mount();
  await screen.findByTestId("gvar-SENTRY_DSN", {}, { timeout: 5000 });
  screen.getByRole("button", { name: "Importer .env" }).click();
  const dialog = await screen.findByRole("dialog", { name: "Importer un fichier .env" });
  fireEvent.change(within(dialog).getByLabelText("Contenu .env"), { target: { value: "A_KEY=1\n" } });
  within(dialog).getByRole("switch", { name: "Importer comme secrets" }).click();
  await waitFor(() => expect(within(dialog).getByRole("switch", { name: "Importer comme secrets" })).toBeChecked());
  within(dialog).getByRole("button", { name: "Importer 1" }).click();
  await waitFor(() => expect(posts).toHaveLength(1));
  expect(posts[0].body).toEqual({ key: "A_KEY", value: "1", secret: true, environments: ["production", "preview", "staging"], all_projects: false, projects: PROJECT_SUMMARIES.map((p) => p.id) });
});
