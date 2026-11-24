import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";

import { ToastProvider } from "@/design-system";
import { setCsrfToken } from "@/lib/api/csrf";
import { GITHUB_REPOS } from "@/mocks/fixtures/github";
import { DEPLOYMENTS, queuedDeployment } from "@/mocks/fixtures/deployments";
import { PROJECT_SUMMARIES, projectDetail } from "@/mocks/fixtures/projects";

import { NewProjectModal } from "./NewProjectModal";

let posts: { csrf: string | null; body: Record<string, unknown> }[];
let reply: () => Response;
let reposReply: () => Response;
let deployReply: () => Response;
let polled: () => Response;
let deployPosts: { path: string; csrf: string | null; body: unknown }[];
let securityPatches: { path: string; csrf: string | null; body: unknown }[];
let securityReply: () => Response;

const NOT_CONNECTED = () => HttpResponse.json({ error: { code: "github_not_connected", message: "GitHub n'est pas connecté." } }, { status: 404 });

const server = setupServer(
  http.post("*/api/projects", async ({ request }) => {
    posts.push({ csrf: request.headers.get("x-csrf-token"), body: (await request.json()) as Record<string, unknown> });
    return reply();
  }),
  http.get("*/api/git/providers/github/repos", () => reposReply()),
  http.post("*/api/projects/:id/redeploy", async ({ request }) => {
    deployPosts.push({ path: new URL(request.url).pathname, csrf: request.headers.get("x-csrf-token"), body: (await request.text()) || null });
    return deployReply();
  }),
  http.patch("*/api/projects/:id/services/:sid", async ({ request }) => {
    securityPatches.push({ path: new URL(request.url).pathname, csrf: request.headers.get("x-csrf-token"), body: await request.json() });
    return securityReply();
  }),
  http.get("*/api/deployments/:id", () => polled()),
);

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  posts = [];
  reply = () => HttpResponse.json(projectDetail(PROJECT_SUMMARIES[0]), { status: 201 });
  reposReply = NOT_CONNECTED;
  deployPosts = [];
  securityPatches = [];
  securityReply = () => HttpResponse.json({});
  deployReply = () => HttpResponse.json(queuedDeployment(PROJECT_SUMMARIES[0].id), { status: 201 });
  polled = () => HttpResponse.json(queuedDeployment(PROJECT_SUMMARIES[0].id));
  setCsrfToken("csrf-1");
});
afterEach(() => server.resetHandlers());

function renderModal() {
  const onClose = vi.fn();
  const onOpen = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ToastProvider>
        <NewProjectModal onClose={onClose} onOpen={onOpen} />
      </ToastProvider>
    </QueryClientProvider>,
  );
  return { onClose, onOpen };
}

/** Étape « Source » (GitHub) puis passage à « Configuration ». */
function pickRepository(repository = "https://github.com/ciphera/web") {
  fireEvent.change(screen.getByLabelText("Dépôt GitHub"), { target: { value: repository } });
  fireEvent.click(screen.getByRole("button", { name: "Continuer" }));
}

it("Railpack : envoie la source Git et le build, avec le jeton CSRF", async () => {
  const { onOpen } = renderModal();
  pickRepository();
  // Nom proposé depuis le dépôt.
  expect((screen.getByLabelText("Nom du projet") as HTMLInputElement).value).toBe("web");
  fireEvent.change(screen.getByLabelText("Branche"), { target: { value: "develop" } });
  fireEvent.click(screen.getByRole("button", { name: "Créer le projet" }));
  await waitFor(() => expect(posts).toHaveLength(1));
  expect(posts[0].csrf).toBe("csrf-1");
  expect(posts[0].body).toEqual({
    name: "web",
    source: { type: "git", repository: "https://github.com/ciphera/web", branch: "develop" },
    build: { kind: "railpack" },
  });
  // Le premier déploiement part à la création.
  expect(await screen.findByText("En file d'attente…")).toBeInTheDocument();
  expect(deployPosts).toEqual([{ path: `/api/projects/${PROJECT_SUMMARIES[0].id}/redeploy`, csrf: "csrf-1", body: null }]);
  fireEvent.click(screen.getByRole("button", { name: "Ouvrir le projet" }));
  expect(onOpen).toHaveBeenCalledWith(PROJECT_SUMMARIES[0].id);
});

it("Dockerfile : envoie le chemin du Dockerfile", async () => {
  renderModal();
  pickRepository("ciphera/api");
  fireEvent.click(screen.getByRole("button", { name: /Dockerfile/ }));
  fireEvent.change(screen.getByLabelText("Chemin du Dockerfile"), { target: { value: "docker/Dockerfile.prod" } });
  fireEvent.click(screen.getByRole("button", { name: "Créer le projet" }));
  await waitFor(() => expect(posts).toHaveLength(1));
  expect(posts[0].body).toEqual({
    name: "api",
    source: { type: "git", repository: "ciphera/api", branch: "main" },
    build: { kind: "dockerfile", path: "docker/Dockerfile.prod" },
  });
});

