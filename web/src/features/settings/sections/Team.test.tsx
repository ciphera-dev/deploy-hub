import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it } from "vitest";

import { ToastProvider } from "@/design-system";

import { canManage, grantableRoles, TeamSection } from "./Team";

const ID = (n: number) => `00000000-0000-4000-8000-00000000000${n}`;
const NOW = Math.floor(Date.now() / 1000);
const member = (n: number, name: string, role: string) => ({ id: ID(n), name, email: `${name.toLowerCase()}@example.com`, role, two_factor: n % 2 === 0, created_at: NOW - 1000, last_active_at: n === 3 ? null : NOW - 120, pending: n === 4 });

let me: { id: string; role: string; name: string; email: string; totp_enabled?: boolean };
let members: ReturnType<typeof member>[];
let invitations: { id: string; email: string; name: string; role: string; created_at: number; expires_at: number; invited_by: string }[];
let calls: { method: string; path: string; body: unknown }[];
const removed = { deleted_hooks: 2, deleted_invitations: 1 };

type StepUp = { password?: string; code?: string };
/** Confirmation renforcee simulee : mot de passe « bon », plus le code « 123456 » si le compte a un second facteur. */
function refusal(body: StepUp) {
  if (body.password !== "bon") return HttpResponse.json({ error: { code: "invalid_credentials", message: "Mot de passe actuel incorrect." } }, { status: 401 });
  if (me.totp_enabled && !body.code) return HttpResponse.json({ error: { code: "second_factor_required", message: "Saisissez aussi le code de votre application d'authentification ou un code de secours." } }, { status: 401 });
  if (me.totp_enabled && body.code !== "123456") return HttpResponse.json({ error: { code: "invalid_code", message: "Code de vérification incorrect." } }, { status: 401 });
  return null;
}

const server = setupServer(
  http.get("*/api/me", () => HttpResponse.json(me)),
  http.get("*/api/team/members", () => HttpResponse.json(members)),
  http.get("*/api/team/invitations", () => HttpResponse.json(invitations)),
  http.patch("*/api/team/members/:id", async ({ request, params }) => {
    const body = (await request.json()) as { role: string } & StepUp;
    calls.push({ method: "PATCH", path: String(params.id), body });
    const refused = refusal(body);
    if (refused) return refused;
    members = members.map((m) => (m.id === params.id ? { ...m, role: body.role } : m));
    return new HttpResponse(null, { status: 204 });
  }),
  http.delete("*/api/team/members/:id", async ({ request, params }) => {
    const body = (await request.json()) as StepUp;
    calls.push({ method: "DELETE", path: String(params.id), body });
    const refused = refusal(body);
    if (refused) return refused;
    members = members.filter((m) => m.id !== params.id);
    return HttpResponse.json(removed);
  }),
  http.post("*/api/team/members/:id/reset-link", async ({ request, params }) => {
    const body = (await request.json()) as StepUp;
    calls.push({ method: "POST", path: `reset:${String(params.id)}`, body });
    const refused = refusal(body);
    if (refused) return refused;
    return HttpResponse.json({ link: "/reset/abc" }, { status: 201 });
  }),
  http.post("*/api/team/members/:id/reset-mfa", async ({ request, params }) => {
    const body = (await request.json()) as StepUp;
    calls.push({ method: "POST", path: `reset-mfa:${String(params.id)}`, body });
    const refused = refusal(body);
    if (refused) return refused;
    members = members.map((m) => (m.id === params.id ? { ...m, two_factor: false } : m));
    return new HttpResponse(null, { status: 204 });
  }),
  http.post("*/api/team/invitations", async ({ request }) => {
    const body = (await request.json()) as { email: string; role: string; name?: string } & StepUp;
    calls.push({ method: "POST", path: "invitations", body });
    const refused = refusal(body);
    if (refused) return refused;
    invitations = [...invitations, { id: ID(9), email: body.email, name: body.name ?? "", role: body.role, created_at: NOW, expires_at: NOW + 259_200, invited_by: "Alice" }];
    return HttpResponse.json({ link: "/invite/secret-token" }, { status: 201 });
  }),
  http.delete("*/api/team/invitations/:id", ({ params }) => {
    calls.push({ method: "DELETE", path: `invitation:${String(params.id)}`, body: null });
    invitations = invitations.filter((i) => i.id !== params.id);
    return new HttpResponse(null, { status: 204 });
  }),
);

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  me = { id: ID(1), role: "owner", name: "Alice", email: "alice@example.com" };
  members = [member(1, "Alice", "owner"), member(2, "Bruno", "admin"), member(3, "Chloe", "developer"), member(4, "David", "viewer")];
  invitations = [{ id: ID(8), email: "eva@example.com", name: "", role: "viewer", created_at: NOW - 600, expires_at: NOW + 1000, invited_by: "Alice" }];
  calls = [];
});
afterEach(() => server.resetHandlers());

