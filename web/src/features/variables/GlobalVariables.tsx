import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Fragment, useRef, useState, type CSSProperties, type ReactNode } from "react";

import { Button, Icon, IconButton, Modal, Switch, TextField, useToast, type IconName } from "@/design-system";
import { fetchMe, ME_QUERY_KEY, MFA_PENDING } from "@/features/auth/api";
import { fetchProjects, PROJECTS_QUERY_KEY, type ProjectSummary } from "@/features/projects/api";
import { timeAgo } from "@/features/projects/format";
import { copyText } from "@/lib/clipboard";

import { createGlobalVariable, deleteGlobalVariable, download, ENVIRONMENTS, envLabel, exportGlobalVariables, fetchGlobalVariables, GLOBAL_VARIABLES_QUERY_KEY, parseDotenv, shortSecretKeys, shortSecretNotice, updateGlobalVariable, type GlobalVariable } from "./api";

// Port de prototypes/dashboard/GlobalVariables.jsx (structure, libellés, tokens).
// Écarts imposés par le backend v0.1 :
// - réservé aux administrateurs (matrice fixe : Variables › Admin) ;
// - un secret n'est jamais relu (« Écriture seule », « Laisser vide pour
//   conserver ») ; l'export (serveur) ne contient ni les secrets ni leurs clés ;
// - les projets viennent de l'API ; l'import crée une variable par ligne
//   (tous environnements ; tous projets, ou la liste actuelle des projets
//   pour un secret) ;
// - un secret ne vise jamais « tous les projets » (le serveur le refuse) ;
//   l'écran Projet propose « Appliquer à ce projet ».

