import { http, HttpResponse, type RequestHandler } from "msw";

import { OVERVIEW } from "./fixtures/dashboard";
import { DEPLOYMENTS, queuedDeployment } from "./fixtures/deployments";
import { WEB_DOMAINS } from "./fixtures/domains";
import { createdHook, DEPLOY_HOOKS, gitSettings } from "./fixtures/git";
import { GITHUB_DISCONNECTED } from "./fixtures/github";
import { emptyOverview, WEB_OVERVIEW, WEB_SERVICES } from "./fixtures/overview";
import { PROJECT_SUMMARIES, projectDetail } from "./fixtures/projects";
import { DANGER, DEPLOY, GENERAL, NOTIFICATIONS, PANEL_DOMAIN, SECURITY, TEAM_INVITATIONS, TEAM_MEMBERS } from "./fixtures/settings";
import { exportedGlobals, GLOBAL_VARIABLES, PROJECT_VARIABLES } from "./fixtures/variables";

const NOT_FOUND = { error: { code: "not_found", message: "Déploiement introuvable." } };

// Les handlers des ecrans sont ajoutes avec chaque ecran ; les donnees factices vivent dans ./fixtures.
export const handlers: RequestHandler[] = [
  http.get("*/api/overview", ({ request }) => HttpResponse.json({ ...OVERVIEW, period: new URL(request.url).searchParams.get("period") === "7d" ? "7d" : "30d" })),
  http.get("*/api/domains", () => HttpResponse.json(WEB_DOMAINS)),
  http.patch("*/api/domains", () => HttpResponse.json(WEB_DOMAINS)),
  http.post("*/api/domains", () => HttpResponse.json({ ...WEB_DOMAINS.domains[2]!, id: "00000000-0000-4000-8000-0000000000df", host: "new.ciphera.dev", path_prefix: "/", dns_status: "pending", dns_error: null }, { status: 201 })),
  http.post("*/api/domains/:id/verify", ({ params }) => HttpResponse.json(WEB_DOMAINS.domains.find((d) => d.id === params.id) ?? WEB_DOMAINS.domains[0])),
  http.post("*/api/domains/:id/renew", ({ params }) => HttpResponse.json({ ...(WEB_DOMAINS.domains.find((d) => d.id === params.id) ?? WEB_DOMAINS.domains[0]!), cert_status: "issuing" }, { status: 202 })),
  http.post("*/api/domains/:id/primary", () => HttpResponse.json(WEB_DOMAINS)),
  http.post("*/api/domains/:id/redirect", ({ params }) => HttpResponse.json(WEB_DOMAINS.domains.find((d) => d.id === params.id) ?? WEB_DOMAINS.domains[0])),
  http.patch("*/api/domains/:id", ({ params }) => HttpResponse.json(WEB_DOMAINS.domains.find((d) => d.id === params.id) ?? WEB_DOMAINS.domains[0])),
  http.delete("*/api/domains/:id", () => new HttpResponse(null, { status: 204 })),
  http.get("*/api/projects/:id/variables/status", () => HttpResponse.json({ pending_apply: true })),
  http.get("*/api/projects/:id/variables", () => HttpResponse.json(PROJECT_VARIABLES)),
  http.put("*/api/projects/:id/variables", () => HttpResponse.json(PROJECT_VARIABLES)),
  http.put("*/api/projects/:id/variables/bulk", () => HttpResponse.json(PROJECT_VARIABLES)),
  http.delete("*/api/projects/:id/variables/:key", () => new HttpResponse(null, { status: 204 })),
  http.get("*/api/variables/export", ({ request }) => new HttpResponse(exportedGlobals(new URL(request.url).searchParams.get("env")), { headers: { "content-type": "text/plain; charset=utf-8" } })),
  http.get("*/api/variables", () => HttpResponse.json(GLOBAL_VARIABLES)),
  http.post("*/api/variables", () => HttpResponse.json(GLOBAL_VARIABLES[0], { status: 201 })),
  http.patch("*/api/variables/:id", ({ params }) => HttpResponse.json(GLOBAL_VARIABLES.find((v) => v.id === params.id) ?? GLOBAL_VARIABLES[0])),
  http.delete("*/api/variables/:id", () => new HttpResponse(null, { status: 204 })),
  http.get("*/api/connectors/github", () => HttpResponse.json(GITHUB_DISCONNECTED)),
  http.get("*/api/git/providers/github/repos", () => HttpResponse.json({ error: { code: "github_not_connected", message: "GitHub n'est pas connecté." } }, { status: 404 })),
  http.get("*/api/projects", () => HttpResponse.json(PROJECT_SUMMARIES)),
  http.get("*/api/projects/:id", ({ params }) => {
    const found = PROJECT_SUMMARIES.find((p) => p.id === params.id);
    return found ? HttpResponse.json(projectDetail(found)) : HttpResponse.json({ error: { code: "not_found", message: "Projet introuvable." } }, { status: 404 });
  }),
  http.get("*/api/deployments", ({ request }) => {
    const url = new URL(request.url);
    const project = url.searchParams.get("project");
    const status = url.searchParams.get("status");
    return HttpResponse.json(DEPLOYMENTS.filter((d) => (!project || d.project_id === project) && (!status || d.status === status)));
  }),
  http.get("*/api/deployments/:id", ({ params }) => {
    const found = DEPLOYMENTS.find((d) => d.id === params.id);
    return found ? HttpResponse.json(found) : HttpResponse.json(NOT_FOUND, { status: 404 });
  }),
  http.get("*/api/projects/:id/git", ({ params }) => {
    const found = PROJECT_SUMMARIES.find((p) => p.id === params.id);
    if (!found) return HttpResponse.json({ error: { code: "not_found", message: "Projet introuvable." } }, { status: 404 });
    if (found.source.type !== "git") return HttpResponse.json({ error: { code: "not_git", message: "Ce projet déploie une image : il n'a pas de réglages Git." } }, { status: 409 });
    return HttpResponse.json(gitSettings(found.source.repository, found.source.branch));
  }),
  http.patch("*/api/projects/:id/git", async ({ params, request }) => {
    const found = PROJECT_SUMMARIES.find((p) => p.id === params.id);
    if (!found || found.source.type !== "git") return HttpResponse.json({ error: { code: "not_found", message: "Projet introuvable." } }, { status: 404 });
    const patch = (await request.json()) as Record<string, unknown>;
    return HttpResponse.json({ ...gitSettings(found.source.repository, found.source.branch), ...patch });
  }),
  http.get("*/api/projects/:id/deploy-hooks", () => HttpResponse.json(DEPLOY_HOOKS)),
  http.post("*/api/projects/:id/deploy-hooks", async ({ request }) => HttpResponse.json(createdHook(((await request.json()) as { name: string }).name), { status: 201 })),
  http.delete("*/api/projects/:id/deploy-hooks/:hookId", () => new HttpResponse(null, { status: 204 })),
  http.post("*/api/projects/:id/restart", () => HttpResponse.json({ deployment_id: DEPLOYMENTS[0].id }, { status: 202 })),
  http.get("*/api/projects/:id/overview", ({ params }) => {
    const found = PROJECT_SUMMARIES.find((p) => p.id === params.id);
    if (!found) return HttpResponse.json({ error: { code: "not_found", message: "Projet introuvable." } }, { status: 404 });
    return HttpResponse.json(found.id === WEB_OVERVIEW.activity[0]?.project_id ? WEB_OVERVIEW : emptyOverview(serviceOf(found)));
  }),
  http.get("*/api/projects/:id/services", ({ params }) => {
    const found = PROJECT_SUMMARIES.find((p) => p.id === params.id);
    if (!found) return HttpResponse.json({ error: { code: "not_found", message: "Projet introuvable." } }, { status: 404 });
    return HttpResponse.json(found.id === WEB_OVERVIEW.activity[0]?.project_id ? WEB_SERVICES : [serviceOf(found)]);
  }),
  http.post("*/api/projects/:id/services/:sid/:action", () => HttpResponse.json({ deployment_id: DEPLOYMENTS[1].id }, { status: 202 })),
  http.post("*/api/projects/:id/redeploy", ({ params }) => HttpResponse.json(queuedDeployment(String(params.id)), { status: 201 })),
  http.get("*/api/settings/general", () => HttpResponse.json(GENERAL)),
  http.patch("*/api/settings/general", async ({ request }) => HttpResponse.json({ ...GENERAL, ...((await request.json()) as object) })),
  http.get("*/api/settings/deployments", () => HttpResponse.json(DEPLOY)),
  http.patch("*/api/settings/deployments", async ({ request }) => HttpResponse.json({ ...DEPLOY, ...((await request.json()) as object) })),
  http.get("*/api/settings/security", () => HttpResponse.json(SECURITY)),
  http.patch("*/api/settings/security", async ({ request }) => HttpResponse.json({ ...SECURITY, ...((await request.json()) as object) })),
  http.get("*/api/settings/danger", () => HttpResponse.json(DANGER)),
  http.get("*/api/settings/panel-domain", () => HttpResponse.json(PANEL_DOMAIN)),
  http.get("*/api/settings/notifications", () => HttpResponse.json(NOTIFICATIONS)),
  http.put("*/api/settings/notifications", () => HttpResponse.json(NOTIFICATIONS)),
  http.get("*/api/team/members", () => HttpResponse.json(TEAM_MEMBERS)),
  http.get("*/api/team/invitations", () => HttpResponse.json(TEAM_INVITATIONS)),
  http.post("*/api/team/invitations", () => HttpResponse.json({ link: "/invite/jeton-factice" }, { status: 201 })),
  http.delete("*/api/team/invitations/:id", () => new HttpResponse(null, { status: 204 })),
  http.patch("*/api/team/members/:id", () => new HttpResponse(null, { status: 204 })),
  http.delete("*/api/team/members/:id", () => HttpResponse.json({ deleted_hooks: 0, deleted_invitations: 0 })),
  http.post("*/api/team/members/:id/reset-link", () => HttpResponse.json({ link: "/reset/jeton-factice" }, { status: 201 })),
  http.post("*/api/organization/export", () => HttpResponse.json({ format: "ciphera-export", version: 1, organization: GENERAL })),
];

/** Service unique d'un projet de la liste, jamais déployé. */
function serviceOf(summary: (typeof PROJECT_SUMMARIES)[number]) {
  const service = projectDetail(summary).services[0]!;
  return { ...service, state: "none" as const, replicas: { running: 0, desired: 1 } };
}
