import { createMemoryHistory } from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it } from "vitest";

import { AppProviders, createAppQueryClient } from "@/app/providers";
import { createAppRouter } from "@/app/router";
import { setCsrfToken } from "@/lib/api/csrf";
import { WEB_DOMAINS } from "@/mocks/fixtures/domains";
import { PROJECT_SUMMARIES, projectDetail } from "@/mocks/fixtures/projects";

import { dnsRecords } from "./api";

const WEB = PROJECT_SUMMARIES[0];
const API = WEB_DOMAINS.domains[2]!;
let role = "owner";
let calls: { method: string; path: string; query: string; body: unknown; csrf: string | null }[] = [];

async function record(request: Request) {
  const url = new URL(request.url);
  const text = request.method === "GET" || request.method === "DELETE" ? "" : await request.text();
  calls.push({ method: request.method, path: url.pathname, query: url.search, body: text ? JSON.parse(text) : null, csrf: request.headers.get("x-csrf-token") });
}

const CREATED = { ...API, id: "00000000-0000-4000-8000-0000000000df", host: "shop.exemple.com", path_prefix: "/", dns_status: "pending", dns_error: null };

const server = setupServer(
  http.get("*/api/me", () => HttpResponse.json({ id: "u1", name: "Alice Martin", email: "alice@example.com", role, csrf_token: "csrf-1" })),
  http.get("*/api/deployments", () => HttpResponse.json([])),
  http.get("*/api/projects/:id", ({ params }) => HttpResponse.json(projectDetail(PROJECT_SUMMARIES.find((p) => p.id === params.id)!))),
  http.get("*/api/domains", () => HttpResponse.json(WEB_DOMAINS)),
  http.post("*/api/domains", async ({ request }) => {
    await record(request);
    return HttpResponse.json(CREATED, { status: 201 });
  }),
  http.post("*/api/domains/:id/verify", async ({ request }) => {
    await record(request);
    return HttpResponse.json({ ...CREATED, dns_status: "ok", cert_status: "issuing" });
  }),
  http.delete("*/api/domains/:id", async ({ request }) => {
    await record(request);
    return new HttpResponse(null, { status: 204 });
  }),
);

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  setCsrfToken("csrf-1");
  role = "owner";
  calls = [];
});
afterEach(() => server.resetHandlers());

function mount(url: string) {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [url] }));
  render(<AppProviders router={router} client={createAppQueryClient(router)} />);
  return router;
}

it("liste : principal, redirection, domaine généré en HTTP, DNS incorrect expliqué", async () => {
  mount(`/projects/${WEB.id}/domains`);
  const app = await screen.findByTestId("domain-app.ciphera.dev", {}, { timeout: 5000 });
  expect(within(app).getByText("Principal")).toBeInTheDocument();
  expect(within(app).getByText("HTTPS actif")).toBeInTheDocument();
  expect(within(screen.getByTestId("domain-ciphera.dev")).getByText("app.ciphera.dev")).toBeInTheDocument();
  const generated = screen.getByTestId("domain-ciphera-web.203.0.113.10.sslip.io");
  expect(within(generated).getByText("Généré")).toBeInTheDocument();
  expect(within(generated).getByText("HTTP (domaine généré)")).toBeInTheDocument();
  // Badge d'avertissement : le domaine généré n'a pas de certificat ; les autres n'en ont pas.
  expect(within(generated).getByText("HTTP, non chiffré")).toBeInTheDocument();
  expect(within(app).queryByText("HTTP, non chiffré")).not.toBeInTheDocument();
  const api = screen.getByTestId("domain-api.ciphera.dev");
  expect(within(api).getByText("DNS incorrect")).toBeInTheDocument();
  fireEvent.click(within(api).getByText("api.ciphera.dev"));
  expect(await within(api).findByText(/résout vers 198\.51\.100\.7/)).toBeInTheDocument();
  // Enregistrement A vers l'IP du nœud, nom relatif à la zone.
  expect(within(api).getByText("203.0.113.10")).toBeInTheDocument();
  expect(within(api).getByText("api")).toBeInTheDocument();
});

