import { useQuery } from "@tanstack/react-query";

import { fetchSystemVersion, SYSTEM_VERSION_QUERY_KEY } from "../api";
import { Card, Note, Pill, Row } from "../ui";

/** Avertissement d'un `latest.json` dont la signature a plus de 120 jours (rien n'est bloque). */
export const LATEST_EXPIRED_MESSAGE = "Impossible de garantir que Ciphera est à jour (métadonnées de version de plus de 120 jours)";

const mono = { font: "400 13px/1 var(--font-mono)", color: "var(--text-strong)" } as const;

/**
 * Parametres › General : etat de la mise a jour (`GET /api/system/version`, administrateur).
 * La derniere version n'est connue que si la verification est activee (`CIPHERA_UPDATE_CHECK=1`)
 * et que `latest.json` est signe par une cle de confiance ; sinon (desactivee, injoignable,
 * signature invalide) : « Non vérifiée ».
 */
export function UpdateStatus() {
  const query = useQuery({ queryKey: SYSTEM_VERSION_QUERY_KEY, queryFn: fetchSystemVersion });
  if (!query.data) return null;
  const { current, latest, latest_expired, changelog_url } = query.data;
  const newer = latest !== undefined && latest !== null && latest !== current;
  return (
    <Card title="Version de Ciphera" desc="Version installée et dernière version publiée, lue dans les métadonnées signées des releases.">
      <Row t="Version installée" first>
        <span style={mono}>{current}</span>
      </Row>
      <Row
        t="Dernière version publiée"
        d={
          latest
            ? changelog_url && (
                <a href={changelog_url} target="_blank" rel="noreferrer noopener" style={{ color: "var(--text-strong)" }}>
                  Notes de version
                </a>
              )
            : "Vérification désactivée (CIPHERA_UPDATE_CHECK) ou impossible pour le moment."
        }
      >
        {latest ? (
          <Pill tone={newer ? "muted" : "ok"} icon={newer ? "ArrowUpRight" : "CircleCheck"} mono>
            {newer ? `${latest} disponible` : `${latest} : à jour`}
          </Pill>
        ) : (
          <Pill tone="muted" icon="CircleMinus">
            Non vérifiée
          </Pill>
        )}
      </Row>
      {latest_expired && (
        <Note icon="TriangleExclamationPoint" color="var(--amber-500)">
          <span role="alert">{LATEST_EXPIRED_MESSAGE}</span>
        </Note>
      )}
    </Card>
  );
}
