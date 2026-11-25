import type { components } from "@/lib/api/schema";

type GitSettings = components["schemas"]["GitSettings"];
type DeployHook = components["schemas"]["DeployHook"];
type CreatedDeployHook = components["schemas"]["CreatedDeployHook"];

// Donnees de prototypes/dashboard/ProjectGit.jsx (`init`, `hooks`), ramenees
// au contrat de l'API. Donnees factices pour MSW et les tests : le secret
// ci-dessous n'est pas un vrai secret.

export function gitSettings(repository: string, branch: string): GitSettings {
  return { repository, branch, root_directory: "", watch_paths: ["apps/web/**", "packages/ui/**"], auto_deploy: true, skip_ci: true, ignore_bots: false };
}

export const HOOK_ID = "00000000-0000-4000-8000-0000000000d1";

export const DEPLOY_HOOKS: DeployHook[] = [{ id: HOOK_ID, name: "CI externe", path: `/v1/hooks/deploy/${HOOK_ID}`, created_by: "Alice Martin", created_at: "2026-05-15T10:00:00.000Z", last_used_at: null, expires_at: "2099-01-01T00:00:00.000Z" }];

export function createdHook(name: string): CreatedDeployHook {
  const id = "00000000-0000-4000-8000-0000000000d2";
  const token = "dh_testFixtureNotASecretFixtureNotASecret0000";
  return { hook: { id, name, path: `/v1/hooks/deploy/${id}`, created_by: "Alice Martin", created_at: "2026-05-15T10:05:00.000Z", last_used_at: null, expires_at: "2026-08-13T10:05:00.000Z" }, token, url_path: `/v1/hooks/deploy/${id}`, token_header: "X-Ciphera-Hook-Token" };
}
