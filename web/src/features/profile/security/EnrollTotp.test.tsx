import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createMemoryHistory } from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";

import { AppProviders, createAppQueryClient } from "@/app/providers";
import { createAppRouter } from "@/app/router";
import { getCsrfToken, setCsrfToken } from "@/lib/api/csrf";

import { EnrollTotp } from "./EnrollTotp";

const SVG = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 29 29"><path d="M4,4h1v1h-1z"/></svg>';
const URI = "otpauth://totp/Ciphera:alice%40example.com?secret=JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP&issuer=Ciphera";
const CODES = Array.from({ length: 10 }, (_, i) => `AAAA${i}-BBBBB`.replace(/\d/, String.fromCharCode(65 + i)));
const ME = { id: "u1", name: "Alice Martin", email: "alice@example.com", role: "owner", csrf_token: "csrf-1", totp_enabled: false, mfa_enrollment_required: false };

let starts: (string | null)[];
let startBodies: unknown[];
let startReply: (() => Response) | null;
let confirms: { csrf: string | null; body: unknown }[];
let confirmReply: () => Response;
let me: typeof ME;
let logouts: (string | null)[];

const server = setupServer(
  http.post("*/api/me/mfa/totp/start", async ({ request }) => {
    starts.push(request.headers.get("x-csrf-token"));
    startBodies.push(await request.json().catch(() => null));
    return startReply ? startReply() : HttpResponse.json({ otpauth_uri: URI, qr_svg: SVG });
  }),
  http.post("*/api/me/mfa/totp/confirm", async ({ request }) => {
    confirms.push({ csrf: request.headers.get("x-csrf-token"), body: await request.json() });
    return confirmReply();
  }),
  http.get("*/api/me", () => HttpResponse.json(me)),
  http.get("*/api/me/webauthn", () => HttpResponse.json({ credentials: [] })),
  http.get("*/api/auth/webauthn/status", () => HttpResponse.json({ available: true })),
  http.get("*/api/me/sessions", () => HttpResponse.json({ sessions: [] })),
  http.post("*/api/auth/logout", ({ request }) => {
    logouts.push(request.headers.get("x-csrf-token"));
    return new HttpResponse(null, { status: 204 });
  }),
);

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  starts = [];
  startBodies = [];
  startReply = null;
  confirms = [];
  confirmReply = () => HttpResponse.json({ recovery_codes: CODES, csrf_token: "csrf-2" });
  me = ME;
  logouts = [];
  setCsrfToken("csrf-1");
});
afterEach(() => server.resetHandlers());

/** Saisit le mot de passe demande avant l'ajout d'un second facteur. */
async function givePassword(value = "mon mot de passe") {
  fireEvent.change(await screen.findByLabelText("Mot de passe"), { target: { value } });
  fireEvent.click(screen.getByRole("button", { name: "Continuer" }));
}

function renderEnroll(props: { forced?: boolean } = {}) {
  const onDone = vi.fn();
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient()}>
      <EnrollTotp onDone={onDone} onClose={onClose} {...props} />
    </QueryClientProvider>,
  );
  return { onDone, onClose };
}

it("affiche le QR code en image data: et la cle de saisie manuelle", async () => {
  const { container } = render(
    <QueryClientProvider client={new QueryClient()}>
      <EnrollTotp onDone={() => undefined} onClose={() => undefined} />
    </QueryClientProvider>,
  );
  await givePassword();
  const img = await screen.findByRole("img", { name: /QR code/ });
  expect(img.getAttribute("src")).toBe(`data:image/svg+xml;base64,${btoa(SVG)}`);
  // Le SVG du serveur n'est jamais injecte dans le DOM.
  expect(container.ownerDocument.querySelector('path[d="M4,4h1v1h-1z"]')).toBeNull();
  expect(screen.getByText("JBSW Y3DP EHPK 3PXP JBSW Y3DP EHPK 3PXP")).toBeInTheDocument();
  expect(starts).toEqual(["csrf-1"]);
  expect(startBodies).toEqual([{ current_password: "mon mot de passe" }]);
});

it("la modale demande le mot de passe avant toute graine ; mot de passe faux : message, aucune graine", async () => {
  startReply = () => HttpResponse.json({ error: { code: "invalid_credentials", message: "x" } }, { status: 401 });
  renderEnroll();
  expect(await screen.findByLabelText("Mot de passe")).toHaveAttribute("type", "password");
  expect(screen.getByRole("button", { name: "Continuer" })).toBeDisabled();
  expect(starts).toEqual([]);
  expect(screen.queryByRole("img", { name: /QR code/ })).not.toBeInTheDocument();
  await givePassword("mauvais");
  expect(await screen.findByRole("alert")).toHaveTextContent("Mot de passe incorrect.");
  expect(screen.queryByRole("img", { name: /QR code/ })).not.toBeInTheDocument();
});

