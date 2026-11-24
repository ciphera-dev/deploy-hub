import type { components } from "@/lib/api/schema";

type GithubRepo = components["schemas"]["GithubRepo"];
type GithubStatus = components["schemas"]["GithubStatus"];

// Donnees factices pour MSW et les tests (Projects.jsx#REPOS, Connectors.jsx),
// ramenees au contrat de l'API. Aucun secret : l'API n'en renvoie jamais.

export const GITHUB_REPOS: GithubRepo[] = [
  { full_name: "acme/api", name: "api", owner: "acme", private: false, default_branch: "develop", updated_at: "2026-11-20T10:00:00Z" },
  { full_name: "acme/web", name: "web", owner: "acme", private: true, default_branch: "main", updated_at: null },
];

export const GITHUB_DISCONNECTED: GithubStatus = { connected: false, app: null };

export const GITHUB_CONNECTED: GithubStatus = {
  connected: true,
  app: {
    app_id: 123,
    slug: "ciphera-ab12cd",
    name: "Ciphera ab12cd",
    owner: "acme",
    html_url: "https://github.com/apps/ciphera-ab12cd",
    install_url: "https://github.com/apps/ciphera-ab12cd/installations/new",
  },
};
