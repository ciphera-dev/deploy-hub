import { createMemoryHistory } from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";

import { AppProviders, createAppQueryClient } from "@/app/providers";
import { createAppRouter } from "@/app/router";
import { setCsrfToken } from "@/lib/api/csrf";

const OWNER = { id: "u1", name: "Alice Martin", email: "alice@example.com", role: "owner", csrf_token: "csrf-1" };
const GENERAL = { id: "7f2a4c10-0000-4000-8000-000000000000", name: "Acme", slug: "acme", timezone: "Europe/Zurich", language: "fr", members: 6, mfa_required: false };

let me: Record<string, unknown>;
let security: { mfa_required: boolean; ip_allowlist: string[]; current_ip: string; self_enrollment_required: boolean };
let selfEnrollment: boolean;
let panel: { domain: string | null; status: string; error: string | null };
let requests: { method: string; path: string; body: unknown; csrf: string | null }[];

const record = async (request: Request) => {
  const text = await request.text();
  requests.push({ method: request.method, path: new URL(request.url).pathname, body: text ? JSON.parse(text) : null, csrf: request.headers.get("x-csrf-token") });
};

const server = setupServer(
  http.get("*/api/me", () => HttpResponse.json(me)),
  http.get("*/api/deployments", () => HttpResponse.json([])),
  http.get("*/api/settings/general", () => HttpResponse.json(GENERAL)),
  http.patch("*/api/settings/general", async ({ request }) => {
    await record(request);
    return HttpResponse.json({ ...GENERAL, ...(requests.at(-1)!.body as object) });
  }),
  http.get("*/api/settings/deployments", () => HttpResponse.json({ auto: true, retention: 5 })),
  http.patch("*/api/settings/deployments", async ({ request }) => {
    await record(request);
    return HttpResponse.json({ auto: (requests.at(-1)!.body as { auto: boolean }).auto, retention: 5 });
  }),
  http.get("*/api/settings/security", () => HttpResponse.json(security)),
  http.patch("*/api/settings/security", async ({ request }) => {
    await record(request);
    const { current_password: _password, ...body } = requests.at(-1)!.body as { mfa_required?: boolean; ip_allowlist?: string[]; current_password?: string };
    if (body.ip_allowlist?.length && !body.ip_allowlist.includes(security.current_ip)) {
      return HttpResponse.json({ error: { code: "ip_allowlist_lockout", message: `Cette liste exclut votre adresse actuelle (${security.current_ip}) : ajoutez-la pour ne pas perdre l'accès.` } }, { status: 422 });
    }
    security = { ...security, ...body };
    return HttpResponse.json({ ...security, self_enrollment_required: selfEnrollment });
  }),
  http.get("*/api/settings/danger", () => HttpResponse.json({ name: "Acme", projects: 3, members: 6, github_app: "Ciphera X" })),
  http.get("*/api/settings/panel-domain", () => HttpResponse.json(panel)),
  http.post("*/api/settings/panel-domain/recheck", async ({ request }) => {
    await record(request);
    if (panel.status === "error") return HttpResponse.json({ error: { code: "dns_error", message: "panel.example.org pointe vers 198.51.100.9 au lieu de 203.0.113.7." } }, { status: 422 });
    panel = { ...panel, error: null };
    return HttpResponse.json(panel, { status: 202 });
  }),
  http.delete("*/api/settings/panel-domain", async ({ request }) => {
    await record(request);
    return new HttpResponse(null, { status: 204 });
  }),
  http.get("*/api/settings/notifications", () =>
    HttpResponse.json({ configured: false, enabled: false, host: "", port: 587, tls: "starttls", username: "", password_set: false, from_address: "", from_name: "Ciphera", public_url: null, email_available: false }),
  ),
  http.post("*/api/variables", async ({ request }) => {
    await record(request);
    return HttpResponse.json({ id: "v1", key: "API_KEY", secret: true, value: null, environments: ["production"], all_projects: true, projects: [], updated_at: 0 }, { status: 201 });
  }),
  http.post("*/api/organization/export", async ({ request }) => {
    await record(request);
    return HttpResponse.json({ format: "ciphera-export", organization: { name: "Acme" } });
  }),
  http.delete("*/api/organization", async ({ request }) => {
    await record(request);
    const body = requests.at(-1)!.body as { password: string };
    if (body.password !== "bon mot de passe") return HttpResponse.json({ error: { code: "invalid_credentials", message: "Mot de passe actuel incorrect." } }, { status: 401 });
    return HttpResponse.json({ complete: false, projects_deleted: 2, remaining_projects: [{ id: "p3", name: "edge" }], github_app: { name: "Ciphera X", html_url: "https://github.com/apps/ciphera-x" } });
  }),
);

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  setCsrfToken("csrf-1");
  me = { ...OWNER };
  security = { mfa_required: false, ip_allowlist: [], current_ip: "203.0.113.7", self_enrollment_required: false };
  selfEnrollment = false;
  panel = { domain: null, status: "none", error: null };
  requests = [];
});
afterEach(() => server.resetHandlers());