const lbl: CSSProperties = { font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" };
const ENVS: string[] = [...ENVIRONMENTS];
const COLS = "minmax(0,1.4fr) minmax(0,1.5fr) minmax(0,1.3fr) 110px 110px 96px";
const IMPORT_SAMPLE = "# Collez le contenu d'un fichier .env\nAPI_URL=https://api.ciphera.dev\nFEATURE_FLAGS=charts,logs\n";

type Saved = { key: string; value: string; secret: boolean; environments: string[]; all_projects: boolean; projects: string[] };

function useIsAdmin(): boolean {
  // Compte chargé par le shell : jamais relu ici (voir useCanDeploy).
  const me = useQuery({ queryKey: ME_QUERY_KEY, queryFn: fetchMe, retryOnMount: false, refetchOnMount: false });
  return !!me.data && me.data !== MFA_PENDING && (me.data.role === "owner" || me.data.role === "admin");
}

function chip(on: boolean, txt: string, onClick: () => void, disabled = false) {
  return (
    <button key={txt} type="button" aria-pressed={on} disabled={disabled} onClick={onClick} style={{ display: "flex", alignItems: "center", gap: 6, height: 28, padding: "0 10px", borderRadius: 99, cursor: disabled ? "not-allowed" : "pointer", opacity: disabled ? 0.45 : 1, font: "500 12px/1 var(--font-sans)", border: "1px solid " + (on ? "var(--gray-500)" : "var(--border-strong)"), background: on ? "var(--surface-selected)" : "transparent", color: on ? "var(--text-strong)" : "var(--text-muted)" }}>
      {on && <Icon name="Check" size={11} />}
      {txt}
    </button>
  );
}

function WideButton({ icon, label, onClick, solid }: { icon: IconName; label: string; onClick: () => void; solid?: boolean }) {
  return (
    <button type="button" onClick={onClick} style={{ display: "flex", alignItems: "center", gap: 8, height: 38, padding: "0 15px", borderRadius: 10, border: solid ? 0 : "1px solid var(--border-strong)", cursor: "pointer", background: solid ? "#8b7cf6" : "transparent", color: solid ? "#fff" : "var(--text-strong)", font: "600 13px/1 var(--font-sans)" }}>
      <Icon name={icon} size={14} color={solid ? "#fff" : "var(--text-strong)"} />
      {label}
    </button>
  );
}

function VarModal({ initial, projects, onClose, onSave }: { initial?: GlobalVariable; projects: ProjectSummary[]; onClose: () => void; onSave: (d: Saved) => void }) {
  const [k, setK] = useState(initial?.key ?? "");
  const [v, setV] = useState(initial && !initial.secret ? (initial.value ?? "") : "");
  const [secret, setSecret] = useState(initial?.secret ?? false);
  const [envs, setEnvs] = useState<string[]>(initial?.environments ?? [...ENVS]);
  // Un secret nomme ses projets : « Tous les projets » est réservé aux publiques.
  const [all, setAll] = useState((!initial || initial.all_projects) && !initial?.secret);
  const [proj, setProj] = useState<string[]>(initial?.projects ?? []);
  const tog = (arr: string[], set: (x: string[]) => void, x: string) => set(arr.includes(x) ? arr.filter((y) => y !== x) : [...arr, x]);
  const ok = !!k && (!!v || !!initial?.secret) && envs.length > 0 && (all || proj.length > 0);
  return (
    <Modal
      title={initial ? "Modifier la variable" : "Nouvelle variable globale"}
      description="Injectée au prochain déploiement des projets concernés. Une variable de projet du même nom a priorité."
      onClose={onClose}
      width={560}
      footer={
        <>
          <Button onClick={onClose}>Annuler</Button>
          <Button variant="inverse" disabled={!ok} onClick={() => onSave({ key: k.toUpperCase().replace(/[^A-Z0-9_]/g, "_"), value: v, secret, environments: envs, all_projects: all, projects: all ? [] : proj })}>
            Enregistrer
          </Button>
        </>
      }
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 18 }}>
        <div style={{ display: "flex", gap: 12 }}>
          <TextField label="Clé" mono placeholder="API_URL" value={k} onChange={setK} style={{ flex: 1 }} />
          <TextField label="Valeur" mono type={secret ? "password" : "text"} autoComplete="off" placeholder={initial?.secret ? "Laisser vide pour conserver" : "valeur"} value={v} onChange={setV} style={{ flex: 1.4 }} />
        </div>
        <Switch
          checked={secret}
          onChange={(on) => {
            setSecret(on);
            if (on) setAll(false);
          }}
          label="Secret"
          description="Chiffré (age) et en écriture seule : la valeur ne sera plus jamais affichée."
        />
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <span style={lbl}>Environnements</span>
          <div style={{ display: "flex", gap: 6 }}>{ENVS.map((e) => chip(envs.includes(e), envLabel(e), () => tog(envs, setEnvs, e)))}</div>
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <span style={lbl}>Projets</span>
          <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
            {chip(all, "Tous les projets", () => setAll(true), secret)}
            {chip(!all, "Sélection", () => setAll(false))}
          </div>
          {secret && <span style={lbl}>Un secret nomme ses projets : il n'est jamais envoyé à tous les projets, y compris ceux de demain.</span>}
          {!all && <div style={{ display: "flex", gap: 6, flexWrap: "wrap", paddingTop: 4 }}>{projects.map((p) => chip(proj.includes(p.id), p.name, () => tog(proj, setProj, p.id)))}</div>}
        </div>
      </div>
    </Modal>
  );
}

