import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useRef, useState, type CSSProperties } from "react";

import { Badge, Button, Icon, IconButton, Menu, Modal, StatusDot, Switch, TextField, useToast } from "@/design-system";
import { fetchMe, ME_QUERY_KEY, MFA_PENDING } from "@/features/auth/api";
import { useCanDeploy } from "@/features/deployments/useDeploymentActions";

import type { Project } from "../api";
import { RUN_AS_RE, serviceAction, updateServiceSecurity, type ProjectService, type ServiceAction } from "./api";
import { Bar } from "./charts";
import { imageIcon, imageLabel, replicasLabel, SERVICE_STATE_VIEW } from "./format";

// ProjectOverview.jsx#ProjectServices et #ServiceCard. Redémarrer et Arrêter
// visent le déploiement servi du service (developer et plus). Portés mais
// inactifs (v0.2) : terminal (ADR 0002), ressources et limites, mise à
// l'échelle des réplicas et ajout de service (un service par environnement
// en v0.1). CPU et mémoire arrivent avec les métriques : « — » en attendant.

const V02 = "Disponible en v0.2";
const DEV_ONLY = "Réservé aux rôles développeur et plus";
const hair = "1px solid var(--border-subtle)";
const lbl: CSSProperties = { font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" };
const card: CSSProperties = { borderRadius: "var(--radius-xl)", border: "1px solid var(--border-default)", background: "var(--surface-app)" };
const step: CSSProperties = { width: 24, height: 24, border: 0, background: "transparent", color: "var(--text-muted)", cursor: "not-allowed", display: "flex", alignItems: "center", justifyContent: "center", opacity: 0.4 };

/** Sécurité du conteneur : utilisateur (`uid[:gid]`) et racine en lecture seule ; valable aux prochains déploiements. */
function SecurityModal({ project, s, onClose, onDone }: { project: Project; s: ProjectService; onClose: () => void; onDone: () => void }) {
  const { flash } = useToast();
  const [runAs, setRunAs] = useState(s.run_as ?? "");
  const [readOnly, setReadOnly] = useState(s.read_only_rootfs);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const value = runAs.trim();
  const valid = value === "" || RUN_AS_RE.test(value);
  const save = async () => {
    setPending(true);
    setError(null);
    const r = await updateServiceSecurity(project.id, s.id, { run_as: value === "" ? null : value, read_only_rootfs: readOnly });
    setPending(false);
    if (!r.ok) return setError(r.message);
    flash("Réglages de sécurité enregistrés : ils s'appliquent au prochain déploiement.");
    onDone();
    onClose();
  };
  return (
    <Modal
      title="Sécurité du conteneur"
      description={s.name}
      onClose={onClose}
      width={520}
      footer={
        <>
          <Button onClick={onClose}>Annuler</Button>
          <Button variant="inverse" disabled={!valid || pending} onClick={() => void save()}>
            Enregistrer
          </Button>
        </>
      }
    >
      <TextField
        label="Utilisateur (uid ou uid:gid)"
        hint={valid ? "Vide : l'utilisateur de l'image. Exécuter sans root est recommandé, par exemple 1000:1000." : "Format attendu : 1000 ou 1000:1000 (numérique)."}
        value={runAs}
        onChange={setRunAs}
        placeholder="1000:1000"
        mono
      />
      <Switch label="Racine en lecture seule" description="Le système de fichiers de l'image n'est plus modifiable ; /tmp reste accessible en écriture (64 Mo)." checked={readOnly} onChange={setReadOnly} />
      <span style={lbl}>Compose : ces réglages valent pour le service exposé ; les autres services gardent leur fichier Compose.</span>
      {error && (
        <span role="alert" style={{ font: "400 12px/1.4 var(--font-sans)", color: "var(--text-negative)" }}>
          {error}
        </span>
      )}
    </Modal>
  );
}

/** `Projects:Write` : développeur, admin, owner (ops lit seulement). */
function useCanWriteProjects(): boolean {
  const me = useQuery({ queryKey: ME_QUERY_KEY, queryFn: fetchMe, retryOnMount: false, refetchOnMount: false });
  return !!me.data && me.data !== MFA_PENDING && ["developer", "admin", "owner"].includes(me.data.role);
}

function ServiceCard({ project, s, canDeploy, onLogs, onDone }: { project: Project; s: ProjectService; canDeploy: boolean; onLogs: () => void; onDone: () => void }) {
  const { flash } = useToast();
  const [busy, setBusy] = useState<ServiceAction | null>(null);
  const inFlight = useRef(false);
  const [security, setSecurity] = useState(false);
  const canWrite = useCanWriteProjects();
  const view = SERVICE_STATE_VIEW[s.state];
  const serving = !!s.deployment_id;
  const run = async (action: ServiceAction) => {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(action);
    try {
      const result = await serviceAction(project.id, s.id, action);
      flash(result.ok ? (action === "restart" ? `Redémarrage de ${s.name} demandé` : `Arrêt de ${s.name} demandé`) : result.message, result.ok ? undefined : 5000);
      if (result.ok) onDone();
    } finally {
      inFlight.current = false;
      setBusy(null);
    }
  };
  const actionHint = !canDeploy ? DEV_ONLY : !serving ? "Aucun déploiement en ligne" : undefined;
  const host = s.name + ".internal" + (s.internal_port ? ":" + s.internal_port : "");
  const { running, desired } = s.replicas;
  return (
    <article aria-label={s.name} style={{ ...card, display: "flex", flexDirection: "column" }}>
      <div style={{ display: "flex", alignItems: "flex-start", gap: 12, padding: "18px 18px 16px" }}>
        <span style={{ width: 36, height: 36, borderRadius: 10, display: "flex", alignItems: "center", justifyContent: "center", background: "var(--surface-selected)", flex: "none" }}>
          <Icon name="Globe" size={17} color="var(--text-strong)" />
        </span>
        <div style={{ display: "flex", flexDirection: "column", gap: 7, flex: 1, minWidth: 0 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <span style={{ font: "500 15px/1 var(--font-sans)", color: "var(--text-strong)" }}>{s.name}</span>
            <Badge>Application</Badge>
            {s.runs_as_root && (
              <button type="button" aria-label="S'exécute en root : ouvrir les réglages de sécurité" title="S'exécute en root. Réglages de sécurité" onClick={() => setSecurity(true)} style={{ border: 0, padding: 0, background: "transparent", cursor: canWrite ? "pointer" : "default" }} disabled={!canWrite}>
                <Badge style={{ background: "var(--surface-selected)", color: "var(--amber-500)" }}>root</Badge>
              </button>
            )}
          </div>
          <span style={{ ...lbl, display: "flex", alignItems: "center", gap: 6 }}>
            <Icon name={imageIcon(project)} size={12} />
            {imageLabel(project)}
          </span>
        </div>
        <Menu
          width={200}
          items={[
            { icon: "Terminal", label: "Ouvrir un terminal", onSelect: () => {}, disabled: true, tag: "v0.2" },
            { icon: "SlidersHorizontal", label: "Ressources & limites", onSelect: () => {}, disabled: true, tag: "v0.2" },
            { icon: "Shield", label: "Sécurité", onSelect: () => setSecurity(true), disabled: !canWrite },
            { icon: "Square", label: "Arrêter", onSelect: () => void run("stop"), disabled: !canDeploy || !serving || s.state === "stopped" || busy !== null },
          ]}
        />
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "0 18px 16px" }}>
        <StatusDot status={busy === "restart" ? "pending" : view.dot} label={busy === "restart" ? "Redémarrage…" : view.label} />
        <span style={{ flex: 1 }} />
        <span style={{ display: "flex", alignItems: "center", gap: 6, height: 24, padding: "0 8px", borderRadius: 6, background: "var(--surface-raised)", maxWidth: "60%" }}>
          <Icon name="Network" size={11} color="var(--text-subtle)" />
          <span style={{ font: "400 11px/1 var(--font-mono)", color: "var(--text-muted)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{host}</span>
        </span>
      </div>
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16, padding: "16px 18px", borderTop: hair }}>
        {(
          [
            ["CPU", s.cpu_percent == null ? "—" : `${Math.round(s.cpu_percent)} %`, s.cpu_percent],
            ["Mémoire", s.memory_bytes == null ? "—" : `${Math.round(s.memory_bytes / 1_048_576)} Mo`, null],
          ] as const
        ).map(([k, v, p]) => (
          <div key={k} style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <div style={{ display: "flex", justifyContent: "space-between" }}>
              <span style={lbl}>{k}</span>
              <span style={{ font: "500 12px/1 var(--font-mono)", color: "var(--text-strong)" }}>{v}</span>
            </div>
            <Bar v={p} />
          </div>
        ))}
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "12px 12px 12px 18px", borderTop: hair, marginTop: "auto" }}>
        <span style={lbl}>Réplicas</span>
        <div aria-label={`Réplicas ${replicasLabel(s)}`} style={{ display: "flex", gap: 3 }}>
          {Array.from({ length: Math.max(desired, 1) }, (_, i) => (
            <span key={i} style={{ width: 8, height: 8, borderRadius: 2, background: i < running ? "var(--green-500)" : "var(--viz-3)" }} />
          ))}
        </div>
        <div title={V02} style={{ display: "flex", alignItems: "center", borderRadius: "var(--radius-md)", border: "1px solid var(--border-strong)", background: "var(--surface-control)", height: 26 }}>
          <button type="button" aria-label="Retirer un réplica" disabled title={V02} style={step}>
            <Icon name="Minus" size={12} />
          </button>
          <span style={{ minWidth: 18, textAlign: "center", font: "500 12px/1 var(--font-mono)", color: "var(--text-strong)" }}>{desired}</span>
          <button type="button" aria-label="Ajouter un réplica" disabled title={V02} style={step}>
            <Icon name="Plus" size={12} />
          </button>
        </div>
        <span style={{ flex: 1 }} />
        <IconButton size={28} variant="ghost" icon="ScrollText" label="Logs" onClick={onLogs} />
        <IconButton size={28} variant="ghost" icon="RotateCcw" label="Redémarrer" title={actionHint} disabled={!canDeploy || !serving || busy !== null} onClick={() => void run("restart")} style={!canDeploy || !serving ? { opacity: 0.4, cursor: "default" } : undefined} />
      </div>
      {security && <SecurityModal project={project} s={s} onClose={() => setSecurity(false)} onDone={onDone} />}
    </article>
  );
}