it("code refuse : message, rien n'est active", async () => {
  confirmReply = () => HttpResponse.json({ error: { code: "invalid_code", message: "x" } }, { status: 422 });
  const { onDone } = renderEnroll();
  await givePassword();
  await screen.findByRole("img", { name: /QR code/ });
  const verify = screen.getByRole("button", { name: "Vérifier" });
  expect(verify).toBeDisabled();
  fireEvent.change(screen.getByLabelText("Code à 6 chiffres"), { target: { value: "12345a" } });
  expect(verify).toBeDisabled();
  fireEvent.change(screen.getByLabelText("Code à 6 chiffres"), { target: { value: "123456" } });
  fireEvent.click(verify);
  expect(await screen.findByRole("alert")).toHaveTextContent("Code incorrect, réessayez.");
  expect(onDone).not.toHaveBeenCalled();
  expect(getCsrfToken()).toBe("csrf-1");
});

it("code accepte : les 10 codes de secours s'affichent une fois, a copier ou telecharger", async () => {
  const createObjectURL = vi.fn((blob: Blob) => {
    void blob;
    return "blob:codes";
  });
  const revokeObjectURL = vi.fn();
  Object.assign(URL, { createObjectURL, revokeObjectURL });
  const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
  const { onDone } = renderEnroll();
  await givePassword();
  await screen.findByRole("img", { name: /QR code/ });
  fireEvent.change(screen.getByLabelText("Code à 6 chiffres"), { target: { value: "123456" } });
  fireEvent.click(screen.getByRole("button", { name: "Vérifier" }));

  const dialog = await screen.findByRole("dialog", { name: "Codes de secours" });
  for (const c of CODES) expect(within(dialog).getByText(c)).toBeInTheDocument();
  expect(confirms).toEqual([{ csrf: "csrf-1", body: { code: "123456" } }]);
  // Session tournee : nouveau jeton CSRF.
  expect(getCsrfToken()).toBe("csrf-2");

  fireEvent.click(within(dialog).getByRole("button", { name: /Télécharger/ }));
  expect(createObjectURL).toHaveBeenCalledTimes(1);
  const blob = createObjectURL.mock.calls[0][0];
  const text = await blob.text();
  for (const c of CODES) expect(text).toContain(c);
  expect(click).toHaveBeenCalled();
  expect(revokeObjectURL).toHaveBeenCalledWith("blob:codes");
  click.mockRestore();

  fireEvent.click(within(dialog).getByRole("button", { name: "J'ai enregistré mes codes" }));
  expect(onDone).toHaveBeenCalled();
});

it("enrolement impose : pas de fermeture ni d'annulation", async () => {
  renderEnroll({ forced: true });
  await givePassword();
  await screen.findByRole("img", { name: /QR code/ });
  expect(screen.queryByRole("button", { name: "Annuler" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Fermer" })).not.toBeInTheDocument();
});

function mount(url: string) {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [url] }));
  const client = createAppQueryClient(router);
  render(<AppProviders router={router} client={client} />);
  return { router };
}

it("profil › Sécurité : l'interrupteur de l'application ouvre l'enrolement", async () => {
  mount("/profile");
  fireEvent.click(await screen.findByRole("button", { name: "Sécurité" }));
  const toggle = await screen.findByRole("switch", { name: "Application d'authentification" });
  expect(toggle).not.toBeChecked();
  fireEvent.click(toggle);
  await givePassword();
  expect(await screen.findByRole("img", { name: /QR code/ })).toBeInTheDocument();
});

it("2FA imposee par l'organisation : le shell redirige vers l'enrolement, puis vers le tableau de bord", async () => {
  me = { ...ME, mfa_enrollment_required: true };
  const { router } = mount("/projects");
  await waitFor(() => expect(router.state.location.pathname).toBe("/2fa-setup"));
  await givePassword();
  await screen.findByRole("img", { name: /QR code/ });
  expect(screen.queryByRole("button", { name: "Annuler" })).not.toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Code à 6 chiffres"), { target: { value: "123456" } });
  fireEvent.click(screen.getByRole("button", { name: "Vérifier" }));
  me = { ...ME, totp_enabled: true };
  fireEvent.click(await screen.findByRole("button", { name: "J'ai enregistré mes codes" }));
  await waitFor(() => expect(router.state.location.pathname).toBe("/"));
});

