import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { ToastProvider } from "@/design-system";
import type { ProjectService } from "@/features/projects/overview/api";
import { logLines } from "@/mocks/fixtures/logs";
import { reconnectDelay, wsUrl } from "@/lib/ws";

import { containersOf } from "./containers";
import { CONTAINER_COLORS, formatMem, LogViewer, type LogContainer } from "./LogViewer";
import type { WireLogLine } from "./useLogStream";

/** WebSocket simulé : les tests poussent les messages du serveur. */
class FakeSocket extends EventTarget {
  static all: FakeSocket[] = [];
  url: string;
  closed = false;
  constructor(url: string) {
    super();
    this.url = url;
    FakeSocket.all.push(this);
    queueMicrotask(() => this.dispatchEvent(new Event("open")));
  }
  close() {
    this.closed = true;
  }
  /** Coupure côté serveur ou réseau. */
  drop() {
    this.closed = true;
    act(() => {
      this.dispatchEvent(new Event("error"));
      this.dispatchEvent(new CloseEvent("close"));
    });
  }
  push(lines: WireLogLine[]) {
    act(() => {
      for (const l of lines) this.dispatchEvent(new MessageEvent("message", { data: JSON.stringify(l) }));
    });
  }
}

const socketFor = (param: string) => {
  const s = FakeSocket.all.find((x) => !x.closed && new URL(x.url).search.includes(param));
  if (!s) throw new Error(`aucun flux ouvert pour ${param} : ${FakeSocket.all.map((x) => x.url).join(", ")}`);
  return s;
};

const CONTAINERS: LogContainer[] = [
  { id: "web", service: "svc-web", img: "web:4821", st: "ok", col: CONTAINER_COLORS[0]! },
  { id: "worker", service: "svc-worker", img: "worker:4821", st: "ok", col: CONTAINER_COLORS[2]! },
  { id: "postgres", service: "svc-pg", img: "—", st: "off", col: CONTAINER_COLORS[3]! },
];

let writeText: ReturnType<typeof vi.fn>;

beforeEach(() => {
  FakeSocket.all = [];
  vi.stubGlobal("WebSocket", FakeSocket);
  writeText = vi.fn(async () => undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

const renderProject = () =>
  render(
    <ToastProvider>
      <LogViewer project="ciphera-web" containers={CONTAINERS} />
    </ToastProvider>,
  );

const messages = () => screen.queryAllByTestId("log-message").map((m) => m.textContent);

it("ouvre un flux par conteneur en marche et affiche ses lignes", () => {
  renderProject();
  const urls = FakeSocket.all.map((s) => new URL(s.url));
  expect(urls.map((u) => u.pathname)).toEqual(["/api/ws/logs", "/api/ws/logs"]);
  expect(urls.map((u) => u.searchParams.get("service"))).toEqual(["svc-web", "svc-worker"]);
  expect(urls.every((u) => u.searchParams.get("tail") === "500")).toBe(true);

  socketFor("svc-web").push(logLines("web", "web"));
  socketFor("svc-worker").push(logLines("worker", "worker").slice(0, 2));
  // Le niveau debug est masqué par l'onglet « Tout ».
  expect(messages()).toContain("GET / 200 12ms");
  expect(messages()).toContain("job deploy#4821 picked (SKIP LOCKED)");
  expect(messages()).not.toContain("cache hit key=dashboard:alex");
  expect(screen.getByText(/9 lignes affichées · tous les conteneurs/)).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: /Erreurs/ }));
  expect(messages()).toEqual(["Unhandled rejection: fetch failed (ECONNRESET) upstream=billing-api"]);
});

it("état vide « Aucun log », puis « Conteneur arrêté » pour un conteneur sans déploiement servi", () => {
  renderProject();
  expect(screen.getByText("Aucun log")).toBeInTheDocument();
  expect(screen.getByText("Modifiez la recherche ou le filtre de niveau.")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: /postgres/ }));
  expect(screen.getByText("Conteneur arrêté")).toBeInTheDocument();
  expect(screen.getByText("Les logs conservés sont disponibles au téléchargement.")).toBeInTheDocument();
  expect(screen.getByText("arrêté")).toBeInTheDocument();
  // Aucun flux pour un conteneur arrêté.
  expect(FakeSocket.all.some((s) => s.url.includes("svc-pg"))).toBe(false);
});

