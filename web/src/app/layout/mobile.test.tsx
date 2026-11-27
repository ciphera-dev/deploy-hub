import { createMemoryHistory } from "@tanstack/react-router";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";

import { fatalError } from "@/app/errors/fatal-error-store";
import { AppProviders, createAppQueryClient } from "@/app/providers";
import { createAppRouter } from "@/app/router";
import { setCsrfToken } from "@/lib/api/csrf";
import { emptyInstanceOverview } from "@/mocks/fixtures/dashboard";

const ME = { id: "u1", name: "Alice Martin", email: "alice@example.com", role: "owner", csrf_token: "csrf-test-alice" };

const server = setupServer(
  http.get("*/api/projects", () => HttpResponse.json([])),
  http.get("*/api/deployments", () => HttpResponse.json([])),
  http.get("*/api/overview", () => HttpResponse.json(emptyInstanceOverview())),
  http.get("*/api/me", () => HttpResponse.json(ME)),
);

/** matchMedia : seules les requetes max-width <= largeur simulee correspondent. */
function setViewport(width: number) {
  window.matchMedia = ((query: string) => {
    const max = /max-width:\s*([\d.]+)px/.exec(query);
    return { matches: max ? width <= Number(max[1]) : false, media: query, addEventListener: () => undefined, removeEventListener: () => undefined } as unknown as MediaQueryList;
  }) as typeof window.matchMedia;
}

function mount(url: string) {
  const router = createAppRouter(createMemoryHistory({ initialEntries: [url] }));
  render(<AppProviders router={router} client={createAppQueryClient(router)} />);
}

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  fatalError.clear();
  setCsrfToken(null);
});
afterEach(() => {
  server.resetHandlers();
  vi.restoreAllMocks();
  // @ts-expect-error retour a l'absence de matchMedia (jsdom)
  delete window.matchMedia;
});

it("sous 768 px : barre d'onglets en bas, sans barre laterale", async () => {
  setViewport(390);
  mount("/projects");
  const nav = await screen.findByRole("navigation", { name: "Navigation principale" });
  expect(nav).toHaveClass("app-tabbar");
  expect(screen.getAllByRole("navigation", { name: "Navigation principale" })).toHaveLength(1);
  const current = within(nav).getAllByRole("link").filter((a) => a.getAttribute("aria-current") === "page");
  expect(current).toHaveLength(1);
  expect(current[0]).toHaveAccessibleName("Projets");
  expect(screen.queryByRole("button", { name: /Replier le menu/ })).toBeNull();
});

it("« Plus » ouvre une feuille du bas accessible, fermee par Echap", async () => {
  setViewport(390);
  mount("/");
  const more = await screen.findByRole("button", { name: "Plus" });
  expect(more).toHaveAttribute("aria-expanded", "false");
  fireEvent.click(more);
  const sheet = await screen.findByRole("dialog", { name: "Plus de navigation" });
  expect(within(sheet).getByText("Bases de données")).toBeInTheDocument();
  expect(within(sheet).getAllByText("Bientôt").length).toBeGreaterThan(0);
  fireEvent.keyDown(sheet, { key: "Escape" });
  await vi.waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

it("au-dessus de 768 px : barre laterale, pas de barre d'onglets", async () => {
  setViewport(1440);
  mount("/");
  const nav = await screen.findByRole("navigation", { name: "Navigation principale" });
  expect(nav).not.toHaveClass("app-tabbar");
  expect(screen.queryByRole("button", { name: "Plus" })).toBeNull();
});
