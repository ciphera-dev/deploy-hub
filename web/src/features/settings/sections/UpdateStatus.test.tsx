import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, expect, it } from "vitest";

import { LATEST_EXPIRED_MESSAGE, UpdateStatus } from "./UpdateStatus";

let version: Record<string, unknown>;

const server = setupServer(http.get("*/api/system/version", () => HttpResponse.json(version)));

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
afterEach(() => server.resetHandlers());

function mount() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <UpdateStatus />
    </QueryClientProvider>,
  );
}

it("latest.json echu : avertissement affiche, version toujours indiquee", async () => {
  version = { current: "v0.1.0", latest: "v0.2.0", latest_expired: true };
  mount();
  expect(await screen.findByRole("alert")).toHaveTextContent(LATEST_EXPIRED_MESSAGE);
  expect(LATEST_EXPIRED_MESSAGE).toBe("Impossible de garantir que Ciphera est à jour (métadonnées de version de plus de 120 jours)");
  expect(screen.getByText("v0.2.0 disponible")).toBeInTheDocument();
});

it("latest.json valide : aucune alerte", async () => {
  version = { current: "v0.2.0", latest: "v0.2.0", changelog_url: "https://get.ciphera.ch/releases/v0.2.0/CHANGELOG.md" };
  mount();
  expect(await screen.findByText("v0.2.0 : à jour")).toBeInTheDocument();
  expect(screen.getByRole("link", { name: "Notes de version" })).toHaveAttribute("href", "https://get.ciphera.ch/releases/v0.2.0/CHANGELOG.md");
  expect(screen.queryByRole("alert")).toBeNull();
});

it("verification desactivee ou latest.json refuse (signature invalide) : « Non vérifiée »", async () => {
  version = { current: "v0.1.0" };
  mount();
  expect(await screen.findByText("Non vérifiée")).toBeInTheDocument();
  expect(screen.queryByRole("alert")).toBeNull();
});
