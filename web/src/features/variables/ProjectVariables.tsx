import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useRef, useState, type CSSProperties } from "react";

import { Badge, Button, Icon, IconButton, Tabs, useToast } from "@/design-system";
import { fetchMe, ME_QUERY_KEY, MFA_PENDING } from "@/features/auth/api";
import { useDeploymentActions } from "@/features/deployments/useDeploymentActions";
import type { Project } from "@/features/projects/api";
import { timeAgo } from "@/features/projects/format";
import { ApiError } from "@/lib/api/errors";
import { copyText } from "@/lib/clipboard";

import { bulkProjectVariables, deleteProjectVariable, download, envLabel, fetchProjectVariables, fetchVariablesStatus, projectVariablesQueryKey, putProjectVariable, shortSecretKeys, shortSecretNotice, variablesStatusQueryKey, type ProjectVariable } from "./api";
import { dotenvLine } from "./dotenv";

// Port de prototypes/dashboard/ProjectVariables.jsx (structure, libellés, tokens).
// Écarts imposés par le backend v0.1 :
// - un seul environnement par projet (`production`) : onglets et pastilles
//   d'environnement ne proposent que ceux du projet ;
// - un secret n'est jamais relu (« Laisser vide pour conserver ») ; le .env
//   téléchargé ne contient pas les secrets ;
// - « non appliquée » vient du serveur (modifiée après la lecture des
//   variables par le dernier déploiement réussi ; une suppression compte,
//   `GET …/variables/status`) ; « Enregistrer et redéployer » lance le
//   redéploiement (les variables sont déjà enregistrées), « Annuler » masque
//   le bandeau ;
// - les globales remplacées par le projet ne sont pas listées par l'API
//   (seule la valeur effective l'est) ;
// - écriture réservée aux rôles développeur et plus (ops : lecture).

