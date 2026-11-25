import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory } from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";

import { AppProviders, createAppQueryClient } from "@/app/providers";
import { createAppRouter } from "@/app/router";
import { getCsrfToken, setCsrfToken } from "@/lib/api/csrf";

import { LoginFlow, looksLikeEmail, tooManyAttempts } from "./LoginFlow";

const PASSWORD = "correct horse battery staple";
const ME = { id: "u1", name: "Alice Martin", email: "alice@example.com", role: "owner", csrf_token: "csrf-1", totp_enabled: false, mfa_enrollment_required: false };

let loginBodies: unknown[];
let release: (() => void) | null;
let reply: () => Response;
let factorCalls: { path: string; body: unknown }[];
let factorReply: () => Response;
let methods: { totp: boolean; webauthn: boolean; recovery: boolean; public_url?: string | null };
let available: boolean;
let statusCalls: number;
let assertCalls: { path: string; body: unknown }[];
let assertReply: () => Response;

const server = setupServer(
  http.post("*/api/auth/login", async ({ request }) => {
    loginBodies.push(await request.json());
    // Reponse retenue jusqu'a ce que le test la libere (etat « en cours »).
    await new Promise<void>((resolve) => {
      release = resolve;
    });
    return reply();
  }),
  http.get("*/api/me", () => HttpResponse.json(ME)),
  ...["/api/auth/mfa/verify", "/api/auth/recovery-code"].map((path) =>
    http.post(`*${path}`, async ({ request }) => {
      factorCalls.push({ path, body: await request.json() });
      return factorReply();
    }),
  ),
  http.get("*/api/auth/mfa/methods", () => HttpResponse.json(methods)),
  http.get("*/api/auth/webauthn/status", () => {
    statusCalls += 1;
    return HttpResponse.json({ available });
  }),
  http.post("*/api/auth/webauthn/assert/options", async ({ request }) => {
    assertCalls.push({ path: "options", body: await request.json() });
    return HttpResponse.json({ ceremony_id: "c-1", options: { publicKey: { challenge: "AQID", rpId: "localhost", allowCredentials: [{ type: "public-key", id: "BAUG" }], userVerification: "required" } } });
  }),
  http.post("*/api/auth/webauthn/assert/verify", async ({ request }) => {
    assertCalls.push({ path: "verify", body: await request.json() });
    return assertReply();
  }),
);

const refuse = (status: number, headers: Record<string, string> = {}) => () =>
  HttpResponse.json({ error: { code: "x", message: "detail interne" } }, { status, headers });

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  loginBodies = [];
  release = null;
  reply = refuse(401);
  factorCalls = [];
  factorReply = () => HttpResponse.json({ csrf_token: "csrf-3" });
  methods = { totp: true, webauthn: false, recovery: true };
  available = false;
  statusCalls = 0;
  assertCalls = [];
  assertReply = () => HttpResponse.json({ csrf_token: "csrf-5" });
  setCsrfToken(null);
});
afterEach(() => {
  server.resetHandlers();
  vi.unstubAllGlobals();
  Reflect.deleteProperty(navigator, "credentials");
});

const bytes = (...b: number[]) => new Uint8Array(b).buffer;

/** Navigateur WebAuthn simule : `get` renvoie une assertion fixe (ou l'erreur donnee). */
function stubWebauthn(error?: DOMException) {
  vi.stubGlobal("PublicKeyCredential", function PublicKeyCredential() {});
  const get = vi.fn(async (_options: CredentialRequestOptions) => {
    if (error) throw error;
    return {
      id: "BAUG",
      rawId: bytes(4, 5, 6),
      type: "public-key",
      response: { authenticatorData: bytes(7), clientDataJSON: bytes(8), signature: bytes(9), userHandle: bytes(10) },
      getClientExtensionResults: () => ({}),
    };
  });
  Object.defineProperty(navigator, "credentials", { value: { get, create: vi.fn() }, configurable: true });
  return get;
}

function mount(url = "/login") {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [url] }));
  const client = createAppQueryClient(router);
  render(<AppProviders router={router} client={client} />);
  return { router };
}

const type = (label: string, value: string) => fireEvent.change(screen.getByLabelText(label), { target: { value } });
const submitButton = () => screen.getByRole("button", { name: "Se connecter" });

async function fillAndSubmit(email = "alice@example.com", password = PASSWORD) {
  await screen.findByRole("heading", { name: "Connectez-vous à Ciphera" });
  type("E-mail", email);
  type("Mot de passe", password);
  fireEvent.click(submitButton());
  await waitFor(() => expect(release).not.toBeNull());
}

