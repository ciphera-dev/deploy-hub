import { createMemoryHistory } from "@tanstack/react-router";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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
  return router;
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

async function openSheet() {
  const more = await screen.findByRole("button", { name: "Plus" });
  fireEvent.click(more);
  return { more, sheet: await screen.findByRole("dialog", { name: "Plus de navigation" }) };
}

it("les quatre onglets sont presents, nommes, et ouvrent leur page", async () => {
  setViewport(390);
  const router = mount("/");
  const nav = await screen.findByRole("navigation", { name: "Navigation principale" });
  const links = within(nav).getAllByRole("link");
  expect(links.map((a) => a.textContent)).toEqual(["Accueil", "Projets", "Déploiements", "Connecteurs"]);
  expect(links.map((a) => a.getAttribute("href"))).toEqual(["/", "/projects", "/deployments", "/connectors"]);
  // le nom accessible contient le texte visible (WCAG 2.5.3)
  for (const a of links) expect(a).toHaveAccessibleName(a.textContent ?? "");
  fireEvent.click(within(nav).getByRole("link", { name: "Déploiements" }));
  await waitFor(() => expect(router.state.location.pathname).toBe("/deployments"));
});

it("la feuille se ferme a Echap et rend le focus a « Plus »", async () => {
  setViewport(390);
  mount("/");
  const { more, sheet } = await openSheet();
  expect(more).toHaveAttribute("aria-expanded", "true");
  fireEvent.keyDown(sheet, { key: "Escape" });
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  await waitFor(() => expect(more).toHaveFocus());
});

it("la feuille se ferme au clic dehors", async () => {
  setViewport(390);
  mount("/");
  await openSheet();
  const overlay = document.querySelector(".app-sheet__overlay") as HTMLElement;
  fireEvent.pointerDown(overlay);
  fireEvent.pointerUp(overlay);
  fireEvent.click(overlay);
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

it("la feuille se ferme apres un lien, y compris vers la page courante", async () => {
  setViewport(390);
  const router = mount("/projects");
  let { sheet } = await openSheet();
  fireEvent.click(within(sheet).getByRole("link", { name: "Projets" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(router.state.location.pathname).toBe("/projects");
  ({ sheet } = await openSheet());
  fireEvent.click(within(sheet).getByRole("link", { name: "Connecteurs" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  await waitFor(() => expect(router.state.location.pathname).toBe("/connectors"));
  expect(await screen.findByRole("button", { name: "Plus" })).toHaveFocus();
});

it("la navigation de la feuille se nomme « Navigation complète »", async () => {
  setViewport(390);
  mount("/");
  const { sheet } = await openSheet();
  expect(within(sheet).getByRole("navigation", { name: "Navigation complète" })).toBeInTheDocument();
});

it("tablette (768 a 1023 px) : barre laterale repliee en rail ; bureau : depliee", async () => {
  setViewport(900);
  mount("/");
  await screen.findByRole("navigation", { name: "Navigation principale" });
  expect(screen.getByRole("button", { name: "Déplier le menu" })).toBeInTheDocument();
});

it("bureau : barre laterale depliee au depart", async () => {
  setViewport(1440);
  mount("/");
  await screen.findByRole("navigation", { name: "Navigation principale" });
  expect(screen.getByRole("button", { name: "Replier le menu" })).toBeInTheDocument();
});

it("en-tete : recherche et cloche inactives, raccourci selon la plateforme, menu du compte", async () => {
  setViewport(1440);
  vi.spyOn(navigator, "platform", "get").mockReturnValue("Win32");
  mount("/");
  const search = await screen.findByRole("button", { name: "Rechercher" });
  expect(search).toHaveAttribute("aria-disabled", "true");
  expect(search).toHaveTextContent("Ctrl K");
  expect(screen.getByRole("button", { name: "Notifications" })).toHaveAttribute("aria-disabled", "true");
  expect(screen.getByRole("button", { name: "Ciphera" })).toHaveAttribute("aria-disabled", "true");
  fireEvent.keyDown(screen.getByRole("button", { name: "Mon compte" }), { key: "ArrowDown" });
  expect(await screen.findByRole("menuitem", { name: /Se déconnecter/ })).toBeInTheDocument();
});

it("en-tete : ⌘K sur macOS", async () => {
  setViewport(1440);
  vi.spyOn(navigator, "platform", "get").mockReturnValue("MacIntel");
  mount("/");
  expect(await screen.findByRole("button", { name: "Rechercher" })).toHaveTextContent("⌘K");
});
