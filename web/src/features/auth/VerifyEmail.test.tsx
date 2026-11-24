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
let confirmBodies: unknown[];

const server = setupServer(
  http.get("*/api/auth/email-verification/:token", () => lookupReply()),
  http.post("*/api/auth/email-verification/:token/confirm", async ({ params, request }) => {
    confirmCalls.push(String(params.token));
    confirmBodies.push(request.headers.get("content-type") ? await request.json() : null);
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
  confirmBodies = [];
});
afterEach(() => server.resetHandlers());

function mount(url: string) {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [url] }));
  const client = createAppQueryClient(router);
  render(<AppProviders router={router} client={client} />);
  return { router };
}

const confirmButton = () => screen.getByRole("button", { name: "Confirmer l'adresse" });
const PASSWORD = "une phrase de passe assez longue";
const type = (label: string, value: string) => fireEvent.change(screen.getByLabelText(label), { target: { value } });

it("changement d'adresse : l'ouverture du lien ne confirme rien, le bouton confirme", async () => {
  mount(`/verify-email/${TOKEN}`);
  expect(await screen.findByRole("heading", { name: "Confirmer votre adresse e-mail" })).toBeInTheDocument();
  expect(screen.getByText("alice@new.example.com")).toBeInTheDocument();
  expect(screen.getByText(/autres sessions ouvertes seront fermées/)).toBeInTheDocument();
  expect(confirmCalls).toEqual([]);
  fireEvent.click(confirmButton());
  expect(await screen.findByRole("heading", { name: "Adresse e-mail confirmée" })).toBeInTheDocument();
  expect(confirmCalls).toEqual([TOKEN]);
  expect(confirmBodies).toEqual([null]);
  expect(screen.getByText(/Votre compte utilise désormais alice@new.example.com/)).toBeInTheDocument();
  expect(screen.queryByText(/connectez-vous avec cette adresse/)).not.toBeInTheDocument();
});

it("inscription : le compte est active, puis connexion", async () => {
  lookupReply = () => HttpResponse.json({ purpose: "signup", email: "eve@example.com", expires_at: 0 });
  const { router } = mount(`/verify-email/${TOKEN}`);
  expect(await screen.findByText("eve@example.com")).toBeInTheDocument();
  expect(screen.getByText(/n'êtes pas à l'origine de cette inscription/)).toBeInTheDocument();
  const activate = () => screen.getByRole("button", { name: "Activer le compte" });
  expect(activate()).toBeDisabled();
  type("Mot de passe", "trop court");
  type("Confirmer le mot de passe", "trop court");
  expect(activate()).toBeDisabled();
  type("Mot de passe", PASSWORD);
  type("Confirmer le mot de passe", PASSWORD + "x");
  expect(activate()).toBeDisabled();
  expect(screen.getByText("Les deux mots de passe ne correspondent pas.")).toBeInTheDocument();
  type("Confirmer le mot de passe", PASSWORD);
  fireEvent.click(activate());
  expect(await screen.findByText("Votre compte est activé avec le mot de passe choisi. Connectez-vous pour continuer.")).toBeInTheDocument();
  expect(confirmBodies).toEqual([{ password: PASSWORD }]);
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
