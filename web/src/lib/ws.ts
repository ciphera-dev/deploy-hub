// WebSocket du panneau vers sa propre origine (ws:// ou wss:// selon la page).
// Le cookie de session part seul (même origine) ; le serveur vérifie l'en-tête
// Origin que le navigateur pose. Aucune donnée n'est envoyée par le client.

/** État d'un flux : ouverture, ouvert, coupé (reconnexion programmée), fermé pour de bon (accès refusé). */
export type SocketStatus = "connecting" | "open" | "reconnecting" | "closed";

export interface JsonSocketHandlers<T> {
  onMessage: (data: T) => void;
  onStatus?: (status: SocketStatus) => void;
  /**
   * Le flux n'aboutit pas (3 échecs de suite sans ouverture) ou le serveur l'a
   * fermé pour accès refusé (1008) : l'appelant vérifie la session (GET /api/me).
   */
  onUnreachable?: () => void;
}

/** Code de fermeture « policy violation » : accès retiré ou refusé, inutile de réessayer. */
export const CLOSE_POLICY = 1008;
/** Échecs consécutifs sans ouverture avant de vérifier la session. */
export const PROBE_AFTER_FAILURES = 3;

/** Attente avant la reconnexion n (0, 1, ...) : 1 s, 2 s, 4 s... au plus 30 s. */
export const reconnectDelay = (attempt: number) => Math.min(1000 * 2 ** attempt, 30_000);

/** URL ws(s):// d'un chemin de l'API sur l'origine courante. */
export function wsUrl(path: string, params: Record<string, string | number | undefined>): string {
  const origin = globalThis.location?.origin ?? "http://localhost";
  const url = new URL(path, origin);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  for (const [k, v] of Object.entries(params)) if (v !== undefined) url.searchParams.set(k, String(v));
  return url.toString();
}

/**
 * Ouvre un flux de messages JSON, rouvert avec un délai croissant après une coupure ;
 * renvoie la fonction de fermeture. Un message illisible est ignoré.
 */
export function openJsonSocket<T>(url: string, handlers: JsonSocketHandlers<T>): () => void {
  let closed = false;
  let attempt = 0;
  let failedOpens = 0;
  let socket: WebSocket | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const connect = () => {
    const current = new WebSocket(url);
    socket = current;
    let dropped = false;
    let opened = false;
    current.addEventListener("open", () => {
      if (closed || socket !== current) return;
      opened = true;
      attempt = 0;
      failedOpens = 0;
      handlers.onStatus?.("open");
    });
    current.addEventListener("message", (event: MessageEvent) => {
      if (closed || socket !== current || typeof event.data !== "string") return;
      let data: T;
      try {
        data = JSON.parse(event.data) as T;
      } catch {
        return;
      }
      handlers.onMessage(data);
    });
    // `error` est toujours suivi de `close` (qui porte le code) : seul `close` compte.
    const drop = (event: Event) => {
      if (closed || socket !== current || dropped) return;
      dropped = true;
      // 1008 : le serveur refuse ce flux ; on ne rouvre pas, on vérifie la session.
      if ((event as CloseEvent).code === CLOSE_POLICY) {
        handlers.onStatus?.("closed");
        handlers.onUnreachable?.();
        return;
      }
      if (!opened && ++failedOpens % PROBE_AFTER_FAILURES === 0) handlers.onUnreachable?.();
      handlers.onStatus?.("reconnecting");
      timer = setTimeout(connect, reconnectDelay(attempt++));
    };
    current.addEventListener("close", drop);
  };

  connect();
  return () => {
    closed = true;
    clearTimeout(timer);
    socket?.close();
  };
}
