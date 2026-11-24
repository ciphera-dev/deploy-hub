import { createMemoryHistory } from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";

import { AppProviders, createAppQueryClient } from "@/app/providers";
import { createAppRouter } from "@/app/router";
import { setCsrfToken } from "@/lib/api/csrf";
import { PROJECT_SUMMARIES, projectDetail } from "@/mocks/fixtures/projects";
import { PROJECT_VARIABLES } from "@/mocks/fixtures/variables";

const WEB = PROJECT_SUMMARIES[0];
let role = "owner";
let puts: { env: string | null; body: unknown; csrf: string | null }[] = [];
let bulks: { env: string | null; body: unknown }[] = [];
let envPending = false;

const server = setupServer(
  http.get("*/api/me", () => HttpResponse.json({ id: "u1", name: "Alice Martin", email: "alice@example.com", role, csrf_token: "csrf-1" })),
  http.get("*/api/deployments", () => HttpResponse.json([])),
  http.get("*/api/projects/:id", ({ params }) => HttpResponse.json(projectDetail(PROJECT_SUMMARIES.find((p) => p.id === params.id)!))),
  http.get("*/api/projects/:id/variables/status", () => HttpResponse.json({ pending_apply: envPending })),
  http.get("*/api/projects/:id/variables", () => HttpResponse.json(PROJECT_VARIABLES)),
  http.put("*/api/projects/:id/variables", async ({ request }) => {
    puts.push({ env: new URL(request.url).searchParams.get("env"), body: await request.json(), csrf: request.headers.get("x-csrf-token") });
    return HttpResponse.json(PROJECT_VARIABLES);
  }),
  http.put("*/api/projects/:id/variables/bulk", async ({ request }) => {
    bulks.push({ env: new URL(request.url).searchParams.get("env"), body: await request.json() });
    return HttpResponse.json(PROJECT_VARIABLES);
  }),
);

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  setCsrfToken("csrf-1");
  role = "owner";
  puts = [];
  bulks = [];
  envPending = false;
});
afterEach(() => server.resetHandlers());

function mount(url: string) {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [url] }));
  render(<AppProviders router={router} client={createAppQueryClient(router)} />);
  return router;
}

it("variables du projet : secrets masqués, héritées des globales, bandeau « non appliquées »", async () => {
  mount(`/projects/${WEB.id}/variables`);
  const db = await screen.findByTestId("var-DATABASE_URL", {}, { timeout: 5000 });
  expect(within(db).getByText("••••••••••••")).toBeInTheDocument();
  // Héritées : section à part, jamais dans le tableau du projet.
  expect(screen.queryByTestId("var-SENTRY_DSN")).not.toBeInTheDocument();
  expect(screen.getByTestId("global-SENTRY_DSN")).toHaveTextContent("https://8f2c@o4501.ingest.sentry.io/77");
  expect(screen.getByTestId("global-RESEND_API_KEY")).toHaveTextContent("••••••••••••");
  expect(screen.getByText("Héritées des variables globales")).toBeInTheDocument();
  // Une variable modifiée depuis le dernier déploiement (LOG_LEVEL).
  expect(within(screen.getByTestId("var-LOG_LEVEL")).getByLabelText("Non appliquée")).toBeInTheDocument();
  const banner = screen.getByRole("status");
  expect(banner).toHaveTextContent("1 modification non appliquée.");
  expect(within(banner).getByRole("button", { name: "Enregistrer et redéployer" })).toBeInTheDocument();
  within(banner).getByRole("button", { name: "Annuler" }).click();
  await waitFor(() => expect(screen.queryByRole("status")).not.toBeInTheDocument());
});

it("ajout d'un secret : PUT avec le jeton CSRF, la valeur ne revient jamais", async () => {
  mount(`/projects/${WEB.id}/variables`);
  await screen.findByTestId("var-PORT", {}, { timeout: 5000 });
  screen.getByRole("button", { name: "Ajouter" }).click();
  fireEvent.change(await screen.findByLabelText("Clé"), { target: { value: "stripe key" } });
  fireEvent.change(screen.getByLabelText("Valeur"), { target: { value: "sk_live_x" } });
  screen.getByRole("button", { name: "Secret" }).click();
  await waitFor(() => expect(screen.getByLabelText("Valeur")).toHaveAttribute("type", "password"));
  screen.getByRole("button", { name: "Valider" }).click();
  await waitFor(() => expect(puts).toHaveLength(1));
  expect(puts[0]).toEqual({ env: "production", body: { key: "STRIPE_KEY", value: "sk_live_x", secret: true, previous_key: null }, csrf: "csrf-1" });
});

