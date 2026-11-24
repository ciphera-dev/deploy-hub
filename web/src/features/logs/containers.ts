import type { ProjectService } from "@/features/projects/overview/api";

import { CONTAINER_COLORS, type LogContainer } from "./LogViewer";

/**
 * Un conteneur par service du projet, avec l'état calculé par le serveur
 * (`GET /api/projects/{id}/services`) : en marche s'il tourne, arrêté sinon.
 * Image affichée comme dans la maquette : `<service>:<numéro du déploiement servi>`.
 * CPU et mémoire : dernière mesure de l'agent pour un service en marche.
 */
export function containersOf(services: ProjectService[]): LogContainer[] {
  return services.map((s, i) => ({
    id: s.name,
    service: s.id,
    img: s.deployment_number != null ? `${s.name}:${s.deployment_number}` : "—",
    st: s.state === "running" ? "ok" : "off",
    col: CONTAINER_COLORS[i % CONTAINER_COLORS.length]!,
    cpu: s.cpu_percent ?? null,
    mem: s.memory_bytes ?? null,
  }));
}
