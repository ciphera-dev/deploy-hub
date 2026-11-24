import { Icon, StatusDot } from "@/design-system";
import "@/features/deployments/deployments.css";
import { isActive, type Deployment, type DeploymentStage } from "@/features/deployments/api";
import { formatStepSeconds } from "@/features/deployments/format";

// Projects.jsx#DeployAnim + étape 3 de NewProjectModal : progression du premier
// déploiement. Les 4 colonnes de la maquette (Clonage, Build, Image, En ligne)
// viennent des étapes de l'API : « En ligne » regroupe Démarrage, Healthcheck et
// Bascule. Les logs de build en direct arrivent avec la Task 26 (zone sans lignes factices).

const GROUPS: { label: string; stages: DeploymentStage[] }[] = [
  { label: "Clonage", stages: ["clone"] },
  { label: "Build", stages: ["build"] },
  { label: "Image", stages: ["image"] },
  { label: "En ligne", stages: ["start", "healthcheck", "switch"] },
];

type GroupState = "done" | "running" | "failed" | "pending";

function groupState(d: Deployment, stages: DeploymentStage[]): { state: GroupState; ms: number | null } {
  const steps = stages.map((s) => d.steps.find((x) => x.stage === s)).filter((x) => !!x);
  if (!steps.length) return { state: "pending", ms: null };
  const ms = steps.reduce((sum, s) => sum + (s.duration_ms ?? 0), 0);
  if (steps.some((s) => s.status === "failed") || (d.status === "failed" && steps.some((s) => s.status === "running"))) return { state: "failed", ms };
  const last = stages[stages.length - 1];
  const lastDone = d.steps.find((s) => s.stage === last)?.status === "succeeded";
  if (lastDone) return { state: "done", ms };
  return { state: d.status === "cancelled" ? "pending" : "running", ms: null };
}

export function progressTitle(d: Deployment | null): string {
  if (!d) return "Lancement du déploiement…";
  return { queued: "En file d'attente…", running: "Build en cours…", success: "Projet en ligne", failed: "Échec du déploiement", cancelled: "Déploiement annulé" }[d.status];
}

export function DeployProgress({ deployment: d, subtitle }: { deployment: Deployment | null; subtitle?: string }) {
  const states = GROUPS.map((g) => (d ? groupState(d, g.stages) : { state: "pending" as const, ms: null }));
  const doneCount = states.filter((s) => s.state === "done").length;
  const pct = d?.status === "success" ? 100 : (doneCount / GROUPS.length) * 100;
  const finished = !!d && d.status === "success";
  const failed = !!d && d.status === "failed";
  const accent = failed ? "#f87171" : finished ? "#86efac" : "#a78bfa";
  return (
    <>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <span style={{ font: "500 16px/1.2 var(--font-sans)", color: "var(--text-strong)" }}>{progressTitle(d)}</span>
          {subtitle && <span style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" }}>{subtitle}</span>}
        </div>
        {d && finished ? <StatusDot status="ok" label="En ligne" /> : d && failed ? <StatusDot status="error" label="Échec" /> : d && !isActive(d) ? <StatusDot status="off" label="Annulé" /> : <StatusDot status="pending" label={`${Math.round(pct)} %`} />}
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 16, padding: "18px 20px", borderRadius: 14, border: "1px solid rgba(255,255,255,.08)", background: "rgba(255,255,255,.02)" }}>
        <ol aria-label="Progression du déploiement" style={{ position: "relative", margin: 0, padding: 0, listStyle: "none", display: "grid", gridTemplateColumns: "repeat(4,1fr)" }}>
          <span style={{ position: "absolute", left: "12.5%", right: "12.5%", top: 11, height: 2, borderRadius: 2, background: "rgba(255,255,255,.08)" }} />
          <span style={{ position: "absolute", left: "12.5%", top: 11, height: 2, borderRadius: 2, width: `${Math.min(100, pct) * 0.75}%`, background: accent, transition: "width .4s ease-out, background .4s" }} />
          {GROUPS.map((g, i) => {
            const { state, ms } = states[i];
            const ok = state === "done";
            const act = state === "running";
            const ko = state === "failed";
            return (
              <li key={g.label} data-state={state} style={{ position: "relative", display: "flex", flexDirection: "column", alignItems: "center", gap: 8 }}>
                <span style={{ position: "relative", width: 24, height: 24, borderRadius: 99, display: "flex", alignItems: "center", justifyContent: "center", background: ok ? accent : ko ? "#f87171" : "#0c0c0f", border: "2px solid " + (ok ? accent : ko ? "#f87171" : act ? "#a78bfa" : "rgba(255,255,255,.14)"), transition: "all .3s" }}>
                  {act && <span className="dep-pulse" style={{ position: "absolute", inset: -6, borderRadius: 99, border: "1px solid rgba(167,139,250,.6)" }} />}
                  {ok ? <Icon name="Check" size={12} color="#0b0b0b" /> : ko ? <Icon name="X" size={12} color="#fff" /> : act ? <span style={{ width: 8, height: 8, borderRadius: 99, background: "#a78bfa" }} /> : null}
                </span>
                <span style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 4 }}>
                  <span style={{ font: "500 12px/1 var(--font-sans)", color: ok || act || ko ? "var(--text-strong)" : "var(--text-subtle)" }}>{g.label}</span>
                  <span style={{ font: "400 11px/1 var(--font-mono)", color: "var(--text-faint)" }}>{ok || ko ? formatStepSeconds(ms) : act ? "…" : "—"}</span>
                </span>
              </li>
            );
          })}
        </ol>
      </div>
      {d?.status === "failed" && d.error && (
        <div role="alert" style={{ padding: "10px 14px", borderRadius: "var(--radius-lg)", background: "rgba(239,68,68,.05)", border: "1px solid rgba(239,68,68,.18)", font: "400 12px/1.4 var(--font-sans)", color: "var(--text-body)" }}>
          {d.error}
        </div>
      )}
      <section aria-label="Logs de build" style={{ padding: "18px 16px", borderRadius: "var(--radius-lg)", background: "var(--gray-0)", border: "1px solid var(--border-default)", font: "400 12px/1.5 var(--font-sans)", color: "var(--text-subtle)" }}>
        Les logs de build en direct seront disponibles prochainement.
      </section>
    </>
  );
}
