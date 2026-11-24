import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll, beforeEach, expect, it } from "vitest";

import { setCsrfToken } from "@/lib/api/csrf";

import { ProfileAvatar } from "./Avatar";

let puts: { csrf: string | null; size: number }[];
let deletes: (string | null)[];

const server = setupServer(
  http.put("*/api/me/avatar", async ({ request }) => {
    puts.push({ csrf: request.headers.get("x-csrf-token"), size: (await request.arrayBuffer()).byteLength });
    return HttpResponse.json({ avatar_url: "/api/me/avatar?v=2" });
  }),
  http.delete("*/api/me/avatar", ({ request }) => {
    deletes.push(request.headers.get("x-csrf-token"));
    return new HttpResponse(null, { status: 204 });
  }),
  http.get("*/api/me", () => HttpResponse.json({})),
);

beforeAll(() => server.listen({ onUnhandledFrame: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  puts = [];
  deletes = [];
  setCsrfToken("csrf-1");
});
afterEach(() => server.resetHandlers());

function renderAvatar(props: { src?: string | null; editable?: boolean } = {}) {
  return render(
    <QueryClientProvider client={new QueryClient()}>
      <ProfileAvatar name="Alice Martin" size={88} {...props} />
    </QueryClientProvider>,
  );
}

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3, 4]);

it("Avatar affiche les initiales sans image", () => {
  const { container } = renderAvatar();
  expect(screen.getByRole("img", { name: "Alice Martin" })).toHaveTextContent("AM");
  expect(container.querySelector("img")).toBeNull();
});

it("avec une image : affiche l'image du serveur", () => {
  renderAvatar({ src: "/api/me/avatar?v=1" });
  expect(screen.getByRole("img", { name: "Alice Martin" })).toHaveAttribute("src", "/api/me/avatar?v=1");
});

it("fenetre Photo de profil : import seulement, sans galerie de personnages", async () => {
  renderAvatar({ editable: true });
  fireEvent.click(screen.getByRole("button", { name: "Changer la photo de profil" }));
  const dialog = await screen.findByRole("dialog");
  expect(dialog).toHaveTextContent("Photo de profil");
  for (const gone of ["Tous", "Anime", "Jeux vidéo", "Séries", "Films"]) expect(dialog).not.toHaveTextContent(gone);
  expect(screen.getByRole("button", { name: "Importer" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Initiales/ })).toHaveAttribute("aria-pressed", "true");

  const file = new File([PNG], "moi.png", { type: "image/png" });
  fireEvent.change(screen.getByLabelText("Importer une photo"), { target: { files: [file] } });
  expect(await screen.findByRole("img", { name: "Aperçu" })).toHaveAttribute("src", expect.stringMatching(/^data:image\/png;base64,/));
  fireEvent.click(screen.getByRole("button", { name: "Enregistrer" }));
  await waitFor(() => expect(puts).toEqual([{ csrf: "csrf-1", size: PNG.byteLength }]));
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
});

it("fichier refuse avant l'envoi : format ou taille", async () => {
  renderAvatar({ editable: true });
  fireEvent.click(screen.getByRole("button", { name: "Changer la photo de profil" }));
  const input = await screen.findByLabelText("Importer une photo");
  fireEvent.change(input, { target: { files: [new File(["<svg/>"], "x.svg", { type: "image/svg+xml" })] } });
  expect(await screen.findByRole("alert")).toHaveTextContent("Formats acceptés : PNG, JPEG ou WebP.");
  const big = new File([new Uint8Array(1024 * 1024 + 1)], "big.png", { type: "image/png" });
  fireEvent.change(input, { target: { files: [big] } });
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("L'image dépasse 1 Mo."));
  fireEvent.click(screen.getByRole("button", { name: "Enregistrer" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  expect(puts).toEqual([]);
});

it("revenir aux initiales supprime l'avatar", async () => {
  renderAvatar({ src: "/api/me/avatar?v=1", editable: true });
  fireEvent.click(screen.getByRole("button", { name: "Changer la photo de profil" }));
  fireEvent.click(await screen.findByRole("button", { name: /Initiales/ }));
  fireEvent.click(screen.getByRole("button", { name: "Enregistrer" }));
  await waitFor(() => expect(deletes).toEqual(["csrf-1"]));
});