it("pause puis reprise : les lignes reçues en pause s'affichent à la reprise", () => {
  renderProject();
  const web = socketFor("svc-web");
  web.push(logLines("web", "web").slice(0, 1));
  expect(messages()).toEqual(["GET / 200 12ms"]);

  fireEvent.click(screen.getByRole("button", { name: /En direct/ }));
  expect(screen.getByRole("button", { name: /En pause/ })).toBeInTheDocument();
  web.push([{ ts: new Date().toISOString(), level: "info", container: "web", message: "pendant la pause" }]);
  expect(messages()).toEqual(["GET / 200 12ms"]);

  fireEvent.click(screen.getByRole("button", { name: /En pause/ }));
  expect(messages()).toEqual(["GET / 200 12ms", "pendant la pause"]);
});

it("recherche : filtre et surligne, sinon « Aucun log »", () => {
  renderProject();
  socketFor("svc-web").push(logLines("web", "web"));
  const input = screen.getByPlaceholderText("Rechercher dans les logs");
  fireEvent.change(input, { target: { value: "slow" } });
  expect(messages()).toEqual(["Slow query 812ms: SELECT * FROM deployments ORDER BY created_at"]);
  expect(document.querySelector("mark")?.textContent).toBe("Slow");

  fireEvent.change(input, { target: { value: "introuvable" } });
  expect(messages()).toEqual([]);
  expect(screen.getByText("Aucun log")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Effacer la recherche" }));
  expect(messages()).toHaveLength(7);
});

it("retour à la ligne", () => {
  renderProject();
  socketFor("svc-web").push(logLines("web", "web").slice(0, 1));
  const msg = () => screen.getByTestId("log-message");
  expect(msg()).toHaveStyle({ whiteSpace: "pre" });
  fireEvent.click(screen.getByTitle("Retour à la ligne"));
  expect(msg()).toHaveStyle({ whiteSpace: "pre-wrap" });
});

it("copie des lignes affichées et d'une ligne", async () => {
  renderProject();
  const lines = logLines("web", "web").slice(0, 2);
  socketFor("svc-web").push(lines);
  fireEvent.click(screen.getByTitle("Copier"));
  expect(writeText).toHaveBeenCalledWith(`${lines[0]!.ts} [INFO] web | GET / 200 12ms\n${lines[1]!.ts} [INFO] web | GET /api/projects 200 34ms`);
  expect(await screen.findByText("2 lignes copiées")).toBeInTheDocument();

  const row = screen.getAllByTestId("log-row")[1]!;
  fireEvent.mouseEnter(row);
  fireEvent.click(within(row).getByTitle("Copier la ligne"));
  expect(writeText).toHaveBeenLastCalledWith(`${lines[1]!.ts} [INFO] web | GET /api/projects 200 34ms`);
  expect(await screen.findByText("Ligne copiée")).toBeInTheDocument();
});

it("ferme les flux au démontage", () => {
  const { unmount } = renderProject();
  expect(FakeSocket.all).toHaveLength(2);
  unmount();
  expect(FakeSocket.all.every((s) => s.closed)).toBe(true);
});

it("logs de build d'un déploiement : flux du déploiement dans un bloc de code", () => {
  render(<LogViewer deployment="dep-1" />);
  expect(FakeSocket.all.map((s) => new URL(s.url).searchParams.get("deployment"))).toEqual(["dep-1"]);
  expect(screen.getByText("Aucun log")).toBeInTheDocument();
  socketFor("deployment=dep-1").push([
    { ts: "2026-11-22T10:00:00.000Z", level: "info", container: "build", message: "#1 [internal] load build definition" },
    { ts: "2026-11-22T10:00:01.000Z", level: "error", container: "build", message: "token=•••• refusé" },
  ]);
  expect(screen.getByText("#1 [internal] load build definition")).toBeInTheDocument();
  expect(screen.getByText("token=•••• refusé")).toBeInTheDocument();
  expect(screen.queryByText("Aucun log")).not.toBeInTheDocument();
});

it("conteneurs : un par service, en marche selon l'état calculé par le serveur, CPU et mémoire de l'agent", () => {
  const service = (over: Partial<ProjectService>): ProjectService => ({ id: "s1", name: "web", environment: "production", state: "running", deployment_number: 42, replicas: { running: 1, desired: 1 }, ...over });
  expect(containersOf([service({ cpu_percent: 12.5, memory_bytes: 256 * 1024 * 1024 }), service({ id: "s2", name: "worker", state: "stopped" }), service({ id: "s3", name: "api", state: "none", deployment_number: null })])).toEqual([
    { id: "web", service: "s1", img: "web:42", st: "ok", col: CONTAINER_COLORS[0], cpu: 12.5, mem: 256 * 1024 * 1024 },
    { id: "worker", service: "s2", img: "worker:42", st: "off", col: CONTAINER_COLORS[1], cpu: null, mem: null },
    { id: "api", service: "s3", img: "—", st: "off", col: CONTAINER_COLORS[2], cpu: null, mem: null },
  ]);
  expect(formatMem(212 * 1024 * 1024)).toBe("212 Mo");
  expect(formatMem(1.1 * 1024 * 1024 * 1024)).toBe("1,1 Go");
});

it("URL WebSocket : même origine, wss sous https", () => {
  expect(wsUrl("/api/ws/logs", { service: "s1", tail: 500, deployment: undefined })).toBe(`ws://${location.host}/api/ws/logs?service=s1&tail=500`);
});

const wire = (sec: number, message: string): WireLogLine => ({ ts: `2026-11-22T10:00:0${sec}.000Z`, level: "info", container: "build", message });
const opened = () => act(async () => {});

it("flux coupé : « reconnexion… » puis flux rouvert, sans doublon des lignes renvoyées", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  try {
    render(<LogViewer deployment="dep-1" />);
    await opened();
    socketFor("deployment=dep-1").push([wire(0, "étape 1"), wire(1, "étape 2")]);
    expect(screen.queryByRole("status")).not.toBeInTheDocument();

    socketFor("deployment=dep-1").drop();
    expect(screen.getByRole("status")).toHaveTextContent("Connexion perdue, reconnexion…");
    // Une seule reconnexion par coupure (error puis close), après le délai.
    act(() => vi.advanceTimersByTime(reconnectDelay(0) - 1));
    expect(FakeSocket.all).toHaveLength(1);
    act(() => vi.advanceTimersByTime(1));
    expect(FakeSocket.all).toHaveLength(2);
    await opened();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();

    // Le serveur renvoie les lignes récentes : seules les nouvelles s'ajoutent.
    socketFor("deployment=dep-1").push([wire(0, "étape 1"), wire(1, "étape 2"), wire(2, "étape 3")]);
    expect(screen.getAllByText(/^étape/).map((e) => e.textContent)).toEqual(["étape 1", "étape 2", "étape 3"]);
  } finally {
    vi.useRealTimers();
  }
});

it("flux coupé : l'indicateur passe de « En direct » à « Reconnexion… »", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  try {
    renderProject();
    await opened();
    expect(screen.getByText("En direct")).toBeInTheDocument();
    socketFor("svc-web").drop();
    expect(screen.queryByText("En direct")).not.toBeInTheDocument();
    expect(screen.getByText("Reconnexion…")).toBeInTheDocument();
    act(() => vi.advanceTimersByTime(reconnectDelay(0)));
    await opened();
    expect(screen.getByText("En direct")).toBeInTheDocument();
  } finally {
    vi.useRealTimers();
  }
});

it("reconnexion : délai croissant, plafonné, et annulée au démontage", () => {
  expect([0, 1, 2, 3, 4, 5, 6].map(reconnectDelay)).toEqual([1000, 2000, 4000, 8000, 16000, 30000, 30000]);
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  try {
    const { unmount } = render(<LogViewer deployment="dep-1" />);
    socketFor("deployment=dep-1").drop();
    unmount();
    act(() => vi.advanceTimersByTime(60_000));
    expect(FakeSocket.all).toHaveLength(1);
  } finally {
    vi.useRealTimers();
  }
});
