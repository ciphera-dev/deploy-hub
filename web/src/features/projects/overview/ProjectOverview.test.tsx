import { createMemoryHistory } from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it } from "vitest";

import { AppProviders, createAppQueryClient } from "@/app/providers";
import { createAppRouter } from "@/app/router";
import { setCsrfToken } from "@/lib/api/csrf";
import { emptyOverview, WEB_OVERVIEW } from "@/mocks/fixtures/overview";
import { PROJECT_SUMMARIES, projectDetail } from "@/mocks/fixtures/projects";

const ME = { id: "u1", name: "Alice Martin", email: "alice@example.com", role: "owner", csrf_token: "csrf-1" };
const WEB = projectDetail(PROJECT_SUMMARIES[0]);
const FRESH = projectDetail(PROJECT_SUMMARIES[4]);
let overviewCalls = 0;

const server = setupServer(
  http.get("*/api/me", () => HttpResponse.json(ME)),
  http.get("*/api/deployments", () => HttpResponse.json([])),
  http.get("*/api/projects/:id", ({ params }) => {
    const found = PROJECT_SUMMARIES.find((p) => p.id === params.id);
    return found ? HttpResponse.json(projectDetail(found)) : HttpResponse.json({ error: { code: "not_found", message: "Projet introuvable." } }, { status: 404 });
  }),
  http.get("*/api/projects/:id/overview", ({ params }) => {
    overviewCalls += 1;
    return HttpResponse.json(params.id === WEB.id ? WEB_OVERVIEW : emptyOverview({ ...FRESH.services[0]!, state: "none", replicas: { running: 0, desired: 1 } }));
  }),
);

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  setCsrfToken("csrf-1");
  overviewCalls = 0;
});
afterEach(() => server.resetHandlers());

function mount(url: string) {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [url] }));
  render(<AppProviders router={router} client={createAppQueryClient(router)} />);
  return router;
}

it("rendu à partir des fixtures de ProjectOverview.jsx : production, indicateurs, services, activité", async () => {
  mount(`/projects/${WEB.id}`);
  const prod = await screen.findByRole("region", { name: "Production" }, { timeout: 5000 });
  expect(within(prod).getByText("Actuel")).toBeInTheDocument();
  expect(within(prod).getByRole("link", { name: /app\.example.com/ })).toHaveAttribute("href", "https://app.example.com");
  expect(within(prod).getByText("Déploiement manuel · #4818")).toBeInTheDocument();
  expect(within(prod).getByText("a3f91c2")).toBeInTheDocument();
  expect(within(prod).getByText("main")).toBeInTheDocument();
  expect(within(prod).getByText("Alex Martin")).toBeInTheDocument();
  expect(within(prod).getByText("Railpack")).toBeInTheDocument();
  expect(within(prod).getByText(/· 1 min 36 s$/)).toBeInTheDocument();

  const kpi = (label: string) => screen.getByLabelText(label);
  expect(within(kpi("Requêtes (24 h)")).getByText("1,24 M")).toBeInTheDocument();
  expect(within(kpi("Latence p95")).getByText("182 ms")).toBeInTheDocument();
  expect(within(kpi("Taux d'erreur")).getByText("0,04 %")).toBeInTheDocument();
  expect(within(kpi("Disponibilité (30 j)")).getByText("99,98 %")).toBeInTheDocument();
  expect(within(kpi("Requêtes (24 h)")).getByRole("img", { name: "Tendance" }).children).toHaveLength(24);

  const services = screen.getByRole("region", { name: "Services" });
  for (const name of ["web", "worker", "postgres"]) expect(within(services).getByText(name)).toBeInTheDocument();
  expect(within(services).getByText("CPU 34%")).toBeInTheDocument();
  expect(within(services).getByText("1 / 1")).toBeInTheDocument();
  expect(within(services).getAllByText("0 / 1")).toHaveLength(2);

  const activity = screen.getByRole("region", { name: "Activité récente" });
  const rows = within(activity).getAllByRole("link");
  expect(rows).toHaveLength(4);
  expect(rows[0]).toHaveTextContent("#4821");
  expect(rows[0]).toHaveTextContent("En cours");
  expect(rows[0]).toHaveAttribute("href", `/projects/${WEB.id}/deployments`);
});

it("sans métriques ni déploiement : « — », séries vides, aucune donnée inventée", async () => {
  mount(`/projects/${FRESH.id}`);
  const prod = await screen.findByRole("region", { name: "Production" }, { timeout: 5000 });
  expect(within(prod).getByText("Aucun domaine configuré")).toBeInTheDocument();
  expect(within(prod).getByText("Aucun déploiement en ligne")).toBeInTheDocument();
  expect(within(prod).queryByText("Actuel")).not.toBeInTheDocument();
  expect(within(prod).queryByRole("link")).not.toBeInTheDocument();
  for (const label of ["Requêtes (24 h)", "Latence p95", "Taux d'erreur", "Disponibilité (30 j)"]) {
    const card = screen.getByLabelText(label);
    expect(within(card).getByText("—")).toBeInTheDocument();
    expect(within(card).getByRole("img", { name: "Aucune mesure" }).children).toHaveLength(0);
  }
  const services = screen.getByRole("region", { name: "Services" });
  expect(within(services).getByText("site")).toBeInTheDocument();
  expect(within(services).getByText("CPU —")).toBeInTheDocument();
  expect(within(services).getByText("0 / 1")).toBeInTheDocument();
  expect(screen.getByText("Aucun déploiement pour l'instant.")).toBeInTheDocument();
});

it("« Tout voir » ouvre l'onglet Services", async () => {
  server.use(http.get("*/api/projects/:id/services", () => HttpResponse.json(WEB_OVERVIEW.services)));
  const router = mount(`/projects/${WEB.id}`);
  fireEvent.click(await screen.findByRole("button", { name: "Tout voir" }, { timeout: 5000 }));
  await waitFor(() => expect(router.state.location.pathname).toBe(`/projects/${WEB.id}/services`));
  expect(overviewCalls).toBe(1);
});