function ImportModal({ noProjects, onClose, onImport }: { noProjects: boolean; onClose: () => void; onImport: (rows: [string, string][], secret: boolean) => void }) {
  const [txt, setTxt] = useState(IMPORT_SAMPLE);
  const [secret, setSecret] = useState(false);
  const rows = parseDotenv(txt);
  return (
    <Modal
      title="Importer un fichier .env"
      description={rows.length + " variable(s) détectée(s), appliquées à tous les environnements (secrets : aux projets actuels)."}
      onClose={onClose}
      width={560}
      footer={
        <>
          <Button onClick={onClose}>Annuler</Button>
          <Button variant="inverse" disabled={!rows.length || (secret && noProjects)} onClick={() => onImport(rows, secret)}>
            Importer {rows.length}
          </Button>
        </>
      }
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
        <textarea aria-label="Contenu .env" value={txt} onChange={(e) => setTxt(e.target.value)} spellCheck="false" style={{ height: 180, resize: "vertical", padding: 12, borderRadius: "var(--radius-lg)", border: "1px solid var(--border-strong)", background: "var(--gray-0)", color: "var(--text-body)", font: "400 12px/1.6 var(--font-mono)", outline: "none" }}></textarea>
        <Switch checked={secret} onChange={setSecret} label="Importer comme secrets" description={secret && noProjects ? "Un secret nomme ses projets : créez d'abord un projet." : "Toutes les valeurs seront chiffrées et masquées."} />
      </div>
    </Modal>
  );
}