function mount(url: string) {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [url] }));
  render(<AppProviders router={router} client={createAppQueryClient(router)} />);
  return router;
}

const nav = () => screen.findByRole("navigation", { name: "Sections des paramètres" }, { timeout: 5000 });

it("/settings ouvre Général ; en-tête et navigation latérale du propriétaire (sept sections)", async () => {
  const router = mount("/settings");
  const sections = await nav();
  await waitFor(() => expect(router.state.location.pathname).toBe("/settings/general"));
  expect(within(sections).getAllByRole("link").map((a) => a.textContent)).toEqual(["Général", "Domaine du panel", "Déploiements", "Notifications", "Sécurité", "Équipe", "Zone de danger"]);
  expect(within(sections).getByRole("link", { name: "Général" })).toHaveAttribute("aria-current", "page");
  expect(within(sections).getByRole("link", { name: "Zone de danger" })).toHaveAttribute("href", "/settings/danger");
  // En-tête : organisation, membres, fuseau, 2FA.
  expect(await screen.findByText("Membres :")).toBeInTheDocument();
  expect(screen.getByText("6")).toBeInTheDocument();
  expect(screen.getByText("facultative")).toBeInTheDocument();
  expect(screen.getByLabelText("Nom de l'organisation")).toHaveValue("Acme");

  fireEvent.change(screen.getByLabelText("Nom de l'organisation"), { target: { value: "Acme SA" } });
  // « Enregistrer » de l'en-tête soumet la section affichée.
  fireEvent.click(screen.getAllByRole("button", { name: "Enregistrer" })[0]!);
  await waitFor(() => expect(requests).toHaveLength(1));
  expect(requests[0]).toMatchObject({ method: "PATCH", path: "/api/settings/general", csrf: "csrf-1", body: { name: "Acme SA", slug: "acme", timezone: "Europe/Zurich", language: "fr" } });
});

it("administrateur : ni domaine du panel ni zone de danger ; autres rôles : accès refusé", async () => {
  me = { ...OWNER, role: "admin" };
  mount("/settings/security");
  const sections = await nav();
  expect(within(sections).getAllByRole("link").map((a) => a.textContent)).toEqual(["Général", "Déploiements", "Notifications", "Sécurité", "Équipe"]);
  cleanupAndMount({ ...OWNER, role: "developer" });
  expect(await screen.findByText(/les paramètres sont réservés aux administrateurs/, undefined, { timeout: 5000 })).toBeInTheDocument();
  expect(screen.queryByRole("navigation", { name: "Sections des paramètres" })).toBeNull();
});

function cleanupAndMount(next: Record<string, unknown>) {
  document.body.innerHTML = "";
  me = next;
  mount("/settings/general");
}

