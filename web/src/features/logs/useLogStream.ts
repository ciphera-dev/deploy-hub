import { useCallback, useEffect, useState } from "react";

import { api } from "@/lib/api/client";
import { notifySessionExpired } from "@/lib/api/session";
import { openJsonSocket, wsUrl } from "@/lib/ws";

/** Message de GET /api/ws/logs (lignes déjà masquées par le serveur). */
export interface WireLogLine {
  ts: string;
  level: string;
  container: string;
  message: string;
}

export type LogLevel = "info" | "warn" | "error" | "debug";

/** Ligne affichée (forme de ProjectLogs.jsx#mk). */
export interface LogLine {
  id: number;
  /** horodatage en millisecondes */
  t: number;
  /** HH:MM:SS.mmm (heure locale) */
  ts: string;
  /** ISO 8601 */
  date: string;
  /** conteneur */
  c: string;
  lv: LogLevel;
  msg: string;
}

/** Une cible du flux : un déploiement (build) ou un service (exécution). */
export type LogSource = { deployment: string } | { service: string; tail?: number };

/** Lignes gardées en mémoire (ProjectLogs.jsx : slice(-1500)). */
export const MAX_LINES = 1500;

const LEVELS: readonly string[] = ["info", "warn", "error", "debug"];
const pad = (n: number, w = 2) => String(n).padStart(w, "0");

let counter = 0;

export function toLine(wire: WireLogLine): LogLine {
  const parsed = Date.parse(wire.ts);
  const d = new Date(Number.isNaN(parsed) ? Date.now() : parsed);
  return {
    id: ++counter,
    t: d.getTime(),
    ts: `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}`,
    date: d.toISOString(),
    c: wire.container,
    lv: (LEVELS.includes(wire.level) ? wire.level : "info") as LogLevel,
    msg: wire.message,
  };
}

const sourceKey = (s: LogSource) => ("deployment" in s ? `d:${s.deployment}` : `s:${s.service}:${s.tail ?? ""}`);

let probing: Promise<void> | null = null;

/** GET /api/me (une seule sonde à la fois) : un 401 signale la session expirée, tout autre résultat est ignoré. */
function probeSession(): void {
  probing ??= api
    .GET("/api/me")
    .then((result) => {
      if (result.response.status === 401) notifySessionExpired();
    })
    .catch(() => undefined)
    .finally(() => {
      probing = null;
    });
}

/** Lignes en direct des cibles données (une connexion par cible, rouverte après une coupure). */
export function useLogStream(sources: LogSource[]) {
  const key = sources.map(sourceKey).join("|");
  // Lignes rattachées à la clé des cibles : un changement de cibles repart de zéro.
  const [store, setStore] = useState<{ key: string; lines: LogLine[] }>({ key, lines: [] });
  // Cibles dont le flux est coupé (reconnexion en cours).
  const [down, setDown] = useState<{ key: string; sources: string[] }>({ key, sources: [] });
  // Cibles dont le serveur a fermé le flux (accès retiré, 1008) : plus de reconnexion.
  const [ended, setEnded] = useState<{ key: string; sources: string[] }>({ key, sources: [] });

  useEffect(() => {
    if (!key) return;
    const closers = key.split("|").map((k) => {
      const [kind, id, tail] = k.split(":");
      const params = kind === "d" ? { deployment: id } : { service: id, tail: tail || undefined };
      // Dernière ligne reçue : après une reconnexion, le serveur renvoie les lignes
      // récentes, écartées jusqu'à la première plus récente.
      let lastT = -Infinity;
      let atLast = new Set<string>();
      let resuming = false;
      return openJsonSocket<WireLogLine>(wsUrl("/api/ws/logs", params), {
        onMessage: (wire) => {
          const line = toLine(wire);
          const sig = `${line.c}\n${line.msg}`;
          if (resuming) {
            if (line.t < lastT || (line.t === lastT && atLast.has(sig))) return;
            if (line.t > lastT) resuming = false;
          }
          if (line.t > lastT) {
            lastT = line.t;
            atLast = new Set();
          }
          if (line.t === lastT) atLast.add(sig);
          setStore((s) => ({ key, lines: [...(s.key === key ? s.lines : []), line].slice(-MAX_LINES) }));
        },
        // Flux injoignable ou refusé : GET /api/me, hors du cache des requêtes (une coupure
        // réseau ou un 5xx ne doit pas remplacer la page : le flux continue de se rouvrir).
        // Seul un 401 applique la politique de session (cache vidé, redirection).
        onUnreachable: probeSession,
        onStatus: (status) => {
          if (status === "reconnecting") resuming = true;
          setEnded((e) => {
            const prev = e.key === key ? e.sources : [];
            return { key, sources: status === "closed" ? [...new Set([...prev, k])] : prev.filter((x) => x !== k) };
          });
          setDown((d) => {
            const prev = d.key === key ? d.sources : [];
            const next = status === "reconnecting" ? [...new Set([...prev, k])] : prev.filter((x) => x !== k);
            return { key, sources: next };
          });
        },
      });
    });
    return () => closers.forEach((close) => close());
  }, [key]);

  const clear = useCallback(() => setStore({ key, lines: [] }), [key]);
  const lines = store.key === key ? store.lines : [];
  const reconnecting = down.key === key && down.sources.length > 0;
  const closed = ended.key === key && ended.sources.length > 0;
  return { lines, clear, reconnecting, closed };
}
