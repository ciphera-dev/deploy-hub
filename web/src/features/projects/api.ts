import { api } from "@/lib/api/client";
import { unwrap } from "@/lib/api/errors";
import type { components } from "@/lib/api/schema";

export type ProjectSummary = components["schemas"]["ProjectSummary"];
export type Project = components["schemas"]["Project"];
export type NewProject = components["schemas"]["NewProject"];
export type Source = components["schemas"]["Source"];
export type Build = components["schemas"]["Build"];
export type ProjectStatus = components["schemas"]["ProjectStatus"];

export const PROJECTS_QUERY_KEY = ["projects"] as const;
export const projectQueryKey = (id: string) => ["projects", id] as const;

/** GET /api/projects */
export async function fetchProjects(): Promise<ProjectSummary[]> {
  return unwrap(await api.GET("/api/projects"));
}

/** GET /api/projects/{id} */
export async function fetchProject(id: string): Promise<Project> {
  return unwrap(await api.GET("/api/projects/{id}", { params: { path: { id } } }));
}

export type CreateResult = { ok: true; project: Project } | { ok: false; message: string };

/** POST /api/projects : les refus de validation sont affiches par la modale. */
export async function createProject(body: NewProject): Promise<CreateResult> {
  let result;
  try {
    result = await api.POST("/api/projects", { body });
  } catch {
    return { ok: false, message: "Serveur injoignable. Réessayez." };
  }
  if (result.data) return { ok: true, project: result.data };
  const { status } = result.response;
  const message = (result.error as { error?: { message?: unknown } } | undefined)?.error?.message;
  if ((status === 422 || status === 409) && typeof message === "string") return { ok: false, message };
  if (status === 403) return { ok: false, message: "Accès refusé : la création d'un projet demande le rôle développeur." };
  return { ok: false, message: "Création impossible pour le moment. Réessayez." };
}