it("sécurité : 2FA obligatoire, SSO « SAML ou OIDC » désactivé, liste d'IP avec garde-fou et mot de passe", async () => {
  mount("/settings/security");
  const mfa = await screen.findByRole("switch", { name: "Double authentification obligatoire" }, { timeout: 5000 });
  expect(screen.getByText("SAML ou OIDC.")).toBeInTheDocument();
  expect(screen.getByRole("switch", { name: "Authentification unique (SSO)" })).toBeDisabled();
  expect(screen.getAllByText("Disponible en v0.2").length).toBeGreaterThan(0);

  fireEvent.click(mfa);
  fireEvent.click(screen.getByRole("switch", { name: "Restreindre l'accès par IP" }));
  fireEvent.change(screen.getByLabelText("Adresses autorisées"), { target: { value: "198.51.100.0/24" } });
  const save = screen.getAllByRole("button", { name: "Enregistrer" }).at(-1)!;
  // Modifier la liste exige le mot de passe : rien n'est envoyé sans lui.
  fireEvent.click(save);
  expect(await screen.findByText("Saisissez votre mot de passe actuel pour confirmer.")).toBeInTheDocument();
  expect(requests).toHaveLength(0);
  fireEvent.change(screen.getByLabelText("Mot de passe actuel"), { target: { value: "bon mot de passe" } });
  fireEvent.click(save);
  expect(await screen.findByText(/exclut votre adresse actuelle \(203\.0\.113\.7\)/)).toBeInTheDocument();
  expect(requests[0]!.body).toEqual({ mfa_required: true, ip_allowlist: ["198.51.100.0/24"], current_password: "bon mot de passe" });

  fireEvent.click(screen.getByRole("button", { name: "Ajouter mon adresse" }));
  expect(screen.getByLabelText("Adresses autorisées")).toHaveValue("198.51.100.0/24\n203.0.113.7");
  fireEvent.click(save);
  expect(await screen.findByText("Paramètres enregistrés")).toBeInTheDocument();
  expect(requests[1]!.body).toEqual({ mfa_required: true, ip_allowlist: ["198.51.100.0/24", "203.0.113.7"], current_password: "bon mot de passe" });
  expect(screen.getByText(/^sudo ciphera admin clear-ip-allowlist$/)).toBeInTheDocument();

  // Restriction désactivée : liste vide envoyée, mot de passe redemandé.
  fireEvent.click(screen.getByRole("switch", { name: "Restreindre l'accès par IP" }));
  fireEvent.change(await screen.findByLabelText("Mot de passe actuel"), { target: { value: "bon mot de passe" } });
  fireEvent.click(save);
  await waitFor(() => expect(requests).toHaveLength(3));
  expect(requests[2]!.body).toEqual({ ip_allowlist: [], current_password: "bon mot de passe" });
});

it("sécurité : un administrateur ne peut ni désactiver la 2FA ni modifier la liste d'IP", async () => {
  me = { ...OWNER, role: "admin" };
  security = { ...security, mfa_required: true };
  mount("/settings/security");
  expect(await screen.findByRole("switch", { name: "Double authentification obligatoire" }, { timeout: 5000 })).toBeDisabled();
  expect(screen.getByRole("switch", { name: "Restreindre l'accès par IP" })).toBeDisabled();
  expect(screen.getByText("La liste d'IP et la désactivation de la 2FA sont réservées au propriétaire.")).toBeInTheDocument();
  expect(screen.queryByLabelText("Mot de passe actuel")).toBeNull();
});

it("sécurité : imposer la 2FA sans second facteur redirige vers l'enrôlement", async () => {
  selfEnrollment = true;
  const router = mount("/settings/security");
  const paths: string[] = [];
  router.subscribe("onBeforeNavigate", (e) => paths.push(e.toLocation.pathname));
  fireEvent.click(await screen.findByRole("switch", { name: "Double authentification obligatoire" }, { timeout: 5000 }));
  fireEvent.click(screen.getAllByRole("button", { name: "Enregistrer" }).at(-1)!);
  await waitFor(() => expect(paths).toContain("/2fa-setup"));
  // Imposer la 2FA ne demande pas de mot de passe.
  expect(requests[0]!.body).toEqual({ mfa_required: true });
});

