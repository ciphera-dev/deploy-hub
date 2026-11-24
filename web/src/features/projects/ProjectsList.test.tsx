import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";

import { ToastProvider } from "@/design-system";
import { setCsrfToken } from "@/lib/api/csrf";
import { PROJECT_SUMMARIES } from "@/mocks/fixtures/projects";

import { ProjectsList } from "./ProjectsList";

let projects: typeof PROJECT_SUMMARIES;
const server = setupServer(http.get("*/api/projects", () => HttpResponse.json(projects)));

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  projects = PROJECT_SUMMARIES;
  setCsrfToken("csrf-1");
});
afterEach(() => server.resetHandlers());

function renderList(open = vi.fn()) {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ToastProvider>
        <ProjectsList open={open} />
      </ToastProvider>
    </QueryClientProvider>,
  );
  return open;
}

const rowOf = (name: string) => screen.getByText(name).closest("[data-project]") as HTMLElement;

it("affiche une ligne par projet avec statut, build et branche", async () => {
  renderList();
  for (const p of PROJECT_SUMMARIES) expect(await screen.findByText(p.name)).toBeInTheDocument();
  const web = within(rowOf("ciphera-web"));
  expect(web.getByText("production · Railpack · main")).toBeInTheDocument();
  expect(web.getByText("En ligne")).toBeInTheDocument();
  expect(within(rowOf("billing-api")).getByText("Échec du build")).toBeInTheDocument();
  expect(within(rowOf("dashboard-v2")).getByText("Build en cours")).toBeInTheDocument();
  expect(within(rowOf("marketing-site")).getByText("production · Compose · develop")).toBeInTheDocument();
  expect(within(rowOf("cache")).getByText("production · Image")).toBeInTheDocument();
  // Aucun domaine n'existe encore : pas de domaine inventé.
  expect(document.body.textContent).not.toContain("ciphera.app");
});

it("les onglets filtrent par erreur ; seul l'environnement production existe", async () => {
  renderList();
  await screen.findByText("ciphera-web");
  const tabs = screen.getByRole("tablist");
  expect(within(tabs).getAllByRole("tab").map((t) => t.textContent)).toEqual(["Tous6", "Production6", "En erreur1"]);
  fireEvent.click(within(tabs).getByRole("tab", { name: /En erreur/ }));
  expect(screen.getByText("billing-api")).toBeInTheDocument();
  expect(screen.queryByText("ciphera-web")).not.toBeInTheDocument();
});

it("déplier un projet en échec montre le bandeau, pas un projet en ligne", async () => {
  renderList();
  await screen.findByText("billing-api");
  fireEvent.click(screen.getByText("billing-api"));
  expect(screen.getByText(/Le dernier build a échoué/)).toBeInTheDocument();
  expect(screen.getByText("ciphera/billing")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Relancer" })).toBeDisabled();
  fireEvent.click(screen.getByText("ciphera-web"));
  expect(screen.queryByText(/Le dernier build a échoué/)).not.toBeInTheDocument();
  expect(screen.getByText("ciphera/web")).toBeInTheDocument();
});

it("« Gérer » ouvre le projet", async () => {
  const open = renderList();
  await screen.findByText("auth-service");
  fireEvent.click(within(rowOf("auth-service")).getByRole("button", { name: "Gérer" }));
  expect(open).toHaveBeenCalledWith(PROJECT_SUMMARIES[3].id);
});

it("trie par nom au clic sur l'en-tête", async () => {
  renderList();
  await screen.findByText("auth-service");
  const names = () => [...document.querySelectorAll("[data-project]")].map((r) => r.getAttribute("data-project"));
  expect(names()[0]).toBe("auth-service");
  fireEvent.click(screen.getByRole("button", { name: /Détails/ }));
  expect(names()[0]).toBe("marketing-site");
});

it("liste vide : message, et l'import ouvre la modale", async () => {
  projects = [];
  renderList();
  expect(await screen.findByText("Aucun projet pour l'instant.")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Importer un dépôt" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "Annuler" })).toBeInTheDocument());
});