it("ajout : POST avec le jeton CSRF, enregistrements DNS, vérification puis certificat demandé", async () => {
  mount(`/projects/${WEB.id}/domains`);
  fireEvent.click(await screen.findByRole("button", { name: "Ajouter un domaine" }, { timeout: 5000 }));
  const dialog = await screen.findByRole("dialog");
  fireEvent.change(within(dialog).getByLabelText("Domaine"), { target: { value: "https://Shop.Exemple.com/x" } });
  fireEvent.change(within(dialog).getByLabelText("Port interne"), { target: { value: "8080" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Continuer" }));
  await waitFor(() => expect(calls).toHaveLength(1));
  expect(calls[0]).toMatchObject({ method: "POST", path: "/api/domains", query: `?project=${WEB.id}`, csrf: "csrf-1" });
  expect(calls[0]!.body).toMatchObject({ host: "shop.exemple.com", path_prefix: "/", internal_port: 8080, redirect: false, www: false });
  expect(await within(dialog).findByText("203.0.113.10")).toBeInTheDocument();
  expect(within(dialog).getByRole("button", { name: "Ajouter le domaine" })).toBeDisabled();
  fireEvent.click(within(dialog).getByRole("button", { name: "Vérifier le DNS" }));
  expect(await within(dialog).findByText("DNS propagé. Le certificat HTTPS est en cours d'émission.")).toBeInTheDocument();
  expect(calls[1]).toMatchObject({ method: "POST", path: `/api/domains/${CREATED.id}/verify`, csrf: "csrf-1" });
  expect(within(dialog).getByRole("button", { name: "Ajouter le domaine" })).toBeEnabled();
});

it("vérification : le certificat n'est dit « en cours d'émission » que s'il est demandé", async () => {
  server.use(
    http.post("*/api/domains/:id/verify", async ({ request }) => {
      await record(request);
      return HttpResponse.json({ ...CREATED, dns_status: "ok", cert_status: "active" });
    }),
  );
  mount(`/projects/${WEB.id}/domains`);
  fireEvent.click(await screen.findByRole("button", { name: "Ajouter un domaine" }, { timeout: 5000 }));
  const dialog = await screen.findByRole("dialog");
  fireEvent.change(within(dialog).getByLabelText("Domaine"), { target: { value: "shop.exemple.com" } });
  fireEvent.change(within(dialog).getByLabelText("Port interne"), { target: { value: "8080" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Continuer" }));
  fireEvent.click(await within(dialog).findByRole("button", { name: "Vérifier le DNS" }));
  expect(await within(dialog).findByText("DNS propagé. Certificat HTTPS actif.")).toBeInTheDocument();
  expect(within(dialog).queryByText(/en cours d'émission/)).not.toBeInTheDocument();
  fireEvent.keyDown(document.activeElement!, { key: "Escape" });

  // Domaine en erreur DNS corrigé, certificat toujours valide : pas d'émission annoncée.
  const moved = { ...WEB_DOMAINS.domains[0]!, dns_status: "error" as const, dns_error: "DNS incorrect." };
  server.use(
    http.get("*/api/domains", () => HttpResponse.json({ ...WEB_DOMAINS, domains: [moved] })),
    http.post("*/api/domains/:id/verify", () => HttpResponse.json({ ...moved, dns_status: "ok", dns_error: null, cert_status: "active" })),
  );
  const row = await screen.findByTestId("domain-app.ciphera.dev", {}, { timeout: 5000 });
  fireEvent.click(within(row).getByRole("button", { name: "Vérifier" }));
  expect(await screen.findByText("app.ciphera.dev : configuration correcte")).toBeInTheDocument();
  expect(screen.queryByText(/émission du certificat/)).not.toBeInTheDocument();
});

it("ajout : refus du serveur affiché, la fenêtre reste ouverte", async () => {
  server.use(http.post("*/api/domains", () => HttpResponse.json({ error: { code: "host_taken", message: "Ce domaine est déjà utilisé." } }, { status: 409 })));
  mount(`/projects/${WEB.id}/domains`);
  fireEvent.click(await screen.findByRole("button", { name: "Ajouter un domaine" }, { timeout: 5000 }));
  const dialog = await screen.findByRole("dialog");
  fireEvent.change(within(dialog).getByLabelText("Domaine"), { target: { value: "app.ciphera.dev" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Continuer" }));
  expect(await within(dialog).findByRole("alert")).toHaveTextContent("Ce domaine est déjà utilisé.");
  expect(within(dialog).getByRole("button", { name: "Continuer" })).toBeInTheDocument();
});

it("suppression : confirmation puis DELETE avec le jeton CSRF", async () => {
  mount(`/projects/${WEB.id}/domains`);
  const api = await screen.findByTestId("domain-api.ciphera.dev", {}, { timeout: 5000 });
  fireEvent.keyDown(within(api).getByRole("button", { name: "Actions pour api.ciphera.dev" }), { key: "Enter" });
  fireEvent.click(await screen.findByRole("menuitem", { name: /Supprimer le domaine/ }));
  const dialog = await screen.findByRole("dialog");
  fireEvent.click(within(dialog).getByRole("button", { name: "Supprimer" }));
  await waitFor(() => expect(calls).toHaveLength(1));
  expect(calls[0]).toMatchObject({ method: "DELETE", path: `/api/domains/${API.id}`, csrf: "csrf-1" });
  expect(await screen.findByText("api.ciphera.dev supprimé")).toBeInTheDocument();
});

it("lecteur : lecture seule, ni ajout ni vérification", async () => {
  role = "viewer";
  mount(`/projects/${WEB.id}/domains`);
  const app = await screen.findByTestId("domain-app.ciphera.dev", {}, { timeout: 5000 });
  await screen.findByText("Alice Martin");
  expect(screen.queryByRole("button", { name: "Ajouter un domaine" })).not.toBeInTheDocument();
  expect(within(app).queryByRole("button", { name: "Vérifier" })).not.toBeInTheDocument();
});

it("enregistrements DNS : apex sur « @ », AAAA pour une IP v6, les deux en double pile, rien sans IP", () => {
  expect(dnsRecords("exemple.com", "203.0.113.10")).toEqual([{ type: "A", host: "@", value: "203.0.113.10" }]);
  expect(dnsRecords("a.b.exemple.com", "2001:db8::1")).toEqual([{ type: "AAAA", host: "a.b", value: "2001:db8::1" }]);
  expect(dnsRecords("exemple.com", "203.0.113.10", "2001:db8::1")).toEqual([
    { type: "A", host: "@", value: "203.0.113.10" },
    { type: "AAAA", host: "@", value: "2001:db8::1" },
  ]);
  expect(dnsRecords("exemple.com", null)).toEqual([]);
});

it("DNS devenu incorrect : affiché même avec un certificat encore valide", async () => {
  const moved = { ...WEB_DOMAINS.domains[0]!, dns_status: "error" as const, dns_error: "app.ciphera.dev pointe vers 198.51.100.7 au lieu de 203.0.113.10 : corrigez l'enregistrement A." };
  server.use(http.get("*/api/domains", () => HttpResponse.json({ ...WEB_DOMAINS, domains: [moved] })));
  mount(`/projects/${WEB.id}/domains`);
  const app = await screen.findByTestId("domain-app.ciphera.dev", {}, { timeout: 5000 });
  expect(within(app).getByText("DNS incorrect")).toBeInTheDocument();
  expect(within(app).queryByText("HTTPS actif")).not.toBeInTheDocument();
});

it("menu : « Renouveler » absent pour un certificat valide à plus de 30 jours", async () => {
  const far = new Date(Date.now() + 80 * 86_400_000).toISOString();
  const near = new Date(Date.now() + 10 * 86_400_000).toISOString();
  const [app, , api] = WEB_DOMAINS.domains;
  server.use(
    http.get("*/api/domains", () =>
      HttpResponse.json({ ...WEB_DOMAINS, domains: [{ ...app!, cert_expires_at: far }, { ...api!, host: "near.ciphera.dev", dns_status: "ok", dns_error: null, cert_status: "active", cert_expires_at: near }] }),
    ),
  );
  mount(`/projects/${WEB.id}/domains`);
  const row = await screen.findByTestId("domain-app.ciphera.dev", {}, { timeout: 5000 });
  fireEvent.keyDown(within(row).getByRole("button", { name: "Actions pour app.ciphera.dev" }), { key: "Enter" });
  await screen.findByRole("menuitem", { name: /Modifier/ });
  expect(screen.queryByRole("menuitem", { name: /Renouveler/ })).not.toBeInTheDocument();
  fireEvent.keyDown(document.activeElement!, { key: "Escape" });
  const nearRow = screen.getByTestId("domain-near.ciphera.dev");
  fireEvent.keyDown(within(nearRow).getByRole("button", { name: "Actions pour near.ciphera.dev" }), { key: "Enter" });
  expect(await screen.findByRole("menuitem", { name: /Renouveler/ })).toBeInTheDocument();
});

it("domaine généré : badge HTTP retiré une fois le certificat émis", async () => {
  const issued = {
    ...WEB_DOMAINS,
    domains: WEB_DOMAINS.domains.map((x) => (x.generated ? { ...x, cert_status: "active" as const, cert_issuer: "Let's Encrypt R11", cert_expires_at: "2027-02-13T10:00:00.000Z" } : x)),
  };
  server.use(http.get("*/api/domains", () => HttpResponse.json(issued)));
  mount(`/projects/${WEB.id}/domains`);
  const generated = await screen.findByTestId("domain-ciphera-web.203.0.113.10.sslip.io", {}, { timeout: 5000 });
  expect(within(generated).getByText("Généré")).toBeInTheDocument();
  expect(within(generated).getByText("HTTPS actif")).toBeInTheDocument();
  expect(within(generated).queryByText("HTTP, non chiffré")).not.toBeInTheDocument();
});