function GRow({ r, i, rev, toggle, edit, copy, del, used }: { r: GlobalVariable; i: number; rev: boolean; toggle: () => void; edit: () => void; copy: () => void; del: () => void; used: number }) {
  const [h, setH] = useState(false);
  return (
    <div data-testid={`gvar-${r.key}`} onMouseEnter={() => setH(true)} onMouseLeave={() => setH(false)} style={{ display: "grid", gridTemplateColumns: COLS, gap: 16, alignItems: "center", minHeight: 54, padding: "0 12px 0 18px", borderTop: i ? "1px solid rgba(255,255,255,.06)" : "none", background: h ? "rgba(255,255,255,.02)" : "transparent" }}>
      <span style={{ display: "flex", alignItems: "center", gap: 10, minWidth: 0 }}>
        <span style={{ width: 26, height: 26, borderRadius: 7, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", background: "var(--surface-selected)", border: "1px solid var(--border-subtle)" }}>
          <Icon name={r.secret ? "Lock" : "Braces"} size={12} color="var(--text-body)" />
        </span>
        <span style={{ font: "500 13px/1 var(--font-mono)", color: "var(--text-strong)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{r.key}</span>
      </span>
      {r.secret ? (
        <span style={{ display: "flex", alignItems: "center", gap: 6, font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" }}>
          <Icon name="EyeOff" size={12} />
          Écriture seule
        </span>
      ) : (
        <span style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
          <span style={{ flex: 1, minWidth: 0, height: 28, display: "flex", alignItems: "center", padding: "0 10px", borderRadius: 8, background: "rgba(255,255,255,.035)", font: "400 12px/1 var(--font-mono)", color: "var(--text-body)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{rev ? r.value : "••••••••••••"}</span>
          <button type="button" aria-label={rev ? "Masquer" : "Afficher"} onClick={toggle} style={{ background: "none", border: 0, padding: 0, cursor: "pointer", color: "var(--text-subtle)", display: "flex" }}>
            <Icon name={rev ? "EyeOff" : "Eye"} size={14} />
          </button>
        </span>
      )}
      <span style={{ display: "inline-flex", width: "fit-content", padding: 2, gap: 2, borderRadius: 8, background: "var(--surface-raised)", border: "1px solid var(--border-subtle)" }}>
        {ENVS.map((e) => {
          const on = r.environments.includes(e);
          return (
            <span key={e} title={envLabel(e)} style={{ height: 20, display: "flex", alignItems: "center", padding: "0 7px", borderRadius: 6, background: on ? "var(--surface-selected)" : "transparent", font: "500 11px/1 var(--font-sans)", color: on ? "var(--text-strong)" : "var(--text-faint)" }}>
              {e === "production" ? "Prod" : envLabel(e)}
            </span>
          );
        })}
      </span>
      <span style={{ display: "flex", alignItems: "center", gap: 6, font: "400 12px/1 var(--font-sans)", color: "var(--text-body)" }}>
        <Icon name="Layers" size={12} color="var(--text-subtle)" />
        {r.all_projects ? "Tous" : used}
      </span>
      <span style={{ display: "flex", flexDirection: "column", gap: 5 }}>
        <span style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-body)" }}>{timeAgo(r.updated_at)}</span>
        <span style={{ ...lbl, fontSize: 11 }}>{(r.updated_by ?? "—").split(" ")[0]}</span>
      </span>
      <div style={{ display: "flex", gap: 2, justifyContent: "flex-end", opacity: h ? 1 : 0.5, transition: "opacity .15s" }}>
        <IconButton size={28} variant="ghost" icon="Pencil" label="Modifier" onClick={edit} />
        {!r.secret && <IconButton size={28} variant="ghost" icon="Copy" label="Copier" onClick={copy} />}
        <IconButton size={28} variant="ghost" icon="Trash" label="Supprimer" onClick={del} />
      </div>
    </div>
  );
}

type ModalState = { type: "edit"; row?: GlobalVariable } | { type: "import" } | null;

export function GlobalVariablesPage() {
  const isAdmin = useIsAdmin();
  if (!isAdmin) return <div style={{ padding: "20px 32px 40px 34px", font: "400 13px/1.5 var(--font-sans)", color: "var(--text-muted)" }}>Réservé aux administrateurs.</div>;
  return <GlobalVariables />;
}

function GlobalVariables() {
  const client = useQueryClient();
  const { flash } = useToast();
  const query = useQuery({ queryKey: GLOBAL_VARIABLES_QUERY_KEY, queryFn: fetchGlobalVariables });
  const projectsQuery = useQuery({ queryKey: PROJECTS_QUERY_KEY, queryFn: fetchProjects });
  const [env, setEnv] = useState("Tous");
  const [q, setQ] = useState("");
  const [reveal, setReveal] = useState<Record<string, boolean>>({});
  const [modal, setModal] = useState<ModalState>(null);
  const busy = useRef(false);
  const rows = query.data ?? [];
  const projects = projectsQuery.data ?? [];
  const list = rows.filter((r) => (env === "Tous" || r.environments.includes(env)) && r.key.toLowerCase().includes(q.toLowerCase()));
  const close = () => setModal(null);
  const refresh = () => client.invalidateQueries({ queryKey: GLOBAL_VARIABLES_QUERY_KEY });
  const guarded = async (fn: () => Promise<void>) => {
    if (busy.current) return;
    busy.current = true;
    try {
      await fn();
    } finally {
      busy.current = false;
    }
  };

  const save = (row: GlobalVariable | undefined, d: Saved) =>
    guarded(async () => {
      const result = row ? await updateGlobalVariable(row.id, { ...d, value: d.value || null }) : await createGlobalVariable({ ...d, value: d.value });
      if (!result.ok) {
        flash(result.message, 5000);
        return;
      }
      await refresh();
      const notice = shortSecretNotice(shortSecretKeys([result.variable]));
      flash(notice ?? (row ? "Variable mise à jour" : "Variable ajoutée"), notice ? 6000 : undefined);
      close();
    });
  const importRows = (pairs: [string, string][], secret: boolean) =>
    guarded(async () => {
      let done = 0;
      const short: string[] = [];
      for (const [key, value] of pairs) {
        // Un secret nomme ses projets : la liste actuelle, jamais « tous ».
        const result = await createGlobalVariable({ key, value, secret, environments: [...ENVS], all_projects: !secret, projects: secret ? projects.map((p) => p.id) : [] });
        if (!result.ok) {
          flash(`${key} : ${result.message}`, 5000);
          break;
        }
        done += 1;
        short.push(...shortSecretKeys([result.variable]));
      }
      await refresh();
      const notice = shortSecretNotice(short);
      if (done) flash(notice ? `${done} variables importées. ${notice}` : done + " variables importées", notice ? 6000 : undefined);
      close();
    });
  const exportEnv = () =>
    guarded(async () => {
      const name = env === "Tous" ? undefined : env;
      const result = await exportGlobalVariables(name);
      if (!result.ok) {
        flash(result.message, 5000);
        return;
      }
      download("ciphera-globales" + (name ? "-" + name : "") + ".env", result.text);
      flash(result.text.split("\n").filter(Boolean).length + " variables exportées");
    });
  const del = (r: GlobalVariable) =>
    guarded(async () => {
      const result = await deleteGlobalVariable(r.id);
      if (!result.ok) {
        flash(result.message, 5000);
        return;
      }
      await refresh();
      flash(r.key + " supprimée");
    });

  const covered = rows.some((r) => r.all_projects) ? projects.length : new Set(rows.flatMap((r) => r.projects)).size;
  const latest = rows.map((r) => r.updated_at).sort().at(-1);
  const box: CSSProperties = { borderRadius: "var(--radius-xl)", border: "1px solid var(--border-default)", background: "var(--surface-app)" };
  const line = "1px solid rgba(255,255,255,.06)";
  const stats: [string, ReactNode, IconName][] = [
    ["Variables", rows.length, "Braces"],
    ["Secrets", rows.filter((r) => r.secret).length, "Lock"],
    ["Projets concernés", covered + " / " + projects.length, "Layers"],
    ["Dernière modification", latest ? timeAgo(latest) : "—", "Clock"],
  ];

  return (
    <div style={{ padding: "20px 32px 40px 34px", display: "flex", flexDirection: "column", gap: 16 }}>
      <div style={{ borderRadius: 20, overflow: "hidden", border: "1px solid var(--border-default)", background: "var(--surface-app)" }}>
        <div style={{ position: "relative", display: "flex", alignItems: "center", gap: 18, padding: "20px 24px", background: "transparent", overflow: "hidden" }}>
          <span style={{ position: "relative", width: 50, height: 50, borderRadius: 14, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", background: "linear-gradient(145deg,#4a4a52,#26262c)", boxShadow: "inset 0 1px 0 rgba(255,255,255,.18)" }}>
            <Icon name="Braces" size={22} color="#f2f2f2" />
          </span>
          <span style={{ position: "relative", display: "flex", flexDirection: "column", gap: 8, flex: 1, minWidth: 0 }}>
            <span style={{ font: "700 18px/1.1 var(--font-sans)", color: "var(--text-strong)" }}>Variables globales</span>
            <span style={{ font: "400 13px/1.4 var(--font-sans)", color: "var(--text-muted)" }}>Partagées par tous vos projets · chiffrées au repos · injectées au prochain déploiement</span>
          </span>
          <span style={{ position: "relative", display: "flex", gap: 8 }}>
            <WideButton icon="Upload" label="Importer .env" onClick={() => setModal({ type: "import" })} />
            <WideButton icon="Download" label="Exporter" onClick={() => void exportEnv()} />
            <WideButton icon="Plus" label="Nouvelle variable" onClick={() => setModal({ type: "edit" })} solid />
          </span>
        </div>
        <div style={{ display: "grid", gridTemplateColumns: "repeat(4,minmax(0,1fr))" }}>
          {stats.map(([k, v, ic], i) => (
            <div key={k} style={{ display: "flex", flexDirection: "column", gap: 9, padding: "16px 20px", borderLeft: i ? line : "none" }}>
              <span style={{ display: "flex", alignItems: "center", gap: 6, ...lbl, whiteSpace: "nowrap" }}>
                <Icon name={ic} size={12} />
                {k}
              </span>
              <span style={{ font: "600 20px/1 var(--font-sans)", color: "var(--text-strong)", whiteSpace: "nowrap" }}>{v}</span>
            </div>
          ))}
        </div>
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "12px 16px", borderRadius: 12, background: "var(--surface-raised)" }}>
        <Icon name="GitMerge" size={15} color="var(--text-muted)" />
        <span style={{ font: "400 12px/1.5 var(--font-sans)", color: "var(--text-muted)" }}>Priorité au déploiement :</span>
        {["Variable du projet", "Globale de l'environnement", "Globale tous environnements"].map((t, i) => (
          <Fragment key={t}>
            {i > 0 && <Icon name="ChevronRight" size={12} color="var(--text-faint)" />}
            <span style={{ height: 24, display: "flex", alignItems: "center", padding: "0 9px", borderRadius: 99, background: "rgba(255,255,255,.05)", border: "1px solid rgba(255,255,255,.08)", font: "500 11px/1 var(--font-sans)", color: "var(--text-strong)" }}>
              {i + 1}. {t}
            </span>
          </Fragment>
        ))}
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 22, borderBottom: "1px solid rgba(255,255,255,.08)" }}>
        {["Tous", ...ENVS].map((e) => {
          const on = env === e;
          const n = e === "Tous" ? rows.length : rows.filter((r) => r.environments.includes(e)).length;
          return (
            <button key={e} type="button" aria-pressed={on} onClick={() => setEnv(e)} style={{ display: "flex", alignItems: "center", gap: 7, height: 38, padding: 0, border: 0, borderBottom: "2px solid " + (on ? "#ededed" : "transparent"), marginBottom: -1, background: "transparent", cursor: "pointer", font: "500 13px/1 var(--font-sans)", color: on ? "var(--text-strong)" : "var(--text-muted)" }}>
              {e === "Tous" ? e : envLabel(e)}
              <span style={{ fontWeight: 400, color: "var(--text-subtle)" }}>{n}</span>
            </button>
          );
        })}
        <span style={{ flex: 1 }}></span>
        <div style={{ display: "flex", alignItems: "center", gap: 8, height: 34, width: 260, padding: "0 12px", marginBottom: 4, borderRadius: 10, border: "1px solid rgba(255,255,255,.1)" }}>
          <Icon name="Search" size={14} color="var(--text-subtle)" />
          <input aria-label="Rechercher une clé" value={q} onChange={(e) => setQ(e.target.value)} placeholder="Rechercher une clé…" style={{ flex: 1, minWidth: 0, background: "transparent", border: 0, outline: "none", color: "var(--text-strong)", font: "400 13px/1 var(--font-mono)" }} />
        </div>
      </div>
      <div style={{ ...box, overflow: "hidden" }}>
        <div style={{ display: "grid", gridTemplateColumns: COLS, gap: 16, padding: "12px 18px", borderBottom: line, font: "500 12px/1 var(--font-sans)", color: "var(--text-strong)" }}>
          <span>Clé</span>
          <span>Valeur</span>
          <span>Environnements</span>
          <span>Projets</span>
          <span>Modifiée</span>
          <span></span>
        </div>
        {list.map((r, i) => (
          <GRow
            key={r.id}
            r={r}
            i={i}
            rev={!!reveal[r.id]}
            toggle={() => setReveal((x) => ({ ...x, [r.id]: !x[r.id] }))}
            edit={() => setModal({ type: "edit", row: r })}
            copy={() => {
              void copyText(r.key + "=" + (r.value ?? "")).then((ok) => flash(ok ? "Copié" : "Copie impossible"));
            }}
            del={() => void del(r)}
            used={r.all_projects ? projects.length : r.projects.length}
          />
        ))}
        {!list.length && (
          <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 8, padding: "40px 0" }}>
            <Icon name="Braces" size={18} color="var(--text-subtle)" />
            <span style={lbl}>{query.isError ? "Variables indisponibles pour le moment." : "Aucune variable."}</span>
          </div>
        )}
      </div>
      {modal?.type === "edit" && <VarModal initial={modal.row} projects={projects} onClose={close} onSave={(d) => void save(modal.row, d)} />}
      {modal?.type === "import" && <ImportModal noProjects={projects.length === 0} onClose={close} onImport={(r, s) => void importRows(r, s)} />}
    </div>
  );
}