it("enrolement impose : « Se déconnecter » ferme la session et mene a /login", async () => {
  me = { ...ME, mfa_enrollment_required: true };
  const { router } = mount("/projects");
  await waitFor(() => expect(router.state.location.pathname).toBe("/2fa-setup"));
  await givePassword();
  await screen.findByRole("img", { name: /QR code/ });
  fireEvent.click(screen.getByRole("button", { name: "Se déconnecter" }));
  await waitFor(() => expect(router.state.location.pathname).toBe("/login"));
  expect(logouts).toEqual(["csrf-1"]);
  expect(getCsrfToken()).toBeNull();
});

it("enrolement impose apres rechargement de /2fa-setup : GET /api/me d'abord, puis start avec le jeton CSRF", async () => {
  me = { ...ME, mfa_enrollment_required: true, csrf_token: "csrf-3" };
  setCsrfToken(null);
  const { router } = mount("/2fa-setup");
  // Le mot de passe est toujours demande : rien n'est demarre avant.
  await givePassword("bon mot de passe");
  await screen.findByRole("img", { name: /QR code/ });
  expect(router.state.location.pathname).toBe("/2fa-setup");
  expect(starts).toEqual(["csrf-3"]);
  expect(startBodies).toEqual([{ current_password: "bon mot de passe" }]);
  fireEvent.click(screen.getByRole("button", { name: "Se déconnecter" }));
  await waitFor(() => expect(router.state.location.pathname).toBe("/login"));
  expect(logouts).toEqual(["csrf-3"]);
});

it("/2fa-setup sans session : retour a /login, sans demarrer l'enrolement", async () => {
  server.use(http.get("*/api/me", () => HttpResponse.json({ error: { code: "unauthorized", message: "" } }, { status: 401 })));
  setCsrfToken(null);
  const { router } = mount("/2fa-setup");
  await waitFor(() => expect(router.state.location.pathname).toBe("/login"));
  expect(starts).toEqual([]);
});

it("/2fa-setup sans enrolement impose : retour au tableau de bord", async () => {
  setCsrfToken(null);
  const { router } = mount("/2fa-setup");
  await waitFor(() => expect(router.state.location.pathname).toBe("/"));
  expect(starts).toEqual([]);
});

it("enrolement impose : une deconnexion refusee reste sur la page et l'affiche", async () => {
  me = { ...ME, mfa_enrollment_required: true };
  server.use(http.post("*/api/auth/logout", () => HttpResponse.json({ error: { code: "csrf", message: "" } }, { status: 403 })));
  const { router } = mount("/2fa-setup");
  await givePassword();
  await screen.findByRole("img", { name: /QR code/ });
  fireEvent.click(screen.getByRole("button", { name: "Se déconnecter" }));
  expect(await screen.findByText(/Déconnexion impossible/)).toBeInTheDocument();
  expect(router.state.location.pathname).toBe("/2fa-setup");
});

it("/2fa-setup : GET /api/me en echec affiche une erreur et « Réessayer », jamais une page vide", async () => {
  me = { ...ME, mfa_enrollment_required: true };
  let meFails = true;
  server.use(http.get("*/api/me", () => (meFails ? HttpResponse.json({ error: { code: "rate_limited", message: "" } }, { status: 429 }) : HttpResponse.json(me))));
  setCsrfToken(null);
  const { router } = mount("/2fa-setup");
  const retry = await screen.findByRole("button", { name: "Réessayer" });
  expect(screen.getByRole("alert")).toHaveTextContent(/Impossible de charger votre compte/);
  expect(starts).toEqual([]);
  meFails = false;
  fireEvent.click(retry);
  await givePassword();
  await screen.findByRole("img", { name: /QR code/ });
  expect(router.state.location.pathname).toBe("/2fa-setup");
});

it("enrolement impose : une deconnexion en 401 (session deja fermee) mene a /login", async () => {
  me = { ...ME, mfa_enrollment_required: true };
  server.use(http.post("*/api/auth/logout", () => HttpResponse.json({ error: { code: "unauthorized", message: "" } }, { status: 401 })));
  const { router } = mount("/2fa-setup");
  await givePassword();
  await screen.findByRole("img", { name: /QR code/ });
  fireEvent.click(screen.getByRole("button", { name: "Se déconnecter" }));
  await waitFor(() => expect(router.state.location.pathname).toBe("/login"));
  expect(screen.queryByText(/Déconnexion impossible/)).not.toBeInTheDocument();
});
