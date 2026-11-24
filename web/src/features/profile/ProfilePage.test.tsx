import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { delay, http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it } from "vitest";

import { ToastProvider } from "@/design-system";
import { setCsrfToken } from "@/lib/api/csrf";

import { ProfilePage } from "./ProfilePage";

const ME = { id: "u1", name: "Alice Martin", email: "alice@example.com", role: "owner", csrf_token: "csrf-1", totp_enabled: false, webauthn_enabled: false, mfa_enrollment_required: false, language: "fr", timezone: "Europe/Zurich", avatar_url: null as string | null };
const PREFS = { email_available: true, email_enabled: true, deploy_succeeded: true, deploy_failed: true, certificates: false, team: true };

let me: typeof ME;
let patches: { csrf: string | null; body: Record<string, unknown> }[];
let patchReply: () => Response;
let prefs: typeof PREFS;
let prefPuts: unknown[];

const server = setupServer(
  http.get("*/api/me", () => HttpResponse.json(me)),
  http.patch("*/api/me", async ({ request }) => {
    const body = (await request.json()) as Record<string, unknown>;
    patches.push({ csrf: request.headers.get("x-csrf-token"), body });
    const reply = patchReply();
    if (reply.ok) me = { ...me, ...(body as Partial<typeof ME>) };
    return reply;
  }),
  http.get("*/api/me/notification-prefs", () => HttpResponse.json(prefs)),
  http.put("*/api/me/notification-prefs", async ({ request }) => {
    const body = (await request.json()) as typeof PREFS;
    prefPuts.push(body);
    prefs = { ...body, email_available: prefs.email_available };
    return HttpResponse.json(prefs);
  }),
);

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  me = ME;
  patches = [];
  patchReply = () => HttpResponse.json(me);
  prefs = PREFS;
  prefPuts = [];
  setCsrfToken("csrf-1");
});
afterEach(() => server.resetHandlers());