it("déploiements : rétention de 5, previews en v0.2, secret ajouté par la modale (variable globale secrète)", async () => {
  mount("/settings/deployments");
  expect(await screen.findByText("5 derniers déploiements réussis", undefined, { timeout: 5000 })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Gérer les secrets" }));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText("Ajouter un secret")).toBeInTheDocument();
  fireEvent.change(within(dialog).getByLabelText("Nom"), { target: { value: "api_key" } });
  fireEvent.change(within(dialog).getByLabelText("Valeur"), { target: { value: "s3cr3t" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Enregistrer" }));
  expect(await screen.findByText("Secret API_KEY enregistré")).toBeInTheDocument();
  expect(requests[0]).toMatchObject({ method: "POST", path: "/api/variables", body: { key: "API_KEY", value: "s3cr3t", secret: true, all_projects: true } });
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("zone de danger : export JSON, transfert en v0.2, suppression avec mot de passe et SUPPRIMER", async () => {
  const createObjectURL = vi.fn(() => "blob:x");
  Object.assign(URL, { createObjectURL, revokeObjectURL: vi.fn() });
  // Téléchargement : pas de navigation dans jsdom.
  const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
  mount("/settings/danger");
  expect(await screen.findByText("Transférer l'organisation", undefined, { timeout: 5000 })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Transférer" })).toBeDisabled();
  // Pas de bouton « Enregistrer » dans l'en-tête de la zone de danger.
  expect(screen.queryByRole("button", { name: "Enregistrer" })).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: "Exporter" }));
  expect(await screen.findByText("Export JSON téléchargé")).toBeInTheDocument();
  expect(requests[0]).toMatchObject({ method: "POST", path: "/api/organization/export", csrf: "csrf-1" });
  expect(createObjectURL).toHaveBeenCalled();
  expect(click).toHaveBeenCalled();
  click.mockRestore();

  fireEvent.click(screen.getByRole("button", { name: "Supprimer" }));
  const dialog = await screen.findByRole("dialog");
  const confirm = within(dialog).getByRole("button", { name: "Supprimer définitivement" });
  expect(confirm).toBeDisabled();
  fireEvent.change(within(dialog).getByLabelText("Mot de passe du compte"), { target: { value: "mauvais" } });
  fireEvent.change(within(dialog).getByPlaceholderText("SUPPRIMER"), { target: { value: "supprimer" } });
  expect(confirm).toBeDisabled();
  fireEvent.change(within(dialog).getByPlaceholderText("SUPPRIMER"), { target: { value: "SUPPRIMER" } });
  expect(confirm).toBeEnabled();
  fireEvent.click(confirm);
  expect(await within(dialog).findByText("Mot de passe actuel incorrect.")).toBeInTheDocument();
  expect(requests[1]).toMatchObject({ method: "DELETE", path: "/api/organization", body: { password: "mauvais", confirm: "SUPPRIMER" } });

  fireEvent.change(within(dialog).getByLabelText("Mot de passe du compte"), { target: { value: "bon mot de passe" } });
  fireEvent.click(within(dialog).getByRole("button", { name: "Supprimer définitivement" }));
  expect(await screen.findByText("Suppression incomplète")).toBeInTheDocument();
  expect(screen.getByText("edge")).toBeInTheDocument();
  expect(screen.getByText(/GitHub App à supprimer sur GitHub/)).toBeInTheDocument();
  expect(screen.getByRole("link", { name: "https://github.com/apps/ciphera-x" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Se reconnecter" })).toBeInTheDocument();
});

it("domaine du panel : confirmation depuis le domaine, retrait qui ramène l'accès 8443", async () => {
  panel = { domain: "panel.example.org", status: "awaiting_confirmation", error: null };
  mount("/settings/panel-domain");
  expect(await screen.findByLabelText("Domaine du panel", undefined, { timeout: 5000 })).toHaveValue("panel.example.org");
  expect(screen.getByText("Certificat valide")).toBeInTheDocument();
  expect(screen.getByRole("link", { name: "https://panel.example.org/settings/panel-domain" })).toBeInTheDocument();
  expect(screen.getByRole("radio", { name: /Accès privé \(VPN\)/ })).toHaveAttribute("aria-disabled", "true");

  fireEvent.click(screen.getByRole("button", { name: "Annuler la bascule" }));
  const dialog = await screen.findByRole("dialog");
  fireEvent.click(within(dialog).getByRole("button", { name: "Annuler la bascule" }));
  expect(await screen.findByText("Domaine du panneau retiré")).toBeInTheDocument();
  expect(requests[0]).toMatchObject({ method: "DELETE", path: "/api/settings/panel-domain", csrf: "csrf-1" });
});

it("domaine du panel confirmé : l'erreur de vérification s'affiche et « Revérifier » appelle la route", async () => {
  panel = { domain: "panel.example.org", status: "active", error: "Résolution DNS impossible pour le moment : nouvel essai automatique." };
  mount("/settings/panel-domain");
  expect(await screen.findByText(/Dernière vérification en échec : Résolution DNS impossible/, undefined, { timeout: 5000 })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Revérifier" }));
  expect(await screen.findByText("Vérification relancée")).toBeInTheDocument();
  expect(requests[0]).toMatchObject({ method: "POST", path: "/api/settings/panel-domain/recheck", csrf: "csrf-1" });
  await waitFor(() => expect(screen.queryByText(/Dernière vérification en échec/)).not.toBeInTheDocument());
});

it("domaine du panel : « Revérifier » refusé (DNS faux) affiche l'erreur de l'API", async () => {
  panel = { domain: "panel.example.org", status: "error", error: "ancienne erreur" };
  mount("/settings/panel-domain");
  fireEvent.click(await screen.findByRole("button", { name: "Revérifier" }, { timeout: 5000 }));
  expect(await screen.findByText(/pointe vers 198\.51\.100\.9/)).toBeInTheDocument();
});

it("notifications : la section SMTP est montée dans la page", async () => {
  mount("/settings/notifications");
  expect(await screen.findByLabelText("Serveur SMTP", undefined, { timeout: 5000 })).toBeInTheDocument();
  expect(screen.getByRole("link", { name: "Notifications" })).toHaveAttribute("aria-current", "page");
});
