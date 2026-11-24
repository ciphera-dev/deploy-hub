import { createMemoryHistory } from "@tanstack/react-router";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it } from "vitest";

import { AppProviders, createAppQueryClient } from "@/app/providers";
import { createAppRouter } from "@/app/router";
import { setCsrfToken } from "@/lib/api/csrf";
import { emptyInstanceOverview, OVERVIEW } from "@/mocks/fixtures/dashboard";

import { greeting } from "./format";

const ME = { id: "u1", name: "Alice Martin", email: "alice@example.com", role: "viewer", csrf_token: "csrf-1" };
let reply: () => Response = () => HttpResponse.json(OVERVIEW);
let periods: string[] = [];
let meCalls = 0;

const server = setupServer(
  http.get("*/api/me", () => {
    meCalls += 1;
    return HttpResponse.json(ME);
  }),
  http.get("*/api/projects", () => HttpResponse.json([])),
  http.get("*/api/deployments", () => HttpResponse.json([])),
  http.get("*/api/overview", ({ request }) => {
    periods.push(new URL(request.url).searchParams.get("period") ?? "");
    return reply();
  }),
);

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  setCsrfToken("csrf-1");
  reply = () => HttpResponse.json(OVERVIEW);
  periods = [];
  meCalls = 0;
});
afterEach(() => server.resetHandlers());

function mount(url = "/") {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [url] }));
  render(<AppProviders router={router} client={createAppQueryClient(router)} />);
  return router;
}

it("rendu à partir des fixtures de Pages2.jsx#OverviewV2 : en-tête, à traiter, indicateurs, déploiements, serveurs", async () => {
  mount();
  const hero = await screen.findByRole("region", { name: "Vue d'ensemble" }, { timeout: 5000 });
  expect(within(hero).getByText(new RegExp(`^${greeting(new Date().getHours())} Alice$`))).toBeInTheDocument();
  expect(within(hero).getByText("2 points à traiter, le reste fonctionne normalement.")).toBeInTheDocument();
  expect(within(hero).getByText("/ 6")).toBeInTheDocument();
  expect(within(hero).getByText("14")).toBeInTheDocument();
  const todo = within(hero).getByRole("region", { name: "À traiter" });
  expect(within(todo).getByText("billing-api")).toBeInTheDocument();
  expect(within(todo).getByText("Dernier build en échec")).toBeInTheDocument();
  expect(within(todo).getByText("Agent injoignable")).toBeInTheDocument();

  const kpis = screen.getByRole("region", { name: "Indicateurs" });
  expect(within(kpis).getByRole("tab", { name: /Déploiements/ })).toHaveAttribute("aria-selected", "true");
  expect(within(kpis).getByText("+12,4 %")).toBeInTheDocument();
  expect(within(kpis).getByRole("img", { name: "Courbe de la période" })).toBeInTheDocument();
  // Requêtes : aucune source, état vide sans valeur inventée.
  fireEvent.click(within(kpis).getByRole("tab", { name: /Requêtes/ }));
  expect(within(kpis).getByText(/Aucune mesure pour l'instant/)).toBeInTheDocument();
  expect(within(within(kpis).getByRole("tab", { name: /Requêtes/ })).getByText("—")).toBeInTheDocument();

  const recent = screen.getByRole("region", { name: "Déploiements récents" });
  expect(within(recent).getAllByText(/ciphera-web/).length).toBeGreaterThan(0);
  const load = screen.getByRole("region", { name: "Charge des serveurs" });
  expect(within(load).getByText("42 % CPU")).toBeInTheDocument();
  expect(within(load).getByText("Hors ligne")).toBeInTheDocument();
  // Agent refusé (protocole d'une autre version) : la cause est affichée.
  expect(within(load).getByText(/Version du protocole incompatible/)).toBeInTheDocument();
  // Le compte du shell n'est pas relu par l'écran.
  expect(meCalls).toBe(1);
}, 20_000);

it("instance neuve : « Rien à signaler » et blocs vides", async () => {
  reply = () => HttpResponse.json(emptyInstanceOverview());
  mount();
  const todo = await screen.findByRole("region", { name: "À traiter" }, { timeout: 5000 });
  expect(within(todo).getByText("Rien à signaler")).toBeInTheDocument();
  expect(screen.getByText("Tous vos services fonctionnent normalement.")).toBeInTheDocument();
  expect(screen.getByText("Aucun déploiement pour l'instant.")).toBeInTheDocument();
  expect(screen.getByText("Aucun serveur enregistré.")).toBeInTheDocument();
}, 20_000);

it("la pastille de période bascule entre 30 et 7 jours", async () => {
  reply = () => HttpResponse.json(emptyInstanceOverview(periods.at(-1) === "7d" ? "7d" : "30d"));
  mount();
  const pill = await screen.findByRole("button", { name: "30 derniers jours" }, { timeout: 5000 });
  fireEvent.click(pill);
  await screen.findByRole("button", { name: "7 derniers jours" });
  expect(periods).toEqual(["30d", "7d"]);
}, 20_000);

it("lecture refusée : écran d'erreur du shell, sans données factices", async () => {
  reply = () => HttpResponse.json({ error: { code: "forbidden", message: "Accès refusé." } }, { status: 403 });
  mount();
  expect(await screen.findByText("Accès refusé", undefined, { timeout: 5000 })).toBeInTheDocument();
  expect(screen.queryByText("Rien à signaler")).not.toBeInTheDocument();
}, 20_000);