export function ProjectServices({ project, services, onChanged }: { project: Project; services: ProjectService[]; onChanged: () => void }) {
  const navigate = useNavigate();
  const canDeploy = useCanDeploy();
  return (
    <div style={{ padding: "24px 32px 40px 34px", display: "flex", flexDirection: "column", gap: 16 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <span style={{ font: "400 12px/1.4 var(--font-sans)", color: "var(--text-subtle)" }}>
          Les services communiquent sur le réseau privé du projet via <span style={{ fontFamily: "var(--font-mono)", color: "var(--text-body)" }}>nom.internal</span>.
        </span>
        <span style={{ flex: 1 }} />
        <Button variant="inverse" icon="Plus" disabled title={V02}>
          Ajouter un service
        </Button>
      </div>
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill,minmax(300px,1fr))", gap: 12 }}>
        {services.map((s) => (
          <ServiceCard key={s.id} project={project} s={s} canDeploy={canDeploy} onDone={onChanged} onLogs={() => void navigate({ to: "/projects/$projectId/logs", params: { projectId: project.id } })} />
        ))}
        <button type="button" disabled title={V02} style={{ ...card, minHeight: 220, borderStyle: "dashed", borderColor: "var(--border-strong)", background: "transparent", display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 10, cursor: "not-allowed", color: "var(--text-muted)", opacity: 0.6 }}>
          <span style={{ width: 36, height: 36, borderRadius: 99, border: "1px dashed var(--gray-500)", display: "flex", alignItems: "center", justifyContent: "center" }}>
            <Icon name="Plus" size={15} />
          </span>
          <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-body)" }}>Nouveau service</span>
          <span style={lbl}>Base de données, cache, worker, image…</span>
        </button>
      </div>
    </div>
  );
}
