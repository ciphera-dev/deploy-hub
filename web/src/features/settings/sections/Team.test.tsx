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

let me: { id: string; role: string; name: string; email: string };
let members: ReturnType<typeof member>[];
let invitations: { id: string; email: string; name: string; role: string; created_at: number; expires_at: number; invited_by: string }[];
let calls: { method: string; path: string; body: unknown }[];
const removed = { deleted_hooks: 2, deleted_invitations: 1 };

const server = setupServer(
  http.get("*/api/me", () => HttpResponse.json(me)),
  http.get("*/api/team/members", () => HttpResponse.json(members)),
  http.get("*/api/team/invitations", () => HttpResponse.json(invitations)),
  http.patch("*/api/team/members/:id", async ({ request, params }) => {
    const body = (await request.json()) as { role: string };
    calls.push({ method: "PATCH", path: String(params.id), body });
    members = members.map((m) => (m.id === params.id ? { ...m, role: body.role } : m));
    return new HttpResponse(null, { status: 204 });
  }),
  http.delete("*/api/team/members/:id", ({ params }) => {
    calls.push({ method: "DELETE", path: String(params.id), body: null });
    members = members.filter((m) => m.id !== params.id);
    return HttpResponse.json(removed);
  }),
  http.post("*/api/team/members/:id/reset-link", ({ params }) => {
    calls.push({ method: "POST", path: `reset:${String(params.id)}`, body: null });
    return HttpResponse.json({ link: "/reset/abc" }, { status: 201 });
  }),
  http.post("*/api/team/invitations", async ({ request }) => {
    const body = (await request.json()) as { email: string; role: string; name?: string };
    calls.push({ method: "POST", path: "invitations", body });
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
  await waitFor(() => expect(calls).toContainEqual({ method: "PATCH", path: ID(3), body: { role: "ops" } }));
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
  fireEvent.click(within(dialog).getByRole("button", { name: "Retirer" }));
  await waitFor(() => expect(calls).toContainEqual({ method: "DELETE", path: ID(4), body: null }));
  expect(await screen.findByText(/2 deploy hooks supprimés, 1 invitation supprimée/)).toBeInTheDocument();
  await waitFor(() => expect(screen.queryByText("David")).not.toBeInTheDocument());
});

it("génère un lien de réinitialisation affiché dans une modale", async () => {
  mount();
  await screen.findByText("Chloe");
  fireEvent.keyDown(screen.getByRole("button", { name: "Actions de Chloe" }), { key: "ArrowDown" });
  fireEvent.click(await screen.findByRole("menuitem", { name: "Lien de réinitialisation" }));
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
  fireEvent.click(send);
  await waitFor(() => expect(calls).toContainEqual({ method: "POST", path: "invitations", body: { email: "nouveau@example.com", role: "developer" } }));
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