it("bouton desactive pendant la requete, puis erreur generique", async () => {
  mount();
  await screen.findByRole("heading", { name: "Connectez-vous à Ciphera" });
  expect(submitButton()).toBeDisabled();
  await fillAndSubmit();
  expect(submitButton()).toBeDisabled();
  expect(submitButton()).toHaveAttribute("aria-busy", "true");
  // Un second envoi pendant la requete est ignore.
  fireEvent.submit(submitButton().closest("form")!);

  release!();
  expect(await screen.findByRole("alert")).toHaveTextContent("E-mail ou mot de passe incorrect.");
  expect(screen.queryByText("detail interne")).not.toBeInTheDocument();
  expect(submitButton()).toBeEnabled();
  expect(loginBodies).toEqual([{ email: "alice@example.com", password: PASSWORD, remember: false }]);
  expect(getCsrfToken()).toBeNull();
});

it("trop de tentatives : delai lu dans Retry-After", async () => {
  reply = refuse(429, { "Retry-After": "120" });
  mount();
  await fillAndSubmit();
  release!();
  expect(await screen.findByRole("alert")).toHaveTextContent("Trop de tentatives. Réessayez dans 2 minutes.");
});

it("connexion reussie : jeton CSRF en memoire et ouverture du tableau de bord", async () => {
  reply = () => HttpResponse.json({ mfa_required: false, csrf_token: "csrf-1" });
  const { router } = mount();
  await fillAndSubmit();
  release!();
  await waitFor(() => expect(router.state.location.pathname).toBe("/"));
  expect(getCsrfToken()).toBe("csrf-1");
  expect(await screen.findByText("Alice Martin")).toBeInTheDocument();
  expect(window.localStorage.length).toBe(0);
  expect(window.sessionStorage.length).toBe(0);
});

it("« rester connecté » est transmis", async () => {
  mount();
  await screen.findByRole("heading", { name: "Connectez-vous à Ciphera" });
  fireEvent.click(screen.getByRole("checkbox", { name: /Rester connecté/ }));
  await fillAndSubmit();
  release!();
  await screen.findByRole("alert");
  expect(loginBodies).toEqual([{ email: "alice@example.com", password: PASSWORD, remember: true }]);
});

async function reachCodeStepAs(heading: string) {
  reply = () => HttpResponse.json({ mfa_required: true, csrf_token: "csrf-2" });
  const mounted = mount();
  await fillAndSubmit();
  release!();
  expect(await screen.findByRole("heading", { name: heading })).toBeInTheDocument();
  expect(mounted.router.state.location.pathname).toBe("/login");
  return mounted;
}

const reachCodeStep = () => reachCodeStepAs("Vérification en deux étapes");

const typeCode = (code: string) => {
  for (const [i, d] of code.split("").entries()) fireEvent.change(screen.getByLabelText(`Chiffre ${i + 1}`), { target: { value: d } });
};

it("double authentification : le code TOTP est verifie puis le tableau de bord s'ouvre", async () => {
  const { router } = await reachCodeStep();
  // Ni validation simulee, ni appareil de confiance ; pas de cle de securite sans passkey sur le compte.
  expect(screen.queryByText("Simuler la validation")).not.toBeInTheDocument();
  expect(screen.queryByText("Clé de sécurité")).not.toBeInTheDocument();
  expect(screen.queryByText(/Faire confiance à cet appareil/)).not.toBeInTheDocument();
  typeCode("123456");
  await waitFor(() => expect(router.state.location.pathname).toBe("/"));
  expect(factorCalls).toEqual([{ path: "/api/auth/mfa/verify", body: { code: "123456" } }]);
  expect(getCsrfToken()).toBe("csrf-3");
});

it("code TOTP refuse : message, champs vides, nouvelle saisie possible", async () => {
  factorReply = () => HttpResponse.json({ error: { code: "invalid_code", message: "x" } }, { status: 401 });
  const { router } = await reachCodeStep();
  typeCode("000000");
  expect(await screen.findByRole("alert")).toHaveTextContent("Code incorrect, réessayez.");
  expect(screen.getByLabelText("Chiffre 1")).toHaveValue("");
  expect(router.state.location.pathname).toBe("/login");
  factorReply = () => HttpResponse.json({ csrf_token: "csrf-4" });
  typeCode("654321");
  fireEvent.click(screen.getByRole("button", { name: "Vérifier" }));
  await waitFor(() => expect(router.state.location.pathname).toBe("/"));
  expect(factorCalls.map((c) => c.body)).toContainEqual({ code: "654321" });
});

it("trop de tentatives de second facteur : delai lu dans Retry-After", async () => {
  factorReply = () => HttpResponse.json({ error: { code: "rate_limited", message: "x" } }, { status: 429, headers: { "Retry-After": "600" } });
  await reachCodeStep();
  typeCode("111111");
  expect(await screen.findByRole("alert")).toHaveTextContent("Trop de tentatives. Réessayez dans 10 minutes.");
});

