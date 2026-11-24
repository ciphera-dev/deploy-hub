import createClient from "openapi-fetch";
import { afterEach, expect, it, vi } from "vitest";

import { unwrap } from "@/lib/api/errors";
import type { paths } from "@/lib/api/schema";

import { classifyError, createQueryClient } from "./query-client";

function setup(fetchImpl: (request: Request) => Promise<Response>) {
  const handlers = { onUnauthorized: vi.fn(), onFatal: vi.fn(), onMutationError: vi.fn() };
  const client = createQueryClient(handlers);
  const api = createClient<paths>({ baseUrl: "http://panel.test", fetch: fetchImpl });
  const load = () => client.fetchQuery({ queryKey: ["health"], retry: false, queryFn: async () => unwrap(await api.GET("/api/health")) }).catch(() => undefined);
  return { handlers, load };
}

const status = (code: number) => async () => new Response("{}", { status: code, headers: { "content-type": "application/json" } });

afterEach(() => vi.restoreAllMocks());

it("401 : redirection vers la connexion, sans ecran d'erreur", async () => {
  const { handlers, load } = setup(status(401));
  await load();
  expect(handlers.onUnauthorized).toHaveBeenCalledTimes(1);
  expect(handlers.onFatal).not.toHaveBeenCalled();
});

it("403 : ecran Forbidden", async () => {
  const { handlers, load } = setup(status(403));
  await load();
  expect(handlers.onFatal).toHaveBeenCalledWith("forbidden");
  expect(handlers.onUnauthorized).not.toHaveBeenCalled();
});

it("erreur reseau (fetch rejette un TypeError) : ecran Offline", async () => {
  const { handlers, load } = setup(async () => {
    throw new TypeError("Failed to fetch");
  });
  await load();
  expect(handlers.onFatal).toHaveBeenCalledWith("offline");
});

it("5xx : ecran ServerError", async () => {
  const { handlers, load } = setup(status(503));
  await load();
  expect(handlers.onFatal).toHaveBeenCalledWith("server");
});

it("404 et erreurs de validation ne declenchent aucun ecran", async () => {
  const { handlers, load } = setup(status(404));
  await load();
  expect(handlers.onFatal).not.toHaveBeenCalled();
  expect(handlers.onUnauthorized).not.toHaveBeenCalled();
});

it("401 sur /api/auth/* (identifiants refuses) : ni redirection ni ecran", async () => {
  const handlers = { onUnauthorized: vi.fn(), onFatal: vi.fn(), onMutationError: vi.fn() };
  const client = createQueryClient(handlers);
  const response = { ok: false, status: 401, url: "http://panel.test/api/auth/login" } as Response;
  await client.fetchQuery({ queryKey: ["login"], retry: false, queryFn: () => unwrap({ response }) }).catch(() => undefined);
  expect(handlers.onUnauthorized).not.toHaveBeenCalled();
  expect(handlers.onFatal).not.toHaveBeenCalled();
});

it("401 sur /api/setup/* (jeton d'installation invalide) : ni redirection ni ecran", async () => {
  const handlers = { onUnauthorized: vi.fn(), onFatal: vi.fn(), onMutationError: vi.fn() };
  const client = createQueryClient(handlers);
  const response = { ok: false, status: 401, url: "http://panel.test/api/setup/complete" } as Response;
  await client.fetchQuery({ queryKey: ["setup"], retry: false, queryFn: () => unwrap({ response }) }).catch(() => undefined);
  expect(handlers.onUnauthorized).not.toHaveBeenCalled();
  expect(handlers.onFatal).not.toHaveBeenCalled();
});

it("les mutations : 401 redirige, 5xx va au gestionnaire de mutation et non a l'ecran plein page", async () => {
  const handlers = { onUnauthorized: vi.fn(), onFatal: vi.fn(), onMutationError: vi.fn() };
  const client = createQueryClient(handlers);
  const api = createClient<paths>({ baseUrl: "http://panel.test", fetch: status(401) });
  await client
    .getMutationCache()
    .build(client, { mutationFn: async () => unwrap(await api.GET("/api/health")) })
    .execute(undefined)
    .catch(() => undefined);
  expect(handlers.onUnauthorized).toHaveBeenCalledTimes(1);

  const failing = createClient<paths>({ baseUrl: "http://panel.test", fetch: status(500) });
  await client
    .getMutationCache()
    .build(client, { mutationFn: async () => unwrap(await failing.GET("/api/health")) })
    .execute(undefined)
    .catch(() => undefined);
  expect(handlers.onMutationError).toHaveBeenCalledWith("server");
  expect(handlers.onFatal).not.toHaveBeenCalled();
});

it("classifyError ignore les erreurs inconnues et les annulations", () => {
  expect(classifyError(new Error("boom"))).toBeNull();
  expect(classifyError(new DOMException("aborted", "AbortError"))).toBeNull();
});