it("Compose : envoie le fichier et le service exposé", async () => {
  renderModal();
  pickRepository("ciphera/site");
  fireEvent.click(screen.getByRole("button", { name: /Compose/ }));
  fireEvent.change(screen.getByLabelText("Service exposé"), { target: { value: "web" } });
  fireEvent.click(screen.getByRole("button", { name: "Créer le projet" }));
  await waitFor(() => expect(posts).toHaveLength(1));
  expect(posts[0].body).toEqual({
    name: "site",
    source: { type: "git", repository: "ciphera/site", branch: "main" },
    build: { kind: "compose", path: "compose.yaml", service: "web" },
  });
});

it("Image Docker : pas de build dans le corps", async () => {
  renderModal();
  fireEvent.click(screen.getByRole("button", { name: /Image Docker/ }));
  fireEvent.change(screen.getByLabelText("Image"), { target: { value: "ghcr.io/org/app:1.2" } });
  fireEvent.click(screen.getByRole("button", { name: "Continuer" }));
  expect((screen.getByLabelText("Nom du projet") as HTMLInputElement).value).toBe("app");
  expect(screen.queryByLabelText("Branche")).not.toBeInTheDocument();
  expect(screen.queryByText("Méthode de build")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Créer le projet" }));
  await waitFor(() => expect(posts).toHaveLength(1));
  expect(posts[0].body).toEqual({ name: "app", source: { type: "image", image: "ghcr.io/org/app:1.2" } });
});

it("refus du serveur : le message s'affiche et la saisie est conservée", async () => {
  reply = () => HttpResponse.json({ error: { code: "name_taken", message: "Ce nom de projet est déjà utilisé." } }, { status: 409 });
  renderModal();
  pickRepository();
  fireEvent.click(screen.getByRole("button", { name: "Créer le projet" }));
  expect(await screen.findByText("Ce nom de projet est déjà utilisé.")).toBeInTheDocument();
  expect((screen.getByLabelText("Nom du projet") as HTMLInputElement).value).toBe("web");
  expect(screen.queryByText("Projet créé")).not.toBeInTheDocument();
});

it("les fournisseurs non pris en charge sont visibles mais désactivés, annoncés en v0.2", () => {
  renderModal();
  for (const name of ["GitLab", "Gitea", "Bitbucket", "URL Git"]) {
    const tile = screen.getByRole("button", { name: `${name} v0.2` });
    expect(tile).toBeDisabled();
    expect(tile).toHaveAttribute("title", "Disponible en v0.2");
  }
  expect(screen.getByRole("button", { name: "GitHub" })).toBeEnabled();
});

it("« Continuer » reste inactif sans dépôt", () => {
  renderModal();
  expect(screen.getByRole("button", { name: "Continuer" })).toBeDisabled();
});

it("GitHub App connectée : choisit un dépôt dans la liste (branche par défaut reprise)", async () => {
  reposReply = () => HttpResponse.json(GITHUB_REPOS);
  renderModal();
  expect(await screen.findByText("acme/web")).toBeInTheDocument();
  // La saisie libre laisse place à la liste.
  expect(screen.queryByLabelText("Dépôt GitHub")).not.toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Rechercher un dépôt"), { target: { value: "api" } });
  expect(screen.queryByText("acme/web")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Importer acme/api" }));
  expect((screen.getByLabelText("Nom du projet") as HTMLInputElement).value).toBe("api");
  expect((screen.getByLabelText("Branche") as HTMLInputElement).value).toBe("develop");
  fireEvent.click(screen.getByRole("button", { name: "Créer le projet" }));
  await waitFor(() => expect(posts).toHaveLength(1));
  expect(posts[0].body).toEqual({
    name: "api",
    source: { type: "git", repository: "acme/api", branch: "develop" },
    build: { kind: "railpack" },
  });
});

it("progression : le déploiement suivi par l'API fait avancer les étapes jusqu'à « Projet en ligne »", async () => {
  const web = DEPLOYMENTS.find((d) => d.number === 4818)!;
  const running = DEPLOYMENTS.find((d) => d.number === 4821)!;
  deployReply = () => HttpResponse.json({ ...running, status: "queued", steps: [] }, { status: 201 });
  polled = () => HttpResponse.json(running);
  renderModal();
  pickRepository();
  fireEvent.click(screen.getByRole("button", { name: "Créer le projet" }));
  expect(await screen.findByText("Build en cours…", {}, { timeout: 6000 })).toBeInTheDocument();
  const stages = () => screen.getAllByRole("listitem").map((li) => li.getAttribute("data-state"));
  expect(stages()).toEqual(["done", "running", "pending", "pending"]);
  polled = () => HttpResponse.json({ ...web, id: running.id });
  expect(await screen.findByText("Projet en ligne", {}, { timeout: 6000 })).toBeInTheDocument();
  expect(stages()).toEqual(["done", "done", "done", "done"]);
  // Aucun domaine inventé : seule l'état « En ligne » est affiché.
  expect(document.body.textContent).not.toContain("ciphera.app");
}, 15000);

it("échec du premier déploiement : la cause est affichée", async () => {
  const failed = DEPLOYMENTS.find((d) => d.number === 4807)!;
  deployReply = () => HttpResponse.json({ ...failed, status: "queued", steps: [], error: null }, { status: 201 });
  polled = () => HttpResponse.json(failed);
  renderModal();
  pickRepository();
  fireEvent.click(screen.getByRole("button", { name: "Créer le projet" }));
  expect(await screen.findByText("Échec du déploiement", {}, { timeout: 6000 })).toBeInTheDocument();
  expect(screen.getByRole("alert")).toHaveTextContent("pnpm install a échoué (ERR_PNPM_LOCKFILE)");
  expect(screen.getAllByRole("listitem").map((li) => li.getAttribute("data-state"))).toEqual(["done", "failed", "pending", "pending"]);
}, 15000);

it("déploiement refusé : le projet reste créé, le message s'affiche", async () => {
  deployReply = () => HttpResponse.json({ error: { code: "forbidden", message: "x" } }, { status: 403 });
  const { onOpen } = renderModal();
  pickRepository();
  fireEvent.click(screen.getByRole("button", { name: "Créer le projet" }));
  expect(await screen.findByText("Projet créé")).toBeInTheDocument();
  expect(screen.getByText(/Le premier déploiement n'a pas pu être lancé/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Ouvrir le projet" }));
  expect(onOpen).toHaveBeenCalledWith(PROJECT_SUMMARIES[0].id);
});

function pickImage() {
  fireEvent.click(screen.getByRole("button", { name: /Image Docker/ }));
  fireEvent.change(screen.getByLabelText("Image"), { target: { value: "ghcr.io/org/app:1.2" } });
  fireEvent.click(screen.getByRole("button", { name: "Continuer" }));
}

it("Image Docker : « Exécuter sans root » décoché par défaut, absent pour une source Git", async () => {
  renderModal();
  pickImage();
  expect(screen.getByRole("switch", { name: "Exécuter sans root (recommandé)" })).not.toBeChecked();
  fireEvent.click(screen.getByRole("button", { name: "Créer le projet" }));
  await waitFor(() => expect(deployPosts).toHaveLength(1));
  expect(securityPatches).toHaveLength(0);
});

it("Image Docker : sans root coché, 1000:1000 est enregistré avant le premier déploiement", async () => {
  renderModal();
  pickImage();
  fireEvent.click(screen.getByRole("switch", { name: "Exécuter sans root (recommandé)" }));
  fireEvent.click(screen.getByRole("button", { name: "Créer le projet" }));
  await waitFor(() => expect(deployPosts).toHaveLength(1));
  expect(securityPatches).toHaveLength(1);
  expect(securityPatches[0]!.csrf).toBe("csrf-1");
  expect(securityPatches[0]!.body).toEqual({ run_as: "1000:1000", read_only_rootfs: false });
});

it("Image Docker : réglage refusé, le projet est créé mais aucun déploiement ne part en root", async () => {
  securityReply = () => HttpResponse.json({ error: { code: "validation_failed", message: "Utilisateur invalide." } }, { status: 422 });
  renderModal();
  pickImage();
  fireEvent.click(screen.getByRole("switch", { name: "Exécuter sans root (recommandé)" }));
  fireEvent.click(screen.getByRole("button", { name: "Créer le projet" }));
  expect(await screen.findByText(/n'a pas été enregistré/)).toBeInTheDocument();
  expect(deployPosts).toHaveLength(0);
});

it("source Git : pas d'option sans root", () => {
  renderModal();
  pickRepository();
  expect(screen.queryByRole("switch", { name: /sans root/ })).not.toBeInTheDocument();
});

it("Image Docker : sans root coché mais projet sans service, erreur affichée et aucun déploiement", async () => {
  reply = () => HttpResponse.json({ ...projectDetail(PROJECT_SUMMARIES[0]), services: [] }, { status: 201 });
  renderModal();
  pickImage();
  fireEvent.click(screen.getByRole("switch", { name: "Exécuter sans root (recommandé)" }));
  fireEvent.click(screen.getByRole("button", { name: "Créer le projet" }));
  expect(await screen.findByText(/aucun service/)).toBeInTheDocument();
  expect(securityPatches).toHaveLength(0);
  expect(deployPosts).toHaveLength(0);
});
