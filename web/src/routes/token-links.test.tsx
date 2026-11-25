import { createBrowserHistory } from "@tanstack/react-router";
import { render, screen } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it } from "vitest";

import { AppProviders, createAppQueryClient } from "@/app/providers";
import { createAppRouter } from "@/app/router";
import { clearHeldTokens } from "@/lib/url-token";

const TOKEN = "testEfGhIjKlMnOpQrStUvWxYz0123456789-_abcde";

const server = setupServer(
  http.get("*/api/auth/password/reset/:token", () => HttpResponse.json({ expires_at: 0 })),
  http.get("*/api/auth/invitations/:token", () => HttpResponse.json({ email: "a@b.org", organization: "Org", role: "member", expires_at: 0 })),
  http.get("*/api/auth/register", () => HttpResponse.json({ public_signup: false })),
  http.get("*/api/auth/webauthn/status", () => HttpResponse.json({ available: false })),
);

beforeAll(() => server.listen({ onUnhandledFrame: "bypass" }));
afterAll(() => server.close());
afterEach(() => server.resetHandlers());
beforeEach(() => clearHeldTokens());

/** Historique du navigateur (jsdom) : le routeur remplace window.history, comme en production. */
function mount(url: string) {
  window.history.replaceState(null, "", url);
  const router = createAppRouter(createBrowserHistory());
  const client = createAppQueryClient(router);
  render(<AppProviders router={router} client={client} />);
  return router;
}

it("token_removed_from_url : la page de réinitialisation reste affichée, sans jeton dans l'URL", async () => {
  const router = mount(`/reset/${TOKEN}`);
  expect(await screen.findByRole("heading", { name: "Choisir un nouveau mot de passe" })).toBeInTheDocument();
  expect(window.location.pathname).toBe("/reset");
  expect(window.location.href).not.toContain(TOKEN);
  expect(router.state.location.pathname).toBe("/reset");
});

it("la page d'invitation reste affichée, sans jeton dans l'URL", async () => {
  mount(`/invite/${TOKEN}`);
  expect(await screen.findByText("Org", { exact: false })).toBeInTheDocument();
  expect(window.location.pathname).toBe("/invite");
  expect(window.location.href).not.toContain(TOKEN);
});

it("la page de vérification d'adresse reste sur place, sans jeton dans l'URL", async () => {
  mount(`/verify-email/${TOKEN}`);
  await screen.findByRole("heading");
  expect(window.location.pathname).toBe("/verify-email");
  expect(window.location.href).not.toContain(TOKEN);
});

it("rechargement sans jeton en mémoire : « Lien expiré ou déjà utilisé »", async () => {
  mount("/reset");
  expect(await screen.findByRole("heading", { name: "Lien expiré ou déjà utilisé" })).toBeInTheDocument();
});