it("code de secours : envoye a /api/auth/recovery-code", async () => {
  const { router } = await reachCodeStep();
  fireEvent.click(screen.getByText("Code de secours"));
  const field = screen.getByLabelText("Code de secours");
  fireEvent.change(field, { target: { value: "abcde-fg234" } });
  expect(field).toHaveValue("ABCDE-FG234");
  fireEvent.click(screen.getByRole("button", { name: "Vérifier" }));
  await waitFor(() => expect(router.state.location.pathname).toBe("/"));
  expect(factorCalls).toEqual([{ path: "/api/auth/recovery-code", body: { code: "ABCDE-FG234" } }]);
});

it("session en attente de MFA expiree : retour a la connexion", async () => {
  factorReply = () => HttpResponse.json({ error: { code: "unauthenticated", message: "x" } }, { status: 401 });
  await reachCodeStep();
  typeCode("123456");
  expect(await screen.findByRole("heading", { name: "Connectez-vous à Ciphera" })).toBeInTheDocument();
  expect(screen.getByRole("alert")).toHaveTextContent("Session expirée, reconnectez-vous.");
});

it("/2fa ouvre directement l'etape de verification", async () => {
  mount("/2fa");
  expect(await screen.findByRole("heading", { name: "Vérification en deux étapes" })).toBeInTheDocument();
  expect(screen.getByRole("listitem", { current: "step" })).toHaveTextContent("Vérification");
});

it("?expired=1 affiche le bandeau de session expiree, sans selecteur de langue", async () => {
  mount("/login?expired=1");
  expect(await screen.findByRole("heading", { name: "Session expirée" })).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Connectez-vous à Ciphera" })).toBeInTheDocument();
  expect(screen.queryByText("Français")).not.toBeInTheDocument();
});

it("sans ?expired, pas de bandeau", async () => {
  mount("/login");
  await screen.findByRole("heading", { name: "Connectez-vous à Ciphera" });
  expect(screen.queryByText("Session expirée")).not.toBeInTheDocument();
});

it("l'etape d'inscription annonce l'inscription sur invitation", () => {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <LoginFlow initialStep="register" onSignedIn={() => undefined} />
    </QueryClientProvider>,
  );
  expect(screen.getByRole("heading", { name: "Inscription sur invitation" })).toBeInTheDocument();
  expect(screen.getByRole("listitem", { current: "step" })).toHaveTextContent("Inscription");
  fireEvent.click(screen.getByRole("button", { name: "Se connecter" }));
  expect(screen.getByRole("heading", { name: "Connectez-vous à Ciphera" })).toBeInTheDocument();
});

it("controle d'e-mail aligne sur le serveur : un domaine internationalise est accepte", async () => {
  for (const ok of ["owner@example.xn--p1ai", "a@b.c", " alice@example.com "]) expect(looksLikeEmail(ok)).toBe(true);
  for (const ko of ["", "alice", "@example.com", "alice@", "  @ "]) expect(looksLikeEmail(ko)).toBe(false);
  mount();
  await fillAndSubmit("owner@example.xn--p1ai");
  release!();
  await screen.findByRole("alert");
  expect(loginBodies).toEqual([{ email: "owner@example.xn--p1ai", password: PASSWORD, remember: false }]);
});

