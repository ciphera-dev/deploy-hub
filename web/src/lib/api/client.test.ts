import { afterEach, expect, it, vi } from "vitest";

afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetModules();
});

it("appelle /api/health a la racine de l'origine", async () => {
  const fetchMock = vi.fn(async (_request: Request) =>
    new Response(JSON.stringify({ status: "ok", version: "0.1.0" }), {
      status: 200,
      headers: { "content-type": "application/json" },
    }),
  );
  vi.stubGlobal("fetch", fetchMock);
  const { api } = await import("./client");

  const { data } = await api.GET("/api/health");

  expect(data).toEqual({ status: "ok", version: "0.1.0" });
  const request = fetchMock.mock.calls[0]![0];
  expect(new URL(request.url).pathname).toBe("/api/health");
  expect(request.credentials).toBe("same-origin");
});
it("ajoute X-CSRF-Token aux ecritures seulement, depuis la memoire", async () => {
  const fetchMock = vi.fn(async (_request: Request) => new Response(null, { status: 204 }));
  vi.stubGlobal("fetch", fetchMock);
  const { api } = await import("./client");
  const { setCsrfToken } = await import("./csrf");

  await api.POST("/api/auth/logout");
  expect(fetchMock.mock.calls[0]![0].headers.get("X-CSRF-Token")).toBeNull();

  setCsrfToken("jeton-de-session");
  await api.POST("/api/auth/logout");
  await api.GET("/api/me");
  const [post, get] = fetchMock.mock.calls.slice(1).map((c) => c[0]);
  expect(post!.headers.get("X-CSRF-Token")).toBe("jeton-de-session");
  expect(get!.headers.get("X-CSRF-Token")).toBeNull();
  setCsrfToken(null);
});
