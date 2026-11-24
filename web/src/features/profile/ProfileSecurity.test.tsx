import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it } from "vitest";

import { ToastProvider } from "@/design-system";
import { setCsrfToken } from "@/lib/api/csrf";

import { describeAgent, ProfileSecurity, since } from "./ProfileSecurity";

const ME = { id: "u1", name: "Alice Martin", email: "alice@example.com", role: "owner", csrf_token: "csrf-1", totp_enabled: true, webauthn_enabled: false, mfa_enrollment_required: false, language: "fr", timezone: "UTC", avatar_url: null };
const NOW = Math.floor(Date.now() / 1000);
const MAC = "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_5) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
const IPHONE = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1";

let sessions: { id: string; ip: string | null; user_agent: string | null; created_at: number; last_seen_at: number; current: boolean }[];
let revoked: { id: string; csrf: string | null }[];
let passwords: { csrf: string | null; body: unknown }[];
let passwordReply: () => Response;
let totpDeletes: unknown[];

const server = setupServer(
  http.get("*/api/me", () => HttpResponse.json(ME)),
  http.get("*/api/me/webauthn", () => HttpResponse.json({ credentials: [] })),
  http.get("*/api/auth/webauthn/status", () => HttpResponse.json({ available: true })),
  http.get("*/api/me/sessions", () => HttpResponse.json({ sessions })),
  http.delete("*/api/me/sessions/:id", ({ request, params }) => {
    revoked.push({ id: params.id as string, csrf: request.headers.get("x-csrf-token") });
    sessions = sessions.filter((s) => s.id !== params.id);
    return new HttpResponse(null, { status: 204 });
  }),
  http.post("*/api/me/password", async ({ request }) => {
    passwords.push({ csrf: request.headers.get("x-csrf-token"), body: await request.json() });
    return passwordReply();
  }),
  http.delete("*/api/me/mfa/totp", async ({ request }) => {
    totpDeletes.push(await request.json());
    return new HttpResponse(null, { status: 204 });
  }),
);

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  sessions = [
    { id: "s1", ip: "203.0.113.7", user_agent: MAC, created_at: NOW - 3600, last_seen_at: NOW, current: true },
    { id: "s2", ip: "198.51.100.4", user_agent: IPHONE, created_at: NOW - 86400, last_seen_at: NOW - 7200, current: false },
    { id: "s3", ip: "192.0.2.9", user_agent: null, created_at: NOW - 3 * 86400, last_seen_at: NOW - 3 * 86400, current: false },
  ];
  revoked = [];
  passwords = [];
  passwordReply = () => new HttpResponse(null, { status: 204 });
  totpDeletes = [];
  setCsrfToken("csrf-1");
});
afterEach(() => server.resetHandlers());

function renderSecurity() {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ToastProvider>
        <ProfileSecurity />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

it("sessions : la session courante est marquee, sans bouton de deconnexion", async () => {
  renderSecurity();
  const list = await screen.findByRole("list", { name: "Sessions actives" });
  await waitFor(() => expect(within(list).getAllByRole("listitem")).toHaveLength(3));
  const [current, phone] = within(list).getAllByRole("listitem");
  expect(current).toHaveTextContent("Mac");
  expect(current).toHaveTextContent("Cet appareil");
  expect(within(current).queryByRole("button")).not.toBeInTheDocument();
  expect(phone).toHaveTextContent("iPhone");
  expect(phone).toHaveTextContent("198.51.100.4");
  expect(phone).toHaveTextContent("il y a 2 h");
  expect(screen.getByText("3 appareils connectés")).toBeInTheDocument();
});

it("sessions : deconnecter une autre session, puis toutes les autres", async () => {
  renderSecurity();
  fireEvent.click(await screen.findByRole("button", { name: "Déconnecter iPhone (Safari 17)" }));
  await waitFor(() => expect(revoked).toEqual([{ id: "s2", csrf: "csrf-1" }]));
  expect(await screen.findByText("2 appareils connectés")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Déconnecter les autres" }));
  await waitFor(() => expect(revoked.map((r) => r.id)).toEqual(["s2", "s3"]));
  expect(await screen.findByText("1 appareil connecté")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Déconnecter les autres" })).not.toBeInTheDocument();
});

it("mot de passe : confirmation verifiee, puis envoi avec le mot de passe actuel", async () => {
  renderSecurity();
  fireEvent.click(await screen.findByRole("button", { name: "Modifier" }));
  const dialog = await screen.findByRole("dialog");
  fireEvent.change(within(dialog).getByLabelText("Mot de passe actuel"), { target: { value: "ancien mot de passe" } });
  fireEvent.change(within(dialog).getByLabelText("Nouveau mot de passe"), { target: { value: "une phrase assez longue" } });
  fireEvent.change(within(dialog).getByLabelText("Confirmer le nouveau mot de passe"), { target: { value: "une autre phrase" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Enregistrer" }));
  expect(await within(dialog).findByRole("alert")).toHaveTextContent("ne correspondent pas");
  expect(passwords).toEqual([]);

  passwordReply = () => HttpResponse.json({ error: { code: "invalid_credentials", message: "Mot de passe actuel incorrect." } }, { status: 401 });
  fireEvent.change(within(dialog).getByLabelText("Confirmer le nouveau mot de passe"), { target: { value: "une phrase assez longue" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Enregistrer" }));
  await waitFor(() => expect(within(dialog).getByRole("alert")).toHaveTextContent("Mot de passe actuel incorrect."));

  passwordReply = () => new HttpResponse(null, { status: 204 });
  fireEvent.change(within(dialog).getByLabelText("Mot de passe actuel"), { target: { value: "ancien mot de passe" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Enregistrer" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  expect(passwords.at(-1)).toEqual({ csrf: "csrf-1", body: { current_password: "ancien mot de passe", new_password: "une phrase assez longue" } });
  expect(await screen.findByRole("status")).toHaveTextContent("Mot de passe modifié");
});

it("application d'authentification : l'interrupteur desactive avec le mot de passe", async () => {
  renderSecurity();
  const toggle = await screen.findByRole("switch", { name: "Application d'authentification" });
  await waitFor(() => expect(toggle).toBeChecked());
  fireEvent.click(toggle);
  const dialog = await screen.findByRole("dialog");
  fireEvent.change(within(dialog).getByLabelText("Mot de passe"), { target: { value: "mon mot de passe" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Désactiver" }));
  await waitFor(() => expect(totpDeletes).toEqual([{ password: "mon mot de passe" }]));
});

it("score : 40 + 20 par methode active", async () => {
  renderSecurity();
  expect(await screen.findByText("80")).toBeInTheDocument();
  expect(screen.getByText("Bonne protection")).toBeInTheDocument();
});

it("lecture du User-Agent et anciennete", () => {
  expect(describeAgent(MAC)).toEqual({ icon: "Laptop", device: "Mac", browser: "Chrome 131" });
  expect(describeAgent(IPHONE)).toEqual({ icon: "Smartphone", device: "iPhone", browser: "Safari 17" });
  expect(describeAgent(null).device).toBe("Appareil inconnu");
  const now = 1_800_000_000_000;
  expect(since(now / 1000 - 30, now)).toBe("Active maintenant");
  expect(since(now / 1000 - 15 * 60, now)).toBe("il y a 15 min");
  expect(since(now / 1000 - 3 * 86400, now)).toBe("il y a 3 j");
});