function mount() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <TeamSection />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

it("applique les règles de rang du serveur", () => {
  expect(canManage("owner", "owner")).toBe(true);
  expect(canManage("admin", "admin")).toBe(false);
  expect(canManage("admin", "owner")).toBe(false);
  expect(canManage("admin", "viewer")).toBe(true);
  expect(canManage("developer", "viewer")).toBe(false);
  expect(grantableRoles("admin")).not.toContain("owner");
  expect(grantableRoles("owner")).toContain("owner");
});

it("affiche les membres, les invitations et les onglets v0.2 désactivés", async () => {
  mount();
  expect(await screen.findByText("Bruno")).toBeInTheDocument();
  expect(screen.getByText("bruno@example.com")).toBeInTheDocument();
  expect(screen.getByText("Jamais connecté")).toBeInTheDocument();
  expect(screen.getByText("eva@example.com")).toBeInTheDocument();
  expect(screen.getByText("Vous")).toBeInTheDocument();
  expect(screen.getByRole("tab", { name: /Rôles et permissions/ })).toBeDisabled();
  expect(screen.getByRole("tab", { name: /Journal d'accès/ })).toBeDisabled();
  expect(screen.getAllByText("v0.2")).toHaveLength(2);
});

it("compte en attente : badge, pas de changement de rôle", async () => {
  mount();
  await screen.findByText("Bruno");
  expect(screen.getAllByText("En attente")).toHaveLength(1);
  expect(screen.queryByRole("button", { name: "Changer le rôle de David" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Changer le rôle de Bruno" })).toBeInTheDocument();
});

it("ne propose aucune action sur sa propre ligne", async () => {
  mount();
  await screen.findByText("Bruno");
  expect(screen.queryByRole("button", { name: "Actions de Alice" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Changer le rôle de Alice" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Actions de Bruno" })).toBeInTheDocument();
});

it("un administrateur ne gère ni admin ni propriétaire et ne propose pas owner", async () => {
  me = { id: ID(2), role: "admin", name: "Bruno", email: "bruno@example.com" };
  mount();
  await screen.findByText("Alice");
  expect(screen.queryByRole("button", { name: "Actions de Alice" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Changer le rôle de Alice" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Actions de Bruno" })).not.toBeInTheDocument();
  fireEvent.keyDown(screen.getByRole("button", { name: "Changer le rôle de Chloe" }), { key: "ArrowDown" });
  expect(await screen.findByRole("menuitem", { name: "Administrateur" })).toBeInTheDocument();
  expect(screen.getByRole("menuitem", { name: "Lecteur" })).toBeInTheDocument();
  expect(screen.queryByRole("menuitem", { name: "Propriétaire" })).not.toBeInTheDocument();
});

it("change le rôle d'un membre", async () => {
  mount();
  await screen.findByText("Chloe");
  fireEvent.keyDown(screen.getByRole("button", { name: "Changer le rôle de Chloe" }), { key: "ArrowDown" });
  expect(await screen.findByRole("menuitem", { name: "Propriétaire" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("menuitem", { name: "Ops" }));
  const dialog = await screen.findByRole("dialog", { name: "Changer le rôle de Chloe ?" });
  expect(calls).toHaveLength(0);
  const confirm = within(dialog).getByRole("button", { name: "Confirmer" });
  expect(confirm).toBeDisabled();
  fireEvent.change(within(dialog).getByLabelText("Votre mot de passe"), { target: { value: "bon" } });
  fireEvent.click(confirm);
  await waitFor(() => expect(calls).toContainEqual({ method: "PATCH", path: ID(3), body: { role: "ops", password: "bon" } }));
  await waitFor(() => expect(screen.getByRole("button", { name: "Changer le rôle de Chloe" })).toHaveTextContent("Ops"));
});

it("retirer demande confirmation, annonce les suppressions puis rafraîchit", async () => {
  mount();
  await screen.findByText("David");
  fireEvent.keyDown(screen.getByRole("button", { name: "Actions de David" }), { key: "ArrowDown" });
  fireEvent.click(await screen.findByRole("menuitem", { name: "Retirer" }));
  const dialog = await screen.findByRole("dialog", { name: "Retirer David ?" });
  expect(calls).toHaveLength(0);
  expect(within(dialog).getByText(/deploy hooks et les invitations en attente/)).toBeInTheDocument();
  expect(within(dialog).getByRole("button", { name: "Retirer" })).toBeDisabled();
  fireEvent.change(within(dialog).getByLabelText("Votre mot de passe"), { target: { value: "bon" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Retirer" }));
  await waitFor(() => expect(calls).toContainEqual({ method: "DELETE", path: ID(4), body: { password: "bon" } }));
  expect(await screen.findByText(/2 deploy hooks supprimés, 1 invitation supprimée/)).toBeInTheDocument();
  await waitFor(() => expect(screen.queryByText("David")).not.toBeInTheDocument());
});

it("génère un lien de réinitialisation affiché dans une modale", async () => {
  mount();
  await screen.findByText("Chloe");
  fireEvent.keyDown(screen.getByRole("button", { name: "Actions de Chloe" }), { key: "ArrowDown" });
  fireEvent.click(await screen.findByRole("menuitem", { name: "Lien de réinitialisation" }));
  const confirm = await screen.findByRole("dialog", { name: "Lien de réinitialisation pour Chloe ?" });
  expect(calls).toHaveLength(0);
  fireEvent.change(within(confirm).getByLabelText("Votre mot de passe"), { target: { value: "bon" } });
  fireEvent.click(within(confirm).getByRole("button", { name: "Générer le lien" }));
  await waitFor(() => expect(calls).toContainEqual({ method: "POST", path: `reset:${ID(3)}`, body: { password: "bon" } }));
  const dialog = await screen.findByRole("dialog", { name: "Lien de réinitialisation" });
  expect(within(dialog).getByLabelText("Lien")).toHaveTextContent(`${window.location.origin}/reset/abc`);
});

it("invite un membre : le lien n'apparaît qu'une fois", async () => {
  mount();
  await screen.findByText("Bruno");
  fireEvent.click(screen.getByRole("button", { name: "Inviter" }));
  const dialog = await screen.findByRole("dialog", { name: "Inviter un membre" });
  const send = within(dialog).getByRole("button", { name: "Envoyer l'invitation" });
  expect(send).toBeDisabled();
  fireEvent.change(within(dialog).getByLabelText("E-mail"), { target: { value: "Nouveau@Example.com" } });
  fireEvent.click(within(dialog).getByRole("radio", { name: "Développeur" }));
  expect(send).toBeDisabled();
  fireEvent.change(within(dialog).getByLabelText("Votre mot de passe"), { target: { value: "bon" } });
  fireEvent.click(send);
  await waitFor(() => expect(calls).toContainEqual({ method: "POST", path: "invitations", body: { email: "nouveau@example.com", role: "developer", password: "bon" } }));
  const link = await screen.findByRole("dialog", { name: "Invitation créée" });
  expect(within(link).getByLabelText("Lien")).toHaveTextContent(`${window.location.origin}/invite/secret-token`);
  fireEvent.click(within(link).getByRole("button", { name: "Terminé" }));
  await waitFor(() => expect(screen.queryByText(/secret-token/)).not.toBeInTheDocument());
  expect(await screen.findByText("nouveau@example.com")).toBeInTheDocument();
});

it("annule une invitation en attente", async () => {
  mount();
  await screen.findByText("eva@example.com");
  fireEvent.click(screen.getByRole("button", { name: "Annuler l'invitation de eva@example.com" }));
  await waitFor(() => expect(calls).toContainEqual({ method: "DELETE", path: `invitation:${ID(8)}`, body: null }));
  await waitFor(() => expect(screen.queryByText("eva@example.com")).not.toBeInTheDocument());
  expect(screen.getByText("Aucune invitation en attente.")).toBeInTheDocument();
});

it("réinitialise le 2FA après confirmation par mot de passe", async () => {
  mount();
  await screen.findByText("Bruno");
  fireEvent.keyDown(screen.getByRole("button", { name: "Actions de Bruno" }), { key: "ArrowDown" });
  fireEvent.click(await screen.findByRole("menuitem", { name: "Réinitialiser le 2FA" }));
  const dialog = await screen.findByRole("dialog", { name: "Réinitialiser le 2FA de Bruno ?" });
  const go = within(dialog).getByRole("button", { name: "Réinitialiser" });
  expect(go).toBeDisabled();
  expect(calls).toHaveLength(0);
  fireEvent.change(within(dialog).getByLabelText("Votre mot de passe"), { target: { value: "faux" } });
  fireEvent.click(go);
  expect(await within(dialog).findByText("Mot de passe actuel incorrect.")).toBeInTheDocument();
  fireEvent.change(within(dialog).getByLabelText("Votre mot de passe"), { target: { value: "bon" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Réinitialiser" }));
  await waitFor(() => expect(calls).toContainEqual({ method: "POST", path: `reset-mfa:${ID(2)}`, body: { password: "bon" } }));
  expect(await screen.findByText("2FA de Bruno réinitialisé")).toBeInTheDocument();
});

it("ne propose pas la réinitialisation du 2FA à un membre sans second facteur", async () => {
  mount();
  await screen.findByText("Chloe");
  fireEvent.keyDown(screen.getByRole("button", { name: "Actions de Chloe" }), { key: "ArrowDown" });
  expect(await screen.findByRole("menuitem", { name: "Lien de réinitialisation" })).toBeInTheDocument();
  expect(screen.queryByRole("menuitem", { name: "Réinitialiser le 2FA" })).not.toBeInTheDocument();
});

it("compte avec second facteur : le code est demandé d'emblée et envoyé avec le mot de passe", async () => {
  me = { ...me, totp_enabled: true };
  mount();
  await screen.findByText("David");
  fireEvent.keyDown(screen.getByRole("button", { name: "Actions de David" }), { key: "ArrowDown" });
  fireEvent.click(await screen.findByRole("menuitem", { name: "Retirer" }));
  const dialog = await screen.findByRole("dialog", { name: "Retirer David ?" });
  fireEvent.change(within(dialog).getByLabelText("Votre mot de passe"), { target: { value: "bon" } });
  const go = within(dialog).getByRole("button", { name: "Retirer" });
  expect(go).toBeDisabled();
  fireEvent.change(within(dialog).getByLabelText("Code de vérification (application ou code de secours)"), { target: { value: "000000" } });
  fireEvent.click(go);
  expect(await within(dialog).findByText("Code de vérification incorrect.")).toBeInTheDocument();
  expect(screen.getByText("David")).toBeInTheDocument();
  fireEvent.change(within(dialog).getByLabelText("Votre mot de passe"), { target: { value: "bon" } });
  fireEvent.change(within(dialog).getByLabelText("Code de vérification (application ou code de secours)"), { target: { value: " 123456 " } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Retirer" }));
  await waitFor(() => expect(calls).toContainEqual({ method: "DELETE", path: ID(4), body: { password: "bon", code: "123456" } }));
  await waitFor(() => expect(screen.queryByText("David")).not.toBeInTheDocument());
});

it("le code est demandé quand le serveur l'exige (second_factor_required)", async () => {
  mount();
  await screen.findByText("Bruno");
  me = { ...me, totp_enabled: true };
  fireEvent.keyDown(screen.getByRole("button", { name: "Actions de Bruno" }), { key: "ArrowDown" });
  fireEvent.click(await screen.findByRole("menuitem", { name: "Réinitialiser le 2FA" }));
  const dialog = await screen.findByRole("dialog", { name: "Réinitialiser le 2FA de Bruno ?" });
  expect(within(dialog).queryByLabelText("Code de vérification (application ou code de secours)")).not.toBeInTheDocument();
  fireEvent.change(within(dialog).getByLabelText("Votre mot de passe"), { target: { value: "bon" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Réinitialiser" }));
  expect(await within(dialog).findByText(/Saisissez aussi le code/)).toBeInTheDocument();
  fireEvent.change(within(dialog).getByLabelText("Votre mot de passe"), { target: { value: "bon" } });
  fireEvent.change(within(dialog).getByLabelText("Code de vérification (application ou code de secours)"), { target: { value: "123456" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Réinitialiser" }));
  await waitFor(() => expect(calls).toContainEqual({ method: "POST", path: `reset-mfa:${ID(2)}`, body: { password: "bon", code: "123456" } }));
  expect(await screen.findByText("2FA de Bruno réinitialisé")).toBeInTheDocument();
});