it("cle de securite comme second facteur : seule methode du compte, proposee d'emblee", async () => {
  const get = stubWebauthn();
  methods = { totp: false, webauthn: true, recovery: false };
  const { router } = await reachCodeStepAs("Clé de sécurité");
  expect(screen.queryByLabelText("Chiffre 1")).not.toBeInTheDocument();
  expect(screen.queryByText("Code de secours")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Utiliser ma clé" }));
  await waitFor(() => expect(router.state.location.pathname).toBe("/"));
  const options = get.mock.calls[0][0].publicKey!;
  expect(new Uint8Array(options.challenge as ArrayBuffer)).toEqual(new Uint8Array([1, 2, 3]));
  expect(new Uint8Array(options.allowCredentials![0].id as ArrayBuffer)).toEqual(new Uint8Array([4, 5, 6]));
  expect(assertCalls).toEqual([
    { path: "options", body: { mode: "second_factor" } },
    {
      path: "verify",
      body: {
        ceremony_id: "c-1",
        remember: false,
        credential: { id: "BAUG", rawId: "BAUG", type: "public-key", response: { authenticatorData: "Bw", clientDataJSON: "CA", signature: "CQ", userHandle: "Cg" }, extensions: {} },
      },
    },
  ]);
  expect(getCsrfToken()).toBe("csrf-5");
});

it("cle de securite refusee ou annulee : message, puis nouvel essai", async () => {
  stubWebauthn(new DOMException("annule", "NotAllowedError"));
  methods = { totp: true, webauthn: true, recovery: true };
  await reachCodeStep();
  fireEvent.click(screen.getByText("Clé de sécurité"));
  expect(await screen.findByRole("alert")).toHaveTextContent("Opération annulée ou délai dépassé. Réessayez.");
  expect(assertCalls.map((c) => c.path)).toEqual(["options"]);
  stubWebauthn();
  assertReply = () => HttpResponse.json({ error: { code: "webauthn_failed", message: "x" } }, { status: 401 });
  fireEvent.click(screen.getByRole("button", { name: "Réessayer" }));
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Clé de sécurité refusée. Réessayez."));
  fireEvent.click(screen.getByText("Utiliser l'application d'authentification"));
  expect(screen.getByLabelText("Chiffre 1")).toBeInTheDocument();
});

it("connexion sans mot de passe avec une cle de securite", async () => {
  stubWebauthn();
  available = true;
  const { router } = mount();
  await screen.findByRole("heading", { name: "Connectez-vous à Ciphera" });
  fireEvent.click(screen.getByRole("checkbox", { name: /Rester connecté/ }));
  fireEvent.click(await screen.findByText("Se connecter sans mot de passe"));
  await waitFor(() => expect(router.state.location.pathname).toBe("/"));
  expect(assertCalls[0]).toEqual({ path: "options", body: { mode: "login" } });
  expect(assertCalls[1].body).toMatchObject({ ceremony_id: "c-1", remember: true });
  expect(loginBodies).toEqual([]);
});

it("panneau ouvert par une adresse IP (WebAuthn indisponible) : pas de connexion par cle", async () => {
  stubWebauthn();
  mount();
  await screen.findByRole("heading", { name: "Connectez-vous à Ciphera" });
  // La reponse du serveur est arrivee (et rendue) avant de conclure a l'absence du bouton.
  await waitFor(() => expect(statusCalls).toBe(1));
  await new Promise((resolve) => setTimeout(resolve, 50));
  expect(screen.queryByText("Se connecter sans mot de passe")).not.toBeInTheDocument();
  expect(screen.queryByText("Clé de sécurité")).not.toBeInTheDocument();
});

it("compte a cle de securite, panneau ouvert par une adresse IP : codes de secours et explication", async () => {
  methods = { totp: false, webauthn: false, recovery: true };
  await reachCodeStepAs("Code de secours");
  expect(screen.getByText(/ouvrez le panneau par son nom de domaine/)).toBeInTheDocument();
  expect(screen.queryByLabelText("Chiffre 1")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Utiliser ma clé" })).not.toBeInTheDocument();
});

it("compte a cle de securite, panneau hors de son adresse publique : codes de secours et adresse publique", async () => {
  methods = { totp: false, webauthn: false, recovery: true, public_url: "https://panel.example.com:8443" };
  await reachCodeStepAs("Code de secours");
  expect(screen.getByText(/ne fonctionne qu'à l'adresse publique du panneau : ouvrez https:\/\/panel\.example\.com:8443/)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Utiliser ma clé" })).not.toBeInTheDocument();
});

it("compte a TOTP et cle, panneau hors de son adresse publique : code TOTP, et l'adresse ou la cle fonctionne", async () => {
  methods = { totp: true, webauthn: false, recovery: true, public_url: "https://panel.example.com:8443" };
  await reachCodeStepAs("Vérification en deux étapes");
  expect(screen.getByLabelText("Chiffre 1")).toBeInTheDocument();
  expect(screen.getByText(/ouvrez https:\/\/panel\.example\.com:8443 pour l'utiliser/)).toBeInTheDocument();
});

it("aucune methode utilisable : explication seule, ni code TOTP ni cle", async () => {
  methods = { totp: false, webauthn: false, recovery: false };
  await reachCodeStepAs("Clé de sécurité");
  expect(screen.getByText(/ouvrez le panneau par son nom de domaine/)).toBeInTheDocument();
  expect(screen.queryByLabelText("Chiffre 1")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /Vérifier|Utiliser ma clé/ })).not.toBeInTheDocument();
  expect(assertCalls).toEqual([]);
});
it("attente longue (plafond journalier du second facteur) : en heures, pas en minutes", () => {
  expect(tooManyAttempts(120)).toBe("Trop de tentatives. Réessayez dans 2 minutes.");
  expect(tooManyAttempts(30)).toBe("Trop de tentatives. Réessayez dans 1 minute.");
  expect(tooManyAttempts(3540)).toBe("Trop de tentatives. Réessayez dans 59 minutes.");
  expect(tooManyAttempts(3600)).toBe("Trop de tentatives. Réessayez dans 1 h.");
  expect(tooManyAttempts(5400)).toBe("Trop de tentatives. Réessayez dans 2 h.");
  expect(tooManyAttempts(86_400)).toBe("Trop de tentatives. Réessayez dans 24 h.");
});
