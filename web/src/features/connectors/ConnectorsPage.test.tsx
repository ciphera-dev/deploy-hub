import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";

import { ToastProvider } from "@/design-system";
import { setCsrfToken } from "@/lib/api/csrf";
import { GITHUB_CONNECTED, GITHUB_DISCONNECTED } from "@/mocks/fixtures/github";

import type { GithubStatus } from "./api";
import { ConnectorsPage } from "./ConnectorsPage";

let status: GithubStatus;
let manifestPosts: { csrf: string | null; body: unknown }[];
let callbacks: URLSearchParams[];
let callbackReply: () => Response;

const server = setupServer(
  http.get("*/api/connectors/github", () => HttpResponse.json(status)),
  http.post("*/api/connectors/github/manifest", async ({ request }) => {
    manifestPosts.push({ csrf: request.headers.get("x-csrf-token"), body: await request.json() });
    return HttpResponse.json({ manifest: { name: "Ciphera ab12cd", default_events: ["push"] }, post_url: "https://github.com/settings/apps/new?state=st4te" });
  }),
  http.get("*/api/connectors/github/callback", ({ request }) => {
    callbacks.push(new URL(request.url).searchParams);
    return callbackReply();
  }),
);

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  status = GITHUB_DISCONNECTED;
  manifestPosts = [];
  callbacks = [];
  callbackReply = () => HttpResponse.json(GITHUB_CONNECTED);
  setCsrfToken("csrf-1");
  localStorage.clear();
});
afterEach(() => {
  server.resetHandlers();
  vi.restoreAllMocks();
});

function renderPage(search: { code?: string; state?: string; installation_id?: string } = {}) {
  const clearSearch = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ToastProvider>
        <ConnectorsPage search={search} clearSearch={clearSearch} />
      </ToastProvider>
    </QueryClientProvider>,
  );
  return { clearSearch };
}

it("seule la carte GitHub est active ; les autres portent leur version", async () => {
  renderPage();
  expect(await screen.findByText("Aucun connecteur actif.")).toBeInTheDocument();
  const githubCard = screen.getByTestId("connector-github");
  expect(within(githubCard).getByRole("button", { name: "Connecter" })).toBeEnabled();
  const cloudflare = screen.getByTestId("connector-cloudflare");
  expect(within(cloudflare).getByText("v0.2")).toBeInTheDocument();
  expect(within(cloudflare).getByRole("button", { name: "Connecter" })).toBeDisabled();
  expect(within(screen.getByTestId("connector-claude")).getByText("v0.3")).toBeInTheDocument();
});

it("Connecter GitHub : demande le manifest (CSRF) puis soumet le formulaire GitHub", async () => {
  const submit = vi.spyOn(HTMLFormElement.prototype, "submit").mockImplementation(() => undefined);
  renderPage();
  fireEvent.click(within(await screen.findByTestId("connector-github")).getByRole("button", { name: "Connecter" }));
  fireEvent.click(screen.getByRole("button", { name: "Autoriser sur GitHub" }));
  await waitFor(() => expect(submit).toHaveBeenCalledTimes(1));
  expect(manifestPosts).toEqual([{ csrf: "csrf-1", body: { panel_url: window.location.origin } }]);
  const form = submit.mock.contexts[0] as HTMLFormElement;
  expect(form.method).toBe("post");
  expect(form.action).toBe("https://github.com/settings/apps/new?state=st4te");
  const field = form.querySelector("input[name=manifest]") as HTMLInputElement;
  expect(JSON.parse(field.value)).toEqual({ name: "Ciphera ab12cd", default_events: ["push"] });
  expect(localStorage.length).toBe(0);
});

it("retour de GitHub : convertit le code une fois et affiche l'App connectée", async () => {
  const { clearSearch } = renderPage({ code: "c0de", state: "st4te" });
  await waitFor(() => expect(clearSearch).toHaveBeenCalled());
  expect(callbacks).toHaveLength(1);
  expect(callbacks[0].get("code")).toBe("c0de");
  expect(callbacks[0].get("state")).toBe("st4te");
  expect(await screen.findByText("Connectés")).toBeInTheDocument();
  expect(screen.getByText("acme")).toBeInTheDocument();
  expect(screen.queryByTestId("connector-github")).not.toBeInTheDocument();
  expect(localStorage.length).toBe(0);
});

it("retour refusé (state invalide) : le message s'affiche", async () => {
  callbackReply = () => HttpResponse.json({ error: { code: "invalid_state", message: "Lien de retour GitHub invalide ou expiré : recommencez la connexion." } }, { status: 400 });
  renderPage({ code: "c0de", state: "bad" });
  expect(await screen.findByText("Lien de retour GitHub invalide ou expiré : recommencez la connexion.")).toBeInTheDocument();
});

it("App connectée : ligne « Connectés » avec le lien d'installation", async () => {
  status = GITHUB_CONNECTED;
  renderPage();
  expect(await screen.findByText("Connectés")).toBeInTheDocument();
  expect(screen.getByRole("link", { name: "Installer" })).toHaveAttribute("href", "https://github.com/apps/ciphera-ab12cd/installations/new");
});

it("déconnexion : le lien vers les réglages de l'App sur GitHub et « supprimez-la aussi sur GitHub » s'affichent", async () => {
  status = GITHUB_CONNECTED;
  server.use(
    http.delete("*/api/connectors/github", () => {
      status = GITHUB_DISCONNECTED;
      return new HttpResponse(null, { status: 204 });
    }),
  );
  renderPage();
  fireEvent.click(await screen.findByRole("button", { name: "Déconnecter" }));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText(/supprimez-la aussi sur GitHub/i)).toBeInTheDocument();
  fireEvent.click(within(dialog).getByRole("button", { name: "Déconnecter" }));
  expect(await screen.findByText(/supprimez-la aussi sur GitHub\./)).toBeInTheDocument();
  expect(screen.getByRole("link", { name: "Ouvrir les réglages de l'App" })).toHaveAttribute("href", GITHUB_CONNECTED.app!.html_url);
});