const hair = "1px solid var(--border-subtle)";
const lbl: CSSProperties = { font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" };
const field: CSSProperties = { height: 32, padding: "0 10px", borderRadius: "var(--radius-md)", border: "1px solid var(--border-strong)", background: "var(--gray-0)", color: "var(--text-strong)", font: "400 12px/1 var(--font-mono)", outline: "none", minWidth: 0 };
const cell: CSSProperties = { minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" };
const COLS = "minmax(0,1.2fr) minmax(0,1.6fr) minmax(0,1.1fr) 120px 92px";

type Draft = { k: string; v: string; secret: boolean; envs: string[] };

/** Rôle du compte chargé par le shell, sans nouvelle requête (voir useCanDeploy). */
function useRole(): string | null {
  const me = useQuery({ queryKey: ME_QUERY_KEY, queryFn: fetchMe, retryOnMount: false, refetchOnMount: false });
  return me.data && me.data !== MFA_PENDING ? me.data.role : null;
}

function EnvChips({ envs, value, onChange, small }: { envs: string[]; value: string[]; onChange?: (v: string[]) => void; small?: boolean }) {
  return (
    <div style={{ display: "flex", gap: 4, flexWrap: "wrap" }}>
      {envs.map((name) => {
        const e = envLabel(name);
        const on = value.includes(name);
        return (
          <button
            key={name}
            type="button"
            aria-pressed={on}
            onClick={onChange ? () => onChange(on ? value.filter((x) => x !== name) : [...value, name]) : undefined}
            style={{ height: small ? 20 : 24, padding: "0 7px", borderRadius: 6, border: "1px solid " + (on ? "var(--border-strong)" : "transparent"), background: on ? "var(--surface-selected)" : "transparent", color: on ? "var(--text-strong)" : "var(--text-faint)", font: "500 11px/1 var(--font-sans)", cursor: onChange ? "pointer" : "default", textDecoration: !on && !onChange ? "line-through" : "none" }}
          >
            {e.slice(0, 4)}
            {e.length > 4 ? "." : ""}
          </button>
        );
      })}
    </div>
  );
}

function VarEditor({ r, envs, onSave, onCancel }: { r: Draft & { isNew: boolean }; envs: string[]; onSave: (d: Draft) => void; onCancel: () => void }) {
  const [k, setK] = useState(r.k);
  const [v, setV] = useState(r.secret ? "" : r.v);
  const [secret, setSecret] = useState(r.secret);
  const [sel, setSel] = useState(r.envs);
  return (
    <div style={{ display: "grid", gridTemplateColumns: COLS, gap: 12, alignItems: "center", padding: "10px 12px", borderBottom: hair, background: "var(--surface-raised)" }}>
      <input autoFocus aria-label="Clé" value={k} onChange={(e) => setK(e.target.value.toUpperCase().replace(/[^A-Z0-9_]/g, "_"))} placeholder="CLÉ" style={field} />
      <div style={{ display: "flex", gap: 6, alignItems: "center" }}>
        <input aria-label="Valeur" value={v} onChange={(e) => setV(e.target.value)} type={secret ? "password" : "text"} autoComplete="off" placeholder={r.secret && r.k ? "Laisser vide pour conserver" : "valeur"} style={{ ...field, flex: 1 }} />
        <button type="button" onClick={() => setSecret((s) => !s)} title="Secret" aria-label="Secret" aria-pressed={secret} style={{ width: 32, height: 32, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", borderRadius: "var(--radius-md)", border: "1px solid " + (secret ? "var(--gray-500)" : "var(--border-strong)"), background: secret ? "var(--surface-selected)" : "transparent", color: secret ? "var(--text-strong)" : "var(--text-subtle)", cursor: "pointer" }}>
          <Icon name={secret ? "Lock" : "LockOpen"} size={13} />
        </button>
      </div>
      <EnvChips envs={envs} value={sel} onChange={setSel} />
      <span></span>
      <div style={{ display: "flex", gap: 4, justifyContent: "flex-end" }}>
        <IconButton size={28} variant="ghost" icon="X" label="Annuler" onClick={onCancel} />
        <IconButton size={28} icon="Check" label="Valider" onClick={() => k && sel.length && onSave({ k, v, secret, envs: sel })} />
      </div>
    </div>
  );
}

function VarRow({ r, env, canEdit, onEdit, onDelete, reveal, setReveal, flash }: { r: ProjectVariable; env: string; canEdit: boolean; onEdit: () => void; onDelete: () => void; reveal: boolean; setReveal: (x: boolean) => void; flash: (m: string) => void }) {
  const [h, setH] = useState(false);
  const value = r.value ?? "";
  const shown = r.secret ? "••••••••••••" : reveal ? value : "•".repeat(Math.min(12, Math.max(6, value.length)));
  return (
    <div data-testid={`var-${r.key}`} onMouseEnter={() => setH(true)} onMouseLeave={() => setH(false)} style={{ display: "grid", gridTemplateColumns: COLS, gap: 12, alignItems: "center", minHeight: 52, padding: "0 12px", borderBottom: hair, background: h ? "var(--surface-raised)" : "transparent" }}>
      <div style={{ ...cell, display: "flex", alignItems: "center", gap: 8 }}>
        {r.secret ? <Icon name="Lock" size={12} color="var(--text-subtle)" /> : <span style={{ width: 12 }}></span>}
        <span style={{ ...cell, font: "500 12px/1 var(--font-mono)", color: "var(--text-strong)" }}>{r.key}</span>
        {r.pending_apply && <span title="Non appliquée" aria-label="Non appliquée" style={{ width: 6, height: 6, borderRadius: 99, background: "#e5b454", flex: "none" }}></span>}
      </div>
      <div style={{ ...cell, display: "flex", alignItems: "center", gap: 8 }}>
        <span style={{ ...cell, font: "400 12px/1 var(--font-mono)", color: r.secret ? "var(--text-subtle)" : "var(--text-body)" }}>{shown}</span>
        {!r.secret && h && (
          <button type="button" aria-label={reveal ? "Masquer" : "Afficher"} onClick={() => setReveal(!reveal)} style={{ background: "none", border: 0, padding: 0, cursor: "pointer", color: "var(--text-subtle)", display: "flex", flex: "none" }}>
            <Icon name={reveal ? "EyeOff" : "Eye"} size={13} />
          </button>
        )}
      </div>
      <EnvChips envs={[env]} value={[env]} small />
      <div style={{ display: "flex", flexDirection: "column", gap: 5, ...cell }}>
        <span style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-body)" }}>{timeAgo(r.updated_at)}</span>
        <span style={{ ...lbl, fontSize: 11 }}>{r.updated_by ?? "—"}</span>
      </div>
      <div style={{ display: "flex", gap: 2, justifyContent: "flex-end", opacity: h ? 1 : 0, transition: "opacity var(--dur-fast)" }}>
        {!r.secret && (
          <IconButton
            size={28}
            variant="ghost"
            icon="Copy"
            label="Copier"
            onClick={() => {
              void copyText(r.key + "=" + value).then((ok) => flash(ok ? "Copié" : "Copie impossible"));
            }}
          />
        )}
        {canEdit && <IconButton size={28} variant="ghost" icon="Pencil" label="Modifier" onClick={onEdit} />}
        {canEdit && <IconButton size={28} variant="ghost" icon="Trash" label="Supprimer" onClick={onDelete} />}
      </div>
    </div>
  );
}

export function ProjectVariables({ project }: { project: Project }) {
  const envs = project.environments.map((e) => e.name);
  const [env, setEnv] = useState(envs[0] ?? "production");
  const client = useQueryClient();
  const { flash } = useToast();
  const role = useRole();
  const canEdit = role === "owner" || role === "admin" || role === "developer";
  const actions = useDeploymentActions(project.id);
  const query = useQuery({ queryKey: projectVariablesQueryKey(project.id, env), queryFn: () => fetchProjectVariables(project.id, env) });
  const status = useQuery({ queryKey: variablesStatusQueryKey(project.id, env), queryFn: () => fetchVariablesStatus(project.id, env) });
  const [q, setQ] = useState("");
  const [edit, setEdit] = useState<string | null>(null);
  const [reveal, setReveal] = useState<Record<string, boolean>>({});
  const [raw, setRaw] = useState(false);
  const [rawTxt, setRawTxt] = useState("");
  const [openG, setOpenG] = useState(true);
  // État au moment où le bandeau a été masqué : un autre état le fait revenir.
  const [dismissedAt, setDismissedAt] = useState<string | null>(null);
  const busy = useRef(false);

  const rows = query.data ?? [];
  const own = rows.filter((r) => !r.inherited);
  const globals = rows.filter((r) => r.inherited);
  const list = own.filter((r) => r.key.toLowerCase().includes(q.toLowerCase()));
  const pending = rows.filter((r) => r.pending_apply).length;
  // Suppressions comprises : aucune ligne ne les porte.
  const unapplied = pending > 0 || status.data?.pending_apply === true;
  const bannerState = `${pending}:${status.data?.pending_apply === true}`;

  const store = (next: ProjectVariable[]) => {
    client.setQueryData(projectVariablesQueryKey(project.id, env), next);
    void client.invalidateQueries({ queryKey: variablesStatusQueryKey(project.id, env) });
  };
  const guarded = async (fn: () => Promise<void>) => {
    if (busy.current) return;
    busy.current = true;
    try {
      await fn();
    } finally {
      busy.current = false;
    }
  };

  const save = (old: ProjectVariable | null, d: Draft) =>
    guarded(async () => {
      let short = false;
      for (const target of d.envs) {
        const result = await putProjectVariable(project.id, target, { key: d.k, value: d.v, secret: d.secret, previous_key: old && target === env && old.key !== d.k ? old.key : null });
        if (!result.ok) {
          flash(result.message, 5000);
          return;
        }
        short ||= shortSecretKeys(result.list).includes(d.k);
        if (target === env) store(result.list);
        else void client.invalidateQueries({ queryKey: projectVariablesQueryKey(project.id, target) });
      }
      setEdit(null);
      const notice = shortSecretNotice(short ? [d.k] : []);
      if (notice) flash(notice, 6000);
    });
  const del = (r: ProjectVariable) =>
    guarded(async () => {
      const result = await deleteProjectVariable(project.id, env, r.key);
      if (!result.ok) {
        flash(result.message, 5000);
        return;
      }
      await client.invalidateQueries({ queryKey: projectVariablesQueryKey(project.id, env) });
      flash("Variable supprimée");
    });
  const openRaw = () => {
    setRawTxt(own.map((r) => (r.secret ? `${r.key}=  # secret` : dotenvLine(r.key, r.value ?? ""))).join("\n") + "\n");
    setRaw(true);
  };
  const applyRaw = () =>
    guarded(async () => {
      const result = await bulkProjectVariables(project.id, env, rawTxt);
      if (!result.ok) {
        flash(result.message, 5000);
        return;
      }
      store(result.list);
      setRaw(false);
      const saved = result.list.filter((r) => !r.inherited).length + " variables enregistrées";
      const notice = shortSecretNotice(shortSecretKeys(result.list));
      flash(notice ? `${saved}. ${notice}` : saved, notice ? 6000 : undefined);
    });
  const downloadEnv = () => {
    // Sans les secrets : leur valeur n'est jamais relue.
    download(`${project.name}.${env}.env`, own.filter((r) => !r.secret).map((r) => dotenvLine(r.key, r.value ?? "")).join("\n") + "\n");
    flash(".env téléchargé");
  };

  if (query.isError) {
    const forbidden = query.error instanceof ApiError && query.error.status === 403;
    return <div style={{ padding: "28px 12px", font: "400 13px/1.5 var(--font-sans)", color: "var(--text-muted)" }}>{forbidden ? "Accès refusé : votre rôle ne permet pas de voir les variables." : "Variables indisponibles pour le moment."}</div>;
  }
  if (query.isPending) return null;
  const E = envLabel(env);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, paddingLeft: 12 }}>
        <Tabs
          tabs={envs.map((e) => ({ value: e, label: envLabel(e), count: e === env ? own.length : undefined }))}
          value={env}
          onChange={(e) => {
            setEnv(e);
            setEdit(null);
            setRaw(false);
          }}
        />
        <span style={{ flex: 1 }}></span>
        <div style={{ display: "flex", alignItems: "center", gap: 8, height: 32, padding: "0 10px", width: 220, borderRadius: "var(--radius-md)", border: "1px solid var(--border-strong)", background: "var(--surface-control)" }}>
          <Icon name="Search" size={14} color="var(--text-subtle)" />
          <input aria-label="Rechercher une clé" value={q} onChange={(e) => setQ(e.target.value)} placeholder="Rechercher une clé" style={{ flex: 1, minWidth: 0, background: "transparent", border: 0, outline: "none", color: "var(--text-strong)", font: "400 12px/1 var(--font-mono)" }} />
        </div>
        <div style={{ display: "flex", gap: 2, padding: 2, borderRadius: 9, border: "1px solid var(--border-default)", background: "var(--surface-control)" }}>
          {(
            [
              ["table", "List", "Tableau"],
              ["raw", "FileCode", ".env"],
            ] as const
          ).map(([k, ic, t]) => {
            const on = (k === "raw") === raw;
            const disabled = k === "raw" && !canEdit;
            return (
              <button key={k} type="button" aria-pressed={on} disabled={disabled} title={disabled ? "Réservé aux rôles développeur et plus" : undefined} onClick={() => (k === "raw" ? openRaw() : setRaw(false))} style={{ display: "flex", alignItems: "center", gap: 6, height: 28, padding: "0 10px", border: 0, borderRadius: 7, cursor: disabled ? "not-allowed" : "pointer", background: on ? "var(--surface-selected)" : "transparent", color: on ? "var(--text-strong)" : "var(--text-muted)", font: "500 12px/1 var(--font-sans)" }}>
                <Icon name={ic} size={13} />
                {t}
              </button>
            );
          })}
        </div>
        <IconButton icon="Download" label="Télécharger .env" onClick={downloadEnv} />
        <Button
          variant="inverse"
          icon="Plus"
          disabled={!canEdit}
          title={canEdit ? undefined : "Réservé aux rôles développeur et plus"}
          onClick={() => {
            setRaw(false);
            setEdit("new");
          }}
        >
          Ajouter
        </Button>
      </div>
      {unapplied && dismissedAt !== bannerState && (
        <div role="status" style={{ display: "flex", alignItems: "center", gap: 12, padding: "10px 12px 10px 14px", borderRadius: "var(--radius-lg)", border: "1px solid rgba(229,180,84,.25)", background: "rgba(229,180,84,.06)" }}>
          <Icon name="TriangleExclamationPoint" size={15} color="#e5b454" />
          <span style={{ flex: 1, font: "400 12px/1.4 var(--font-sans)", color: "var(--text-body)" }}>
            <span style={{ color: "var(--text-strong)", fontWeight: 500 }}>
              {pending > 0 ? `${pending} modification${pending > 1 ? "s" : ""} non appliquée${pending > 1 ? "s" : ""}.` : "Suppressions non appliquées."}
            </span>{" "}
            Les variables sont injectées au prochain déploiement.
          </span>
          <Button size="sm" variant="ghost" onClick={() => setDismissedAt(bannerState)}>
            Annuler
          </Button>
          <Button size="sm" variant="inverse" icon="Rocket" disabled={!actions.canRedeploy || actions.busy} title={actions.canDeploy ? actions.redeployHint : "Réservé aux rôles développeur et plus"} onClick={() => void actions.redeploy()}>
            Enregistrer et redéployer
          </Button>
        </div>
      )}
      {raw ? (
        <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
          <div style={{ borderRadius: "var(--radius-lg)", border: "1px solid var(--border-default)", overflow: "hidden" }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "10px 14px", borderBottom: hair, ...lbl }}>
              <Icon name="FileCode" size={13} />
              {project.name}.{env}.env<span style={{ flex: 1 }}></span>
              <span>Les secrets vides conservent leur valeur actuelle</span>
            </div>
            <textarea aria-label="Contenu .env" value={rawTxt} onChange={(e) => setRawTxt(e.target.value)} spellCheck="false" className="log-scroll" style={{ display: "block", width: "100%", height: 300, resize: "vertical", padding: "14px 16px", border: 0, background: "var(--gray-0)", color: "var(--text-body)", font: "400 12px/1.8 var(--font-mono)", outline: "none" }}></textarea>
          </div>
          <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
            <Button onClick={() => setRaw(false)}>Annuler</Button>
            <Button variant="filled" icon="Check" onClick={() => void applyRaw()}>
              Enregistrer
            </Button>
          </div>
        </div>
      ) : (
        <div style={{ borderTop: hair }}>
          <div style={{ display: "grid", gridTemplateColumns: COLS, gap: 12, padding: "12px 12px", borderBottom: hair, ...lbl }}>
            <span style={{ paddingLeft: 20 }}>Clé</span>
            <span>Valeur</span>
            <span>Environnements</span>
            <span>Modifiée</span>
            <span></span>
          </div>
          {edit === "new" && <VarEditor r={{ k: "", v: "", secret: false, envs: [env], isNew: true }} envs={envs} onSave={(d) => void save(null, d)} onCancel={() => setEdit(null)} />}
          {list.map((r) =>
            edit === r.key ? (
              <VarEditor key={r.key} r={{ k: r.key, v: r.value ?? "", secret: r.secret, envs: [env], isNew: false }} envs={envs} onSave={(d) => void save(r, d)} onCancel={() => setEdit(null)} />
            ) : (
              <VarRow key={r.key} r={r} env={env} canEdit={canEdit} onEdit={() => setEdit(r.key)} onDelete={() => void del(r)} reveal={!!reveal[r.key]} setReveal={(x) => setReveal((s) => ({ ...s, [r.key]: x }))} flash={flash} />
            ),
          )}
          {!list.length && edit !== "new" && <div style={{ padding: "28px 12px", ...lbl }}>Aucune variable pour {E}.</div>}
        </div>
      )}
      <div style={{ marginTop: 12, borderRadius: "var(--radius-lg)", border: "1px solid var(--border-default)", overflow: "hidden" }}>
        <button type="button" aria-expanded={openG} onClick={() => setOpenG((o) => !o)} style={{ display: "flex", alignItems: "center", gap: 10, width: "100%", padding: "14px 16px", background: "transparent", border: 0, cursor: "pointer", textAlign: "left" }}>
          <Icon name={openG ? "ChevronDown" : "ChevronRight"} size={14} color="var(--text-muted)" />
          <Icon name="Braces" size={14} color="var(--text-muted)" />
          <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>Héritées des variables globales</span>
          <span style={{ ...lbl, fontFamily: "var(--font-mono)" }}>{globals.length}</span>
          <span style={{ flex: 1 }}></span>
          <span style={lbl}>Plateforme › Variables globales</span>
        </button>
        {openG && (
          <div style={{ borderTop: hair }}>
            {globals.map((g, i) => (
              <div key={g.key} data-testid={`global-${g.key}`} style={{ display: "grid", gridTemplateColumns: "minmax(0,1.2fr) minmax(0,1.6fr) minmax(0,1fr)", gap: 12, alignItems: "center", minHeight: 44, padding: "0 16px 0 40px", borderTop: i ? hair : "none" }}>
                <span style={{ ...cell, display: "flex", alignItems: "center", gap: 8, font: "500 12px/1 var(--font-mono)", color: "var(--text-body)" }}>
                  {g.secret && <Icon name="Lock" size={11} color="var(--text-subtle)" />}
                  {g.key}
                </span>
                <span style={{ ...cell, font: "400 12px/1 var(--font-mono)", color: "var(--text-subtle)" }}>{g.secret ? "••••••••••••" : g.value}</span>
                <span style={{ justifySelf: "end" }}>
                  <Badge>Globale</Badge>
                </span>
              </div>
            ))}
            {!globals.length && <div style={{ padding: "16px 40px", ...lbl }}>Aucune variable globale pour {E}.</div>}
          </div>
        )}
      </div>
    </div>
  );
}
