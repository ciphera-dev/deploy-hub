import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";

import { getCsrfToken, setCsrfToken } from "@/lib/api/csrf";

import { SecurityKeyCard } from "./SecurityKeyCard";

const KEY = { id: "k1", name: "YubiKey bureau", created_at: 1_763_700_000, last_used_at: null };

let keys: (typeof KEY)[];
let available: boolean;
let optionBodies: unknown[];
let verifies: { csrf: string | null; body: unknown }[];
let verifyReply: () => Response;
let deletes: { id: string; csrf: string | null; body: unknown }[];
let deleteReply: () => Response;

const server = setupServer(
  http.get("*/api/me/webauthn", () => HttpResponse.json({ credentials: keys })),
  http.get("*/api/auth/webauthn/status", () => HttpResponse.json({ available })),
  http.get("*/api/me", () => HttpResponse.json({})),
  http.post("*/api/me/webauthn/register/options", async ({ request }) => {
    optionBodies.push(await request.json().catch(() => null));
    return HttpResponse.json({
      ceremony_id: "r-1",
      options: { publicKey: { challenge: "AQID", rp: { id: "localhost", name: "Ciphera" }, user: { id: "BwgJ", name: "a@b.c", displayName: "A" }, pubKeyCredParams: [], excludeCredentials: [{ type: "public-key", id: "BAUG" }] } },
    });
  }),
  http.post("*/api/me/webauthn/register/verify", async ({ request }) => {
    verifies.push({ csrf: request.headers.get("x-csrf-token"), body: await request.json() });
    return verifyReply();
  }),
  http.delete("*/api/me/webauthn/:id", async ({ request, params }) => {
    deletes.push({ id: params.id as string, csrf: request.headers.get("x-csrf-token"), body: await request.json() });
    return deleteReply();
  }),
);

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  keys = [];
  available = true;
  optionBodies = [];
  verifies = [];
  verifyReply = () => HttpResponse.json({ id: "k2", csrf_token: "csrf-2", recovery_codes: [] });
  deletes = [];
  deleteReply = () => new HttpResponse(null, { status: 204 });
  setCsrfToken("csrf-1");
});
afterEach(() => {
  server.resetHandlers();
  vi.unstubAllGlobals();
  Reflect.deleteProperty(navigator, "credentials");
});

const bytes = (...b: number[]) => new Uint8Array(b).buffer;

function stubCreate(error?: DOMException) {
  const create = vi.fn(async () => {
    if (error) throw error;
    return {
      id: "CgsM",
      rawId: bytes(10, 11, 12),
      type: "public-key",
      response: { attestationObject: bytes(1), clientDataJSON: bytes(2), getTransports: () => ["usb"] },
      getClientExtensionResults: () => ({}),
    };
  });
  vi.stubGlobal("PublicKeyCredential", function PublicKeyCredential() {});
  Object.defineProperty(navigator, "credentials", { value: { create, get: vi.fn() }, configurable: true });
  return create;
}

function typePassword(value = "mon mot de passe") {
  fireEvent.change(screen.getByLabelText("Mot de passe"), { target: { value } });
}

function renderCard() {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <SecurityKeyCard />
    </QueryClientProvider>,
  );
}

it("sans cle : inactive, ajout par le navigateur puis liste relue", async () => {
  const create = stubCreate();
  renderCard();
  expect(await screen.findByText("YubiKey, Touch ID · aucune")).toBeInTheDocument();
  expect(screen.getByText("Inactive")).toBeInTheDocument();
  fireEvent.click(await screen.findByRole("button", { name: "Ajouter" }));
  fireEvent.change(screen.getByLabelText("Nom de la clé"), { target: { value: "YubiKey bureau" } });
  keys = [KEY];
  typePassword();
  fireEvent.click(screen.getByRole("button", { name: "Continuer" }));
  expect(await screen.findByText("YubiKey, Touch ID · 1 clé")).toBeInTheDocument();
  expect(screen.getByText("Active")).toBeInTheDocument();
  const publicKey = create.mock.calls[0] as unknown as [CredentialCreationOptions];
  expect(new Uint8Array(publicKey[0].publicKey!.excludeCredentials![0].id as ArrayBuffer)).toEqual(new Uint8Array([4, 5, 6]));
  expect(optionBodies).toEqual([{ current_password: "mon mot de passe" }]);
  expect(verifies).toEqual([
    {
      csrf: "csrf-1",
      body: {
        ceremony_id: "r-1",
        name: "YubiKey bureau",
        credential: { id: "CgsM", rawId: "CgsM", type: "public-key", response: { attestationObject: "AQ", clientDataJSON: "Ag", transports: ["usb"] }, extensions: {} },
      },
    },
  ]);
  expect(getCsrfToken()).toBe("csrf-2");
  // Le compte avait deja ses codes : la fenetre se ferme sans en afficher.
  expect(screen.queryByText("Codes de secours")).not.toBeInTheDocument();
});

