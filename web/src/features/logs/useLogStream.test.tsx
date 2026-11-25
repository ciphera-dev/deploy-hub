import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { fatalError } from "@/app/errors/fatal-error-store";
import { setSessionExpiredHandler } from "@/lib/api/session";
import { reconnectDelay } from "@/lib/ws";

const get = vi.hoisted(() => vi.fn());
vi.mock("@/lib/api/client", () => ({ api: { GET: get } }));

import { useLogStream } from "./useLogStream";

class FakeSocket extends EventTarget {
  static all: FakeSocket[] = [];
  constructor(public url: string) {
    super();
    FakeSocket.all.push(this);
  }
  close() {}
  end(code = 1006) {
    act(() => {
      this.dispatchEvent(new CloseEvent("close", { code }));
    });
  }
}

const expire = vi.fn();

beforeEach(() => {
  FakeSocket.all = [];
  get.mockReset();
  expire.mockReset();
  vi.useFakeTimers();
  vi.stubGlobal("WebSocket", FakeSocket);
  setSessionExpiredHandler(expire);
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  setSessionExpiredHandler(null);
});

const last = () => FakeSocket.all[FakeSocket.all.length - 1]!;

/** Trois ouvertures ratées de suite. */
async function failThrice() {
  for (let i = 0; i < 3; i++) {
    last().end();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(reconnectDelay(i));
    });
  }
}

const sources = [{ service: "svc" }];

it("sonde en erreur réseau ou 5xx : pas d'écran d'erreur, le flux continue de se rouvrir", async () => {
  fatalError.clear();
  get.mockRejectedValueOnce(new TypeError("Failed to fetch"));
  renderHook(() => useLogStream(sources));
  const before = FakeSocket.all.length;
  await failThrice();
  expect(get).toHaveBeenCalledWith("/api/me");
  expect(fatalError.get()).toBeNull();
  expect(expire).not.toHaveBeenCalled();
  expect(FakeSocket.all.length).toBeGreaterThan(before + 2);

  get.mockResolvedValueOnce({ response: { status: 503 } });
  await failThrice();
  expect(fatalError.get()).toBeNull();
  expect(expire).not.toHaveBeenCalled();
});

it("sonde en 401 : politique de session (expiration)", async () => {
  get.mockResolvedValueOnce({ response: { status: 401 } });
  renderHook(() => useLogStream(sources));
  await failThrice();
  expect(expire).toHaveBeenCalledTimes(1);
});

it("1008 : flux fermé, indiqué par `closed`, sans reconnexion", async () => {
  get.mockResolvedValue({ response: { status: 200 } });
  const { result } = renderHook(() => useLogStream(sources));
  last().end(1008);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60_000);
  });
  expect(FakeSocket.all).toHaveLength(1);
  expect(result.current.closed).toBe(true);
  expect(result.current.reconnecting).toBe(false);
});
