import { createMemoryHistory } from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it } from "vitest";

import { AppProviders, createAppQueryClient } from "@/app/providers";
import { createAppRouter } from "@/app/router";

const TOKEN = "testEfGhIjKlMnOpQrStUvWxYz0123456789-_abcde";
const PASSWORD = "une phrase de passe assez longue";

let lookupReply: () => Response;
let resetReply: () => Response;
let resetCalls: unknown[];
let statusReply: boolean;
let forgotCalls: unknown[];
let forgotReply: () => Response;

const server = setupServer(
  http.get("*/api/auth/password/reset/:token", () => lookupReply()),
  http.post("*/api/auth/password/reset", async ({ request }) => {
    resetCalls.push(await request.json());
    return resetReply();
  }),
  http.get("*/api/auth/password/forgot", () => HttpResponse.json({ email_available: statusReply })),
  http.post("*/api/auth/password/forgot", async ({ request }) => {
    forgotCalls.push(await request.json());
    return forgotReply();
  }),
  http.get("*/api/auth/webauthn/status", () => HttpResponse.json({ available: false })),
);

const refuse = (status: number, message = "detail", headers: Record<string, string> = {}) => () => HttpResponse.json({ error: { code: "x", message } }, { status, headers });

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  lookupReply = () => HttpResponse.json({ expires_at: 0 });
  resetReply = () => new HttpResponse(null, { status: 204 });
  resetCalls = [];
  statusReply = true;
  forgotCalls = [];
  forgotReply = () => new HttpResponse(null, { status: 202 });
});
afterEach(() => server.resetHandlers());

function mount(url: string) {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [url] }));
  const client = createAppQueryClient(router);
  render(<AppProviders router={router} client={client} />);
  return { router };
}

const type = (label: string, value: string) => fireEvent.change(screen.getByLabelText(label), { target: { value } });
const saveButton = () => screen.getByRole("button", { name: "Enregistrer le mot de passe" });

it("lien valable : nouveau mot de passe, puis connexion avec le bandeau « mot de passe modifié »", async () => {
  const { router } = mount(`/reset/${TOKEN}`);
  expect(await screen.findByRole("heading", { name: "Choisir un nouveau mot de passe" })).toBeInTheDocument();
  expect(saveButton()).toBeDisabled();
  type("Nouveau mot de passe", "trop court");
  type("Confirmer le mot de passe", "trop court");
  expect(saveButton()).toBeDisabled();
  type("Nouveau mot de passe", PASSWORD);
  type("Confirmer le mot de passe", PASSWORD + "x");
  expect(saveButton()).toBeDisabled();
  expect(screen.getByText("Les deux mots de passe ne correspondent pas.")).toBeInTheDocument();
  type("Confirmer le mot de passe", PASSWORD);
  fireEvent.click(saveButton());

  await waitFor(() => expect(router.state.location.pathname).toBe("/login"));
  expect(router.state.location.search).toEqual({ reset: 1 });
  expect(resetCalls).toEqual([{ token: TOKEN, password: PASSWORD }]);
  expect(await screen.findByRole("heading", { name: "Mot de passe modifié" })).toBeInTheDocument();
});

it("lien expire : etat dedie, pas de formulaire", async () => {
  lookupReply = refuse(410);
  mount(`/reset/${TOKEN}`);
  expect(await screen.findByRole("heading", { name: "Lien expiré" })).toBeInTheDocument();
  expect(screen.queryByLabelText("Nouveau mot de passe")).not.toBeInTheDocument();
});

it("lien inconnu ou deja utilise : etat « invalide », retour a la connexion", async () => {
  lookupReply = refuse(404);
  const { router } = mount(`/reset/${TOKEN}`);
  expect(await screen.findByRole("heading", { name: "Lien de réinitialisation invalide" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Se connecter" }));
  await waitFor(() => expect(router.state.location.pathname).toBe("/login"));
});

it("refus a l'enregistrement : message affiche, on reste sur l'ecran", async () => {
  const { router } = mount(`/reset/${TOKEN}`);
  await screen.findByRole("heading", { name: "Choisir un nouveau mot de passe" });
  type("Nouveau mot de passe", PASSWORD);
  type("Confirmer le mot de passe", PASSWORD);

  resetReply = refuse(410);
  fireEvent.click(saveButton());
  expect(await screen.findByRole("alert")).toHaveTextContent("Ce lien a expiré.");

  resetReply = refuse(422, "Le mot de passe doit contenir au moins 12 caractères.");
  fireEvent.click(saveButton());
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Le mot de passe doit contenir au moins 12 caractères."));

  resetReply = refuse(429, "x", { "Retry-After": "120" });
  fireEvent.click(saveButton());
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Réessayez dans 2 minutes."));
  expect(router.state.location.pathname).toBe("/reset");
});

it("mot de passe oublie sans envoi possible : « contactez un administrateur »", async () => {
  statusReply = false;
  mount("/login");
  fireEvent.click(await screen.findByRole("button", { name: "mot de passe" }));
  expect(await screen.findByText(/contactez un administrateur/)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Envoyer le lien" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Retour à la connexion" }));
  expect(await screen.findByRole("heading", { name: "Connectez-vous à Ciphera" })).toBeInTheDocument();
});

it("mot de passe oublie : demande envoyee, meme message que le compte existe ou non", async () => {
  mount("/login");
  fireEvent.click(await screen.findByRole("button", { name: "mot de passe" }));
  const send = await screen.findByRole("button", { name: "Envoyer le lien" });
  expect(send).toBeDisabled();
  type("E-mail", "nora@example.com");
  fireEvent.click(send);
  expect(await screen.findByRole("heading", { name: "Vérifiez votre boîte mail" })).toBeInTheDocument();
  expect(screen.getByText(/Si un compte existe pour nora@example.com, un lien valable 30 minutes/)).toBeInTheDocument();
  expect(forgotCalls).toEqual([{ email: "nora@example.com" }]);
});

it("mot de passe oublie : trop de demandes", async () => {
  forgotReply = refuse(429, "x", { "Retry-After": "600" });
  mount("/login");
  fireEvent.click(await screen.findByRole("button", { name: "mot de passe" }));
  await screen.findByRole("button", { name: "Envoyer le lien" });
  type("E-mail", "nora@example.com");
  fireEvent.click(screen.getByRole("button", { name: "Envoyer le lien" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Réessayez dans 10 minutes.");
});