it("secret trop court : enregistré, avertissement « non masqué dans les journaux »", async () => {
  server.use(
    http.put("*/api/projects/:id/variables", async ({ request }) => {
      puts.push({ env: new URL(request.url).searchParams.get("env"), body: await request.json(), csrf: request.headers.get("x-csrf-token") });
      return HttpResponse.json([...PROJECT_VARIABLES, { key: "PIN", value: null, secret: true, inherited: false, pending_apply: true, updated_at: "2026-11-24T10:00:00.000Z", updated_by: "Alice Martin", warnings: ["too_short_to_redact"] }]);
    }),
  );
  mount(`/projects/${WEB.id}/variables`);
  await screen.findByTestId("var-PORT", {}, { timeout: 5000 });
  screen.getByRole("button", { name: "Ajouter" }).click();
  fireEvent.change(await screen.findByLabelText("Clé"), { target: { value: "PIN" } });
  fireEvent.change(screen.getByLabelText("Valeur"), { target: { value: "123" } });
  screen.getByRole("button", { name: "Secret" }).click();
  await waitFor(() => expect(screen.getByLabelText("Valeur")).toHaveAttribute("type", "password"));
  screen.getByRole("button", { name: "Valider" }).click();
  await waitFor(() => expect(puts).toHaveLength(1));
  expect(await screen.findByText("PIN : secret de moins de 4 caractères, enregistré mais non masqué dans les journaux.")).toBeInTheDocument();
});

it("secret modifié : « Laisser vide pour conserver », valeur vide envoyée", async () => {
  mount(`/projects/${WEB.id}/variables`);
  const row = await screen.findByTestId("var-STRIPE_SECRET_KEY", {}, { timeout: 5000 });
  within(row).getByRole("button", { name: "Modifier" }).click();
  const value = await screen.findByPlaceholderText("Laisser vide pour conserver");
  expect(value).toHaveValue("");
  screen.getByRole("button", { name: "Valider" }).click();
  await waitFor(() => expect(puts).toHaveLength(1));
  expect(puts[0].body).toEqual({ key: "STRIPE_SECRET_KEY", value: "", secret: true, previous_key: null });
});

it("mode .env : secrets sans valeur, enregistrement en un envoi ; téléchargement sans secrets", async () => {
  mount(`/projects/${WEB.id}/variables`);
  await screen.findByTestId("var-PORT", {}, { timeout: 5000 });
  screen.getByRole("button", { name: ".env" }).click();
  const area = await screen.findByLabelText("Contenu .env");
  const text = (area as HTMLTextAreaElement).value;
  expect(text).toContain("DATABASE_URL=  # secret");
  expect(text).toContain("NEXT_PUBLIC_API_URL=https://app.ciphera.dev/api");
  expect(text).not.toContain("SENTRY_DSN");
  fireEvent.change(area, { target: { value: text + "NEW_FLAG=on\n" } });
  screen.getByRole("button", { name: "Enregistrer" }).click();
  await waitFor(() => expect(bulks).toHaveLength(1));
  expect(bulks[0].env).toBe("production");
  expect((bulks[0].body as { content: string }).content).toContain("NEW_FLAG=on");

  const createObjectURL = vi.fn((blob: Blob) => {
    void blob;
    return "blob:env";
  });
  Object.assign(URL, { createObjectURL, revokeObjectURL: vi.fn() });
  const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
  screen.getByRole("button", { name: "Télécharger .env" }).click();
  const file = await createObjectURL.mock.calls[0][0].text();
  expect(file).toContain("PORT=3000");
  expect(file).not.toContain("DATABASE_URL");
  expect(file).not.toContain("STRIPE_SECRET_KEY");
  click.mockRestore();
});

it("ops : lecture seule (ni ajout, ni modification)", async () => {
  role = "ops";
  mount(`/projects/${WEB.id}/variables`);
  const row = await screen.findByTestId("var-PORT", {}, { timeout: 5000 });
  expect(screen.getByRole("button", { name: "Ajouter" })).toBeDisabled();
  expect(within(row).queryByRole("button", { name: "Modifier" })).not.toBeInTheDocument();
  expect(within(row).queryByRole("button", { name: "Supprimer" })).not.toBeInTheDocument();
});

it("lecteur : refus du serveur, écran « Accès refusé » sans aucune variable", async () => {
  role = "viewer";
  server.use(http.get("*/api/projects/:id/variables", () => HttpResponse.json({ error: { code: "forbidden", message: "Accès refusé." } }, { status: 403 })));
  mount(`/projects/${WEB.id}/variables`);
  expect(await screen.findByText("Accès refusé", {}, { timeout: 5000 })).toBeInTheDocument();
  expect(screen.queryByTestId("var-PORT")).not.toBeInTheDocument();
});

it("suppression seule : le bandeau « non appliquées » s'affiche aussi", async () => {
  envPending = true;
  const applied = PROJECT_VARIABLES.map((v) => ({ ...v, pending_apply: false }));
  server.use(http.get("*/api/projects/:id/variables", () => HttpResponse.json(applied)));
  mount(`/projects/${WEB.id}/variables`);
  await screen.findByTestId("var-PORT", {}, { timeout: 5000 });
  const banner = await screen.findByRole("status");
  expect(banner).toHaveTextContent("Suppressions non appliquées.");
  expect(within(banner).getByRole("button", { name: "Enregistrer et redéployer" })).toBeInTheDocument();
});

it("rien à appliquer : pas de bandeau", async () => {
  const applied = PROJECT_VARIABLES.map((v) => ({ ...v, pending_apply: false }));
  server.use(http.get("*/api/projects/:id/variables", () => HttpResponse.json(applied)));
  mount(`/projects/${WEB.id}/variables`);
  await screen.findByTestId("var-PORT", {}, { timeout: 5000 });
  await new Promise((r) => setTimeout(r, 50));
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
});
