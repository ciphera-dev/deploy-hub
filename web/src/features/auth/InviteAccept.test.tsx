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
let acceptReply: () => Response;
let acceptCalls: { token: string; body: unknown }[];
let signupOpen: boolean;
let registerCalls: unknown[];

const server = setupServer(
  http.get("*/api/auth/invitations/:token", () => lookupReply()),
  http.post("*/api/auth/invitations/:token/accept", async ({ params, request }) => {
    acceptCalls.push({ token: String(params.token), body: await request.json() });
    return acceptReply();
  }),
  http.get("*/api/auth/register", () => HttpResponse.json({ public_signup: signupOpen })),
  http.post("*/api/auth/register", async ({ request }) => {
    registerCalls.push(await request.json());
    return new HttpResponse(null, { status: 202 });
  }),
  http.get("*/api/auth/webauthn/status", () => HttpResponse.json({ available: false })),
);

const refuse = (status: number, message = "detail") => () => HttpResponse.json({ error: { code: "x", message } }, { status });

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  lookupReply = () => HttpResponse.json({ email: "nora@example.com", name: "Nora", role: "developer", expires_at: 0 });
  acceptReply = () => new HttpResponse(null, { status: 204 });
  acceptCalls = [];
  signupOpen = false;
  registerCalls = [];
});
afterEach(() => server.resetHandlers());

function mount(url: string) {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [url] }));
  const client = createAppQueryClient(router);
  render(<AppProviders router={router} client={client} />);
  return { router };
}

const type = (label: string, value: string) => fireEvent.change(screen.getByLabelText(label), { target: { value } });
const createButton = () => screen.getByRole("button", { name: "Créer le compte" });

it("invitation valable : compte cree, puis connexion avec le bandeau « compte créé »", async () => {
  const { router } = mount(`/invite/${TOKEN}`);
  expect(await screen.findByRole("heading", { name: "Créer votre compte" })).toBeInTheDocument();
  expect(screen.getByText("nora@example.com")).toBeInTheDocument();
  expect(screen.getByRole("list", { name: "Étapes de l'inscription" })).toBeInTheDocument();
  expect(screen.getByText(/Créez votre compte en trois étapes/)).toBeInTheDocument();
  expect(screen.queryByText(/Connectez-vous en trois étapes/)).not.toBeInTheDocument();
  expect(screen.getByText(/rôle Développeur/)).toBeInTheDocument();
  expect(screen.queryByLabelText("E-mail")).not.toBeInTheDocument();
  expect(screen.getByLabelText("Nom complet")).toHaveValue("Nora");

  type("Mot de passe", "trop court");
  expect(createButton()).toBeDisabled();
  type("Nom complet", "Nora Kaci");
  type("Mot de passe", PASSWORD);
  fireEvent.click(createButton());

  await waitFor(() => expect(router.state.location.pathname).toBe("/login"));
  expect(router.state.location.search).toEqual({ created: 1 });
  expect(acceptCalls).toEqual([{ token: TOKEN, body: { name: "Nora Kaci", password: PASSWORD } }]);
  expect(await screen.findByRole("heading", { name: "Compte créé" })).toBeInTheDocument();
});

it("invitation expiree : etat dedie, pas de formulaire", async () => {
  lookupReply = refuse(410);
  mount(`/invite/${TOKEN}`);
  expect(await screen.findByRole("heading", { name: "Invitation expirée" })).toBeInTheDocument();
  expect(screen.queryByLabelText("Mot de passe")).not.toBeInTheDocument();
});

it("lien inconnu ou deja utilise : etat « invalide », retour a la connexion", async () => {
  lookupReply = refuse(404);
  const { router } = mount(`/invite/${TOKEN}`);
  expect(await screen.findByRole("heading", { name: "Lien d'invitation invalide" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Se connecter" }));
  await waitFor(() => expect(router.state.location.pathname).toBe("/login"));
});

it("refus a l'acceptation : message affiche, on reste sur l'ecran", async () => {
  const { router } = mount(`/invite/${TOKEN}`);
  await screen.findByRole("heading", { name: "Créer votre compte" });
  type("Mot de passe", PASSWORD);

  acceptReply = refuse(409);
  fireEvent.click(createButton());
  expect(await screen.findByRole("alert")).toHaveTextContent("Un compte existe déjà pour cet e-mail.");

  acceptReply = refuse(422, "Nom : 1 à 200 caractères.");
  fireEvent.click(createButton());
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Nom : 1 à 200 caractères."));

  acceptReply = refuse(410);
  fireEvent.click(createButton());
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Ce lien d'invitation a expiré."));
  expect(router.state.location.pathname).toBe(`/invite/${TOKEN}`);
});

it("inscription publique fermee (defaut) : « Inscription sur invitation »", async () => {
  mount("/register");
  expect(await screen.findByRole("heading", { name: "Inscription sur invitation" })).toBeInTheDocument();
  expect(screen.getByRole("list", { name: "Étapes de l'inscription" })).toBeInTheDocument();
  expect(screen.queryByRole("list", { name: "Étapes de connexion" })).not.toBeInTheDocument();
  expect(screen.queryByText(/Connectez-vous en trois étapes/)).not.toBeInTheDocument();
  expect(screen.queryByLabelText("Mot de passe")).not.toBeInTheDocument();
});

it("inscription publique ouverte : formulaire de register.html, puis connexion avec le bandeau « vérifiez votre boîte mail »", async () => {
  signupOpen = true;
  const { router } = mount("/register");
  expect(await screen.findByRole("heading", { name: "Créer un compte" })).toBeInTheDocument();
  expect(screen.getByRole("list", { name: "Étapes de l'inscription" })).toBeInTheDocument();
  type("Nom complet", "Eve");
  type("E-mail", "eve@example.com");
  type("Mot de passe", PASSWORD);
  fireEvent.click(createButton());
  await waitFor(() => expect(router.state.location.pathname).toBe("/login"));
  expect(registerCalls).toEqual([{ name: "Eve", email: "eve@example.com", password: PASSWORD }]);
  expect(router.state.location.search).toEqual({ verify: 1 });
  expect(await screen.findByRole("heading", { name: "Vérifiez votre boîte mail" })).toBeInTheDocument();
});
