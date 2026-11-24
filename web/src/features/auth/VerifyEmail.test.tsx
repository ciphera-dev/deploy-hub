import { createMemoryHistory } from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it } from "vitest";

import { AppProviders, createAppQueryClient } from "@/app/providers";
import { createAppRouter } from "@/app/router";

const TOKEN = "testEfGhIjKlMnOpQrStUvWxYz0123456789-_abcde";

let lookupReply: () => Response;
let confirmReply: () => Response;
let confirmCalls: string[];

const server = setupServer(
  http.get("*/api/auth/email-verification/:token", () => lookupReply()),
  http.post("*/api/auth/email-verification/:token/confirm", ({ params }) => {
    confirmCalls.push(String(params.token));
    return confirmReply();
  }),
  http.get("*/api/auth/webauthn/status", () => HttpResponse.json({ available: false })),
);

const refuse = (status: number, code = "x") => () => HttpResponse.json({ error: { code, message: "detail" } }, { status });

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  lookupReply = () => HttpResponse.json({ purpose: "change", email: "alice@new.example.com", expires_at: 0 });
  confirmReply = () => new HttpResponse(null, { status: 204 });
  confirmCalls = [];
});
afterEach(() => server.resetHandlers());

function mount(url: string) {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [url] }));
  const client = createAppQueryClient(router);
  render(<AppProviders router={router} client={client} />);
  return { router };
}

const confirmButton = () => screen.getByRole("button", { name: "Confirmer l'adresse" });

it("changement d'adresse : l'ouverture du lien ne confirme rien, le bouton confirme", async () => {
  mount(`/verify-email/${TOKEN}`);
  expect(await screen.findByRole("heading", { name: "Confirmer votre adresse e-mail" })).toBeInTheDocument();
  expect(screen.getByText("alice@new.example.com")).toBeInTheDocument();
  expect(screen.getByText(/autres sessions ouvertes seront fermées/)).toBeInTheDocument();
  expect(confirmCalls).toEqual([]);
  fireEvent.click(confirmButton());
  expect(await screen.findByRole("heading", { name: "Adresse e-mail confirmée" })).toBeInTheDocument();
  expect(confirmCalls).toEqual([TOKEN]);
  expect(screen.getByText(/Votre compte utilise désormais alice@new.example.com/)).toBeInTheDocument();
});

it("inscription : le compte est active, puis connexion", async () => {
  lookupReply = () => HttpResponse.json({ purpose: "signup", email: "eve@example.com", expires_at: 0 });
  const { router } = mount(`/verify-email/${TOKEN}`);
  expect(await screen.findByText("eve@example.com")).toBeInTheDocument();
  fireEvent.click(confirmButton());
  expect(await screen.findByText("Votre compte est activé. Connectez-vous pour continuer.")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Se connecter" }));
  await waitFor(() => expect(router.state.location.pathname).toBe("/login"));
});

it("lien expire ou deja utilise : etat dedie, pas de bouton", async () => {
  lookupReply = refuse(410);
  mount(`/verify-email/${TOKEN}`);
  expect(await screen.findByRole("heading", { name: "Lien expiré" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Confirmer l'adresse" })).not.toBeInTheDocument();
});

it("lien deja utilise", async () => {
  lookupReply = refuse(404);
  mount(`/verify-email/${TOKEN}`);
  expect(await screen.findByRole("heading", { name: "Lien de vérification invalide" })).toBeInTheDocument();
});

it("adresse prise entre-temps : refus affiche", async () => {
  confirmReply = refuse(409, "email_taken");
  mount(`/verify-email/${TOKEN}`);
  await screen.findByRole("heading", { name: "Confirmer votre adresse e-mail" });
  fireEvent.click(confirmButton());
  expect(await screen.findByRole("heading", { name: "Adresse déjà utilisée" })).toBeInTheDocument();
});
