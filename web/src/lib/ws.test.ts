import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { openJsonSocket, reconnectDelay } from "./ws";

class FakeSocket extends EventTarget {
  static all: FakeSocket[] = [];
  closed = false;
  constructor(public url: string) {
    super();
    FakeSocket.all.push(this);
  }
  close() {
    this.closed = true;
  }
  open() {
    this.dispatchEvent(new Event("open"));
  }
  /** Fermeture côté serveur ou réseau (le navigateur envoie `error` puis `close`). */
  end(code = 1006) {
    this.dispatchEvent(new Event("error"));
    this.dispatchEvent(new CloseEvent("close", { code }));
  }
}

beforeEach(() => {
  FakeSocket.all = [];
  vi.useFakeTimers();
  vi.stubGlobal("WebSocket", FakeSocket);
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

const last = () => FakeSocket.all[FakeSocket.all.length - 1]!;

it("no_reconnect_on_1008", () => {
  const onUnreachable = vi.fn();
  const onStatus = vi.fn();
  openJsonSocket("ws://x/ws", { onMessage: vi.fn(), onStatus, onUnreachable });
  last().open();
  last().end(1008);
  vi.advanceTimersByTime(60_000);
  expect(FakeSocket.all).toHaveLength(1);
  expect(onStatus).toHaveBeenLastCalledWith("closed");
  // La session est vérifiée une fois, pas de boucle de reconnexion.
  expect(onUnreachable).toHaveBeenCalledTimes(1);
});

it("probes_me_after_3_failures", () => {
  const onUnreachable = vi.fn();
  openJsonSocket("ws://x/ws", { onMessage: vi.fn(), onUnreachable });
  for (let i = 0; i < 2; i++) {
    last().end();
    vi.advanceTimersByTime(reconnectDelay(i));
  }
  expect(onUnreachable).not.toHaveBeenCalled();
  last().end();
  expect(onUnreachable).toHaveBeenCalledTimes(1);
  // Le flux continue d'être retenté.
  vi.advanceTimersByTime(reconnectDelay(2));
  expect(FakeSocket.all).toHaveLength(4);
});

it("une ouverture réussie remet le compte d'échecs à zéro", () => {
  const onUnreachable = vi.fn();
  openJsonSocket("ws://x/ws", { onMessage: vi.fn(), onUnreachable });
  last().end();
  vi.advanceTimersByTime(reconnectDelay(0));
  last().end();
  vi.advanceTimersByTime(reconnectDelay(1));
  last().open();
  last().end();
  vi.advanceTimersByTime(reconnectDelay(0));
  last().end();
  expect(onUnreachable).not.toHaveBeenCalled();
});