it("premiere cle d'un compte sans codes : les codes de secours sont affiches une fois", async () => {
  stubCreate();
  const codes = Array.from({ length: 10 }, (_, i) => `ABCD${String.fromCharCode(65 + i)}-EFGHJ`);
  verifyReply = () => HttpResponse.json({ id: "k2", csrf_token: "csrf-2", recovery_codes: codes });
  renderCard();
  fireEvent.click(await screen.findByRole("button", { name: "Ajouter" }));
  keys = [KEY];
  typePassword();
  fireEvent.click(screen.getByRole("button", { name: "Continuer" }));
  expect(await screen.findByText("Codes de secours")).toBeInTheDocument();
  expect(screen.getByText(/plus accès à votre clé de sécurité/)).toBeInTheDocument();
  for (const code of codes) expect(screen.getByText(code, { exact: false })).toBeInTheDocument();
  // La liste est deja relue derriere la fenetre des codes.
  expect(await screen.findByText("YubiKey, Touch ID · 1 clé")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "J'ai enregistré mes codes" }));
  await waitFor(() => expect(screen.queryByText("Codes de secours")).not.toBeInTheDocument());
  expect(screen.queryByText(codes[0], { exact: false })).not.toBeInTheDocument();
});

it("ajout annule ou cle deja enregistree : message dans la fenetre", async () => {
  stubCreate(new DOMException("x", "NotAllowedError"));
  renderCard();
  fireEvent.click(await screen.findByRole("button", { name: "Ajouter" }));
  typePassword();
  fireEvent.click(screen.getByRole("button", { name: "Continuer" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Opération annulée ou délai dépassé. Réessayez.");
  stubCreate(new DOMException("x", "InvalidStateError"));
  typePassword();
  fireEvent.click(screen.getByRole("button", { name: "Continuer" }));
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Cette clé est déjà enregistrée."));
  expect(verifies).toEqual([]);
});

it("suppression : mot de passe redemande, refus puis succes", async () => {
  keys = [KEY];
  renderCard();
  const list = await screen.findByRole("list", { name: "Clés enregistrées" });
  expect(within(list).getByText("YubiKey bureau")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Supprimer YubiKey bureau" }));
  const confirm = screen.getByRole("button", { name: "Supprimer" });
  expect(confirm).toBeDisabled();
  deleteReply = () => HttpResponse.json({ error: { code: "invalid_credentials", message: "x" } }, { status: 401 });
  fireEvent.change(screen.getByLabelText("Mot de passe"), { target: { value: "mauvais" } });
  fireEvent.click(confirm);
  expect(await screen.findByRole("alert")).toHaveTextContent("Mot de passe incorrect.");
  deleteReply = () => new HttpResponse(null, { status: 204 });
  keys = [];
  fireEvent.change(screen.getByLabelText("Mot de passe"), { target: { value: "correct horse" } });
  fireEvent.click(screen.getByRole("button", { name: "Supprimer" }));
  expect(await screen.findByText("YubiKey, Touch ID · aucune")).toBeInTheDocument();
  expect(deletes).toEqual([
    { id: "k1", csrf: "csrf-1", body: { password: "mauvais" } },
    { id: "k1", csrf: "csrf-1", body: { password: "correct horse" } },
  ]);
});

it("panneau ouvert par une adresse IP : pas de bouton d'ajout, explication", async () => {
  available = false;
  renderCard();
  expect(await screen.findByText(/par son nom de domaine/)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Ajouter" })).not.toBeInTheDocument();
});

it("panneau hors de son adresse publique : pas de bouton d'ajout, adresse publique donnee", async () => {
  stubCreate();
  server.use(http.get("*/api/auth/webauthn/status", () => HttpResponse.json({ available: false, public_url: "https://panel.example.com:8443" })));
  renderCard();
  expect(await screen.findByText(/ouvrez https:\/\/panel\.example\.com:8443 pour en ajouter une/)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Ajouter" })).not.toBeInTheDocument();
});

it("ajout : le bouton Continuer attend le mot de passe, un mot de passe faux est signale", async () => {
  const create = stubCreate();
  server.use(http.post("*/api/me/webauthn/register/options", () => HttpResponse.json({ error: { code: "invalid_credentials", message: "x" } }, { status: 401 })));
  renderCard();
  fireEvent.click(await screen.findByRole("button", { name: "Ajouter" }));
  const dialog = screen.getByRole("dialog", { name: "Ajouter une clé de sécurité" });
  expect(within(dialog).getByLabelText("Mot de passe")).toHaveAttribute("type", "password");
  expect(within(dialog).getByRole("button", { name: "Continuer" })).toBeDisabled();
  typePassword("mauvais");
  fireEvent.click(within(dialog).getByRole("button", { name: "Continuer" }));
  expect(await within(dialog).findByRole("alert")).toHaveTextContent("Mot de passe incorrect.");
  expect(create).not.toHaveBeenCalled();
});

it("ajout : un 401 de session expiree n'est pas presente comme un mot de passe faux ; l'annulation garde le mot de passe", async () => {
  stubCreate(new DOMException("x", "NotAllowedError"));
  renderCard();
  fireEvent.click(await screen.findByRole("button", { name: "Ajouter" }));
  typePassword();
  fireEvent.click(screen.getByRole("button", { name: "Continuer" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Opération annulée");
  expect(screen.getByLabelText("Mot de passe")).toHaveValue("mon mot de passe");
  server.use(http.post("*/api/me/webauthn/register/options", () => HttpResponse.json({ error: { code: "unauthenticated", message: "x" } }, { status: 401 })));
  fireEvent.click(screen.getByRole("button", { name: "Continuer" }));
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Enregistrement impossible"));
  expect(screen.getByRole("alert")).not.toHaveTextContent("Mot de passe incorrect.");
});