function renderPage() {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ToastProvider>
        <ProfilePage />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

it("informations : sans bloc securite redondant ni donnees de facturation", async () => {
  renderPage();
  expect(await screen.findByText("Propriétaire")).toBeInTheDocument();
  expect(screen.getAllByText("alice@example.com").length).toBeGreaterThan(0);
  expect(screen.getByRole("img", { name: "Alice Martin" })).toHaveTextContent("AM");
  const nav = screen.getByRole("navigation", { name: "Profil" });
  expect(within(nav).getAllByRole("button").map((b) => b.textContent)).toEqual(["Informations du compte", "Sécurité", "Notifications"]);
  for (const gone of ["Sessions actives", "Zone sensible", "Numéro de téléphone", "Devise du compte", "E-mail de récupération", "Activité du compte"]) {
    expect(screen.queryByText(gone)).not.toBeInTheDocument();
  }
  expect(screen.getByRole("combobox", { name: "Langue" })).toHaveValue("fr");
  expect(screen.getByText("Europe/Zurich")).toBeInTheDocument();
});

it("nom, langue et fuseau : enregistres par PATCH /api/me", async () => {
  renderPage();
  fireEvent.click(await screen.findByRole("button", { name: "Modifier nom" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Nom" }), { target: { value: "Alice Durand" } });
  fireEvent.click(screen.getByRole("button", { name: "Enregistrer" }));
  await waitFor(() => expect(patches).toEqual([{ csrf: "csrf-1", body: { name: "Alice Durand" } }]));
  expect((await screen.findAllByText("Alice Durand")).length).toBeGreaterThan(0);

  fireEvent.change(screen.getByRole("combobox", { name: "Langue" }), { target: { value: "en" } });
  await waitFor(() => expect(patches.at(-1)?.body).toEqual({ language: "en" }));

  patchReply = () => HttpResponse.json({ error: { code: "validation_failed", message: "Fuseau horaire inconnu." } }, { status: 422 });
  fireEvent.click(screen.getByRole("button", { name: "Modifier fuseau horaire" }));
  fireEvent.change(screen.getByRole("combobox", { name: "Fuseau horaire" }), { target: { value: "Mars/Olympus" } });
  fireEvent.click(screen.getByRole("button", { name: "Enregistrer" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Fuseau horaire inconnu.");
  expect(screen.getByRole("combobox", { name: "Fuseau horaire" })).toBeInTheDocument();
});

it("e-mail : le mot de passe actuel est redemande", async () => {
  renderPage();
  fireEvent.click(await screen.findByRole("button", { name: "Modifier adresse e-mail" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Adresse e-mail" }), { target: { value: "alice@new.example.com" } });
  fireEvent.click(screen.getByRole("button", { name: "Enregistrer" }));
  const dialog = await screen.findByRole("dialog");
  expect(patches).toEqual([]);
  fireEvent.change(within(dialog).getByLabelText("Mot de passe actuel"), { target: { value: "mon mot de passe" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Confirmer" }));
  await waitFor(() => expect(patches).toEqual([{ csrf: "csrf-1", body: { email: "alice@new.example.com", current_password: "mon mot de passe" } }]));
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
});

it("notifications : canal e-mail seulement, chaque case enregistree", async () => {
  renderPage();
  fireEvent.click(await screen.findByRole("button", { name: "Notifications" }));
  const team = await screen.findByRole("checkbox", { name: "Équipe par e-mail" });
  expect(team).toHaveAttribute("aria-checked", "true");
  for (const gone of ["Dashboard", "Ne pas déranger", "Rapport hebdomadaire", "Alertes de sécurité"]) {
    expect(screen.queryByText(gone)).not.toBeInTheDocument();
  }
  expect(screen.getByText("3 / 4 événements")).toBeInTheDocument();
  fireEvent.click(team);
  await waitFor(() => expect(prefPuts).toEqual([{ email_enabled: true, deploy_succeeded: true, deploy_failed: true, certificates: false, team: false }]));
  expect(await screen.findByText("2 / 4 événements")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("switch", { name: "Notifications par e-mail" }));
  await waitFor(() => expect(prefPuts.at(-1)).toMatchObject({ email_enabled: false }));
  await waitFor(() => expect(screen.getByRole("checkbox", { name: "Équipe par e-mail" })).toBeDisabled());
});

it("notifications : etat de chargement puis erreur avec nouvel essai (jamais d'onglet vide)", async () => {
  let fail = true;
  server.use(
    http.get("*/api/me/notification-prefs", async () => {
      await delay(30);
      return fail ? HttpResponse.json({ error: "internal", message: "x" }, { status: 500 }) : HttpResponse.json(prefs);
    }),
  );
  renderPage();
  fireEvent.click(await screen.findByRole("button", { name: "Notifications" }));
  expect(await screen.findByRole("status")).toHaveTextContent("Chargement…");
  expect(await screen.findByRole("alert")).toHaveTextContent("Impossible de charger vos préférences.");
  fail = false;
  fireEvent.click(screen.getByRole("button", { name: "Réessayer" }));
  expect(await screen.findByRole("checkbox", { name: "Équipe par e-mail" })).toHaveAttribute("aria-checked", "true");
});

it("notifications : deux clics rapides, enregistrements en serie et etat final du dernier clic", async () => {
  let inFlight = 0;
  let maxInFlight = 0;
  server.use(
    http.put("*/api/me/notification-prefs", async ({ request }) => {
      inFlight += 1;
      maxInFlight = Math.max(maxInFlight, inFlight);
      const body = (await request.json()) as typeof PREFS;
      prefPuts.push(body);
      await delay(prefPuts.length === 1 ? 60 : 5);
      prefs = { ...body, email_available: prefs.email_available };
      inFlight -= 1;
      return HttpResponse.json(prefs);
    }),
  );
  renderPage();
  fireEvent.click(await screen.findByRole("button", { name: "Notifications" }));
  fireEvent.click(await screen.findByRole("checkbox", { name: "Équipe par e-mail" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Certificats par e-mail" }));
  await waitFor(() => expect(prefPuts).toHaveLength(2));
  await waitFor(() => expect(inFlight).toBe(0));
  expect(maxInFlight).toBe(1);
  expect(prefPuts).toEqual([
    { email_enabled: true, deploy_succeeded: true, deploy_failed: true, certificates: false, team: false },
    { email_enabled: true, deploy_succeeded: true, deploy_failed: true, certificates: true, team: false },
  ]);
  await waitFor(() => expect(screen.getByRole("checkbox", { name: "Certificats par e-mail" })).toHaveAttribute("aria-checked", "true"));
  expect(screen.getByRole("checkbox", { name: "Équipe par e-mail" })).toHaveAttribute("aria-checked", "false");
  expect(screen.getByText("3 / 4 événements")).toBeInTheDocument();
});

it("notifications : avertit quand l'instance ne peut pas envoyer d'e-mails", async () => {
  prefs = { ...PREFS, email_available: false };
  renderPage();
  fireEvent.click(await screen.findByRole("button", { name: "Notifications" }));
  expect(await screen.findByRole("note")).toHaveTextContent("Paramètres › Notifications");
});
