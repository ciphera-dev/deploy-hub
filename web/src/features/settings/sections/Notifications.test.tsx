import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it } from "vitest";

import { ToastProvider } from "@/design-system";

import { NotificationsSection } from "./Notifications";

const CONFIGURED = {
  configured: true,
  enabled: true,
  host: "smtp.example.com",
  port: 587,
  tls: "starttls",
  username: "ciphera@example.com",
  password_set: true,
  from_address: "ciphera@example.com",
  from_name: "Ciphera",
  public_url: "https://panel.example.com",
  email_available: true,
};

let current: Record<string, unknown>;
let putBodies: Record<string, unknown>[];
let putReply: (() => Response) | null;

const server = setupServer(
  http.get("*/api/settings/notifications", () => HttpResponse.json(current)),
  http.put("*/api/settings/notifications", async ({ request }) => {
    const body = (await request.json()) as Record<string, unknown>;
    putBodies.push(body);
    if (putReply) return putReply();
    const { password, ...rest } = body;
    current = { ...current, ...rest, configured: true, password_set: current.password_set || Boolean(password) };
    return HttpResponse.json(current);
  }),
);

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  current = { ...CONFIGURED };
  putBodies = [];
  putReply = null;
});
afterEach(() => server.resetHandlers());

function mount() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <NotificationsSection />
      </ToastProvider>
    </QueryClientProvider>,
  );
}

const type = (label: string, value: string) => fireEvent.change(screen.getByLabelText(label), { target: { value } });
const save = () => fireEvent.click(screen.getByRole("button", { name: "Enregistrer" }));

it("mot de passe en ecriture seule : jamais affiche, conserve si le champ reste vide", async () => {
  mount();
  expect(await screen.findByLabelText("Serveur SMTP")).toHaveValue("smtp.example.com");
  expect(screen.getByLabelText("Mot de passe SMTP")).toHaveValue("");
  expect(screen.getByText(/Un mot de passe est enregistré/)).toBeInTheDocument();
  expect(screen.getByText(/Les e-mails partent via smtp.example.com/)).toBeInTheDocument();

  type("Serveur SMTP", "mail.example.org");
  save();
  await waitFor(() => expect(putBodies).toHaveLength(1));
  expect(putBodies[0]).not.toHaveProperty("password");
  expect(putBodies[0]).toMatchObject({ host: "mail.example.org", port: 587, tls: "starttls", enabled: true });
  expect(await screen.findByText("Paramètres enregistrés")).toBeInTheDocument();
});

it("nouveau mot de passe et SSL/TLS : envoyes, le champ est vide ensuite", async () => {
  mount();
  await screen.findByLabelText("Serveur SMTP");
  fireEvent.click(screen.getByRole("radio", { name: "SSL/TLS" }));
  expect(screen.getByLabelText("Port")).toHaveValue("465");
  type("Mot de passe SMTP", "nouveau-test-secret");
  save();
  await waitFor(() => expect(putBodies).toHaveLength(1));
  expect(putBodies[0]).toMatchObject({ tls: "tls", port: 465, password: "nouveau-test-secret" });
  await waitFor(() => expect(screen.getByLabelText("Mot de passe SMTP")).toHaveValue(""));
  expect(screen.queryByRole("radio", { name: /Aucun/ })).not.toBeInTheDocument();
});

it("sans CIPHERA_PUBLIC_URL : les liens ne partent pas, le panneau le dit", async () => {
  current = { ...CONFIGURED, public_url: null, email_available: false };
  mount();
  expect(await screen.findByText(/CIPHERA_PUBLIC_URL/)).toBeInTheDocument();
});

it("refus du serveur : message affiche", async () => {
  putReply = () => HttpResponse.json({ error: { code: "validation_failed", message: "Serveur SMTP : nom d'hôte invalide." } }, { status: 422 });
  mount();
  await screen.findByLabelText("Serveur SMTP");
  save();
  expect(await screen.findByRole("alert")).toHaveTextContent("Serveur SMTP : nom d'hôte invalide.");
});

it("lecture refusee : etat d'acces refuse, pas de chargement sans fin", async () => {
  server.use(http.get("*/api/settings/notifications", () => HttpResponse.json({ error: { code: "forbidden", message: "Accès refusé." } }, { status: 403 })));
  mount();
  expect(await screen.findByRole("alert")).toHaveTextContent("Accès refusé");
  expect(screen.queryByText("Chargement des paramètres…")).toBeNull();
  expect(screen.queryByLabelText("Serveur SMTP")).toBeNull();
});

it("panne du serveur : message d'erreur", async () => {
  server.use(http.get("*/api/settings/notifications", () => new HttpResponse(null, { status: 500 })));
  mount();
  expect(await screen.findByRole("alert")).toHaveTextContent("Impossible de charger les paramètres");
});

it("role admin : section en lecture seule, rien n'est modifiable", async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <NotificationsSection readOnly />
      </ToastProvider>
    </QueryClientProvider>,
  );
  expect(await screen.findByLabelText("Serveur SMTP")).toBeDisabled();
  expect(screen.getByLabelText("Serveur SMTP")).toHaveValue("smtp.example.com");
  for (const label of ["Port", "Utilisateur SMTP", "Mot de passe SMTP", "Adresse d'expédition", "Nom affiché"]) {
    expect(screen.getByLabelText(label)).toBeDisabled();
  }
  expect(screen.getByRole("radio", { name: "STARTTLS" })).toBeDisabled();
  expect(screen.getByRole("switch", { name: "Envoi des e-mails" })).toBeDisabled();
  expect(screen.getByText("Réservé au propriétaire")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Enregistrer" })).not.toBeInTheDocument();
  expect(putBodies).toHaveLength(0);
});
