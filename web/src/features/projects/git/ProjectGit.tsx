import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useRef, useState, type CSSProperties, type ReactNode } from "react";

import githubMark from "@/assets/images/connectors/github-mark.png";
import { Button, CopyButton, Icon, IconButton, StatusDot, Switch, useToast } from "@/design-system";
import { fetchMe, ME_QUERY_KEY, MFA_PENDING } from "@/features/auth/api";
import { fetchGithubStatus, GITHUB_STATUS_QUERY_KEY } from "@/features/connectors/api";
import { deploymentsQueryKey, fetchDeployments } from "@/features/deployments/api";
import { shortSha } from "@/features/deployments/format";
import { ApiError } from "@/lib/api/errors";

import type { Project } from "../api";
import { timeAgo } from "../format";
import { createDeployHook, deleteDeployHook, fetchDeployHooks, fetchGitSettings, gitQueryKey, hooksQueryKey, panelUrl, saveGitSettings, type CreatedDeployHook, type GitSettings, type UpdateGitSettings } from "./api";
import { TokenCreatedModal } from "./TokenCreatedModal";

// Port de prototypes/dashboard/ProjectGit.jsx (structure, libellés, tokens).
// Écarts imposés par le backend v0.1 :
// - branche de production en saisie libre (aucune liste des branches du dépôt) ;
// - « Changer » / « Déconnecter » le dépôt, branches et previews de PR,
//   commentaires de PR : portés mais inactifs (v0.3) ;
// - secret de signature géré par la GitHub App, jamais réaffiché (masqué,
//   ni révélation ni régénération) ; livraisons non historisées ;
// - un deploy hook n'affiche son URL secrète qu'une fois (TokenCreatedModal),
//   la liste montre l'URL sans le secret.

const hair = "1px solid var(--border-subtle)";
const lbl: CSSProperties = { font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" };
const mono = (v: ReactNode, c?: string) => <span style={{ fontFamily: "var(--font-mono)", fontSize: 12, color: c }}>{v}</span>;
const field: CSSProperties = { height: 32, padding: "0 10px", borderRadius: "var(--radius-md)", border: "1px solid var(--border-strong)", background: "var(--surface-control)", color: "var(--text-strong)", font: "400 12px/1 var(--font-mono)", outline: "none", minWidth: 0, boxShadow: "var(--shadow-control)" };
const SOON = "Bientôt disponible";

function Section({ title, desc, children, last }: { title: string; desc: string; children: ReactNode; last?: boolean }) {
  return (
    <section aria-label={title} style={{ display: "grid", gridTemplateColumns: "260px minmax(0,640px)", gap: 40, padding: "28px 12px", borderBottom: last ? "none" : hair }}>
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <h2 style={{ margin: 0, font: "500 14px/1.2 var(--font-sans)", color: "var(--text-strong)" }}>{title}</h2>
        <span style={{ font: "400 12px/1.5 var(--font-sans)", color: "var(--text-subtle)", textWrap: "pretty" }}>{desc}</span>
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 16, minWidth: 0 }}>{children}</div>
    </section>
  );
}

function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <label style={{ display: "flex", flexDirection: "column", gap: 8, flex: 1, minWidth: 0 }}>
      <span style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-muted)" }}>{label}</span>
      {children}
      {hint && <span style={{ ...lbl, fontSize: 11, color: "var(--text-faint)" }}>{hint}</span>}
    </label>
  );
}

function Chips({ items, onChange, placeholder, label, disabled }: { items: string[]; onChange: (v: string[]) => void; placeholder: string; label: string; disabled?: boolean }) {
  const [v, setV] = useState("");
  return (
    <div style={{ ...field, height: "auto", minHeight: 32, display: "flex", flexWrap: "wrap", alignItems: "center", gap: 6, padding: "4px 6px", opacity: disabled ? 0.5 : 1 }}>
      {items.map((t) => (
        <span key={t} style={{ display: "flex", alignItems: "center", gap: 4, height: 22, padding: "0 4px 0 8px", borderRadius: 5, background: "var(--surface-selected)", font: "400 12px/1 var(--font-mono)", color: "var(--text-strong)" }}>
          {t}
          <button type="button" aria-label={`Retirer ${t}`} disabled={disabled} onClick={() => onChange(items.filter((x) => x !== t))} style={{ display: "flex", background: "none", border: 0, padding: 2, cursor: "pointer", color: "var(--text-subtle)" }}>
            <Icon name="X" size={11} />
          </button>
        </span>
      ))}
      <input
        aria-label={label}
        value={v}
        disabled={disabled}
        onChange={(e) => setV(e.target.value)}
        onKeyDown={(e) => {
          if ((e.key === "Enter" || e.key === ",") && v.trim()) {
            e.preventDefault();
            if (!items.includes(v.trim())) onChange([...items, v.trim()]);
            setV("");
          }
          if (e.key === "Backspace" && !v && items.length) onChange(items.slice(0, -1));
        }}
        placeholder={items.length ? "" : placeholder}
        style={{ flex: 1, minWidth: 80, height: 22, background: "transparent", border: 0, outline: "none", color: "var(--text-strong)", font: "400 12px/1 var(--font-mono)", padding: "0 4px" }}
      />
    </div>
  );
}

/** Rôle du compte chargé par le shell, sans nouvelle requête (voir useCanDeploy). */
function useRole(): string | null {
  const me = useQuery({ queryKey: ME_QUERY_KEY, queryFn: fetchMe, retryOnMount: false, refetchOnMount: false });
  return me.data && me.data !== MFA_PENDING ? me.data.role : null;
}

type Draft = { branch: string; root: string; watch: string[]; auto: boolean; skip: boolean; ignoreBots: boolean };

const draftOf = (s: GitSettings): Draft => ({ branch: s.branch, root: s.root_directory || "./", watch: s.watch_paths, auto: s.auto_deploy, skip: s.skip_ci, ignoreBots: s.ignore_bots });

function patchOf(from: Draft, to: Draft): UpdateGitSettings {
  const p: UpdateGitSettings = {};
  if (from.branch !== to.branch) p.branch = to.branch.trim();
  if (from.root !== to.root) p.root_directory = to.root.trim();
  if (JSON.stringify(from.watch) !== JSON.stringify(to.watch)) p.watch_paths = to.watch;
  if (from.auto !== to.auto) p.auto_deploy = to.auto;
  if (from.skip !== to.skip) p.skip_ci = to.skip;
  if (from.ignoreBots !== to.ignoreBots) p.ignore_bots = to.ignoreBots;
  return p;
}

export function ProjectGit({ project }: { project: Project }) {
  const settings = useQuery({ queryKey: gitQueryKey(project.id), queryFn: () => fetchGitSettings(project.id) });
  if (settings.isPending) return null;
  if (!settings.data) {
    const notGit = settings.error instanceof ApiError && settings.error.status === 409;
    return <div style={{ padding: "28px 12px", font: "400 13px/1.5 var(--font-sans)", color: "var(--text-muted)" }}>{notGit ? "Ce projet déploie une image : il n'a pas de réglages Git." : "Réglages Git indisponibles pour le moment."}</div>;
  }
  return <GitForm key={JSON.stringify(settings.data)} project={project} saved={settings.data} />;
}

function GitForm({ project, saved }: { project: Project; saved: GitSettings }) {
  const client = useQueryClient();
  const { flash } = useToast();
  const role = useRole();
  const canEdit = role === "owner" || role === "admin" || role === "developer";
  const isAdmin = role === "owner" || role === "admin";
  const init = draftOf(saved);
  const [s, setS] = useState<Draft>(init);
  const [saving, setSaving] = useState(false);
  const [created, setCreated] = useState<CreatedDeployHook | null>(null);
  const busy = useRef(false);
  const set = <K extends keyof Draft>(k: K, v: Draft[K]) => setS((x) => ({ ...x, [k]: v }));
  const dirty = JSON.stringify(s) !== JSON.stringify(init);

  const github = useQuery({ queryKey: GITHUB_STATUS_QUERY_KEY, queryFn: fetchGithubStatus, enabled: isAdmin });
  const deployments = useQuery({ queryKey: deploymentsQueryKey(project.id), queryFn: () => fetchDeployments(project.id) });
  const hooks = useQuery({ queryKey: hooksQueryKey(project.id), queryFn: () => fetchDeployHooks(project.id), enabled: isAdmin });
  const lastCommit = deployments.data?.find((d) => d.commit_sha);

  const whUrl = panelUrl("/v1/webhooks/github");
  const [owner, ...rest] = saved.repository.split("/");

  const save = async () => {
    if (busy.current) return;
    busy.current = true;
    setSaving(true);
    try {
      const result = await saveGitSettings(project.id, patchOf(init, s));
      if (!result.ok) {
        flash(result.message, 5000);
        return;
      }
      client.setQueryData(gitQueryKey(project.id), result.settings);
      await client.invalidateQueries({ queryKey: ["projects", project.id], exact: true });
      flash("Paramètres Git enregistrés");
    } finally {
      busy.current = false;
      setSaving(false);
    }
  };

  const addHook = async () => {
    if (busy.current) return;
    busy.current = true;
    try {
      const result = await createDeployHook(project.id, `Hook ${(hooks.data?.length ?? 0) + 1}`);
      if (!result.ok) {
        flash(result.message, 5000);
        return;
      }
      setCreated(result.created);
      await client.invalidateQueries({ queryKey: hooksQueryKey(project.id) });
    } finally {
      busy.current = false;
    }
  };

  const removeHook = async (hookId: string) => {
    if (busy.current) return;
    busy.current = true;
    try {
      const result = await deleteDeployHook(project.id, hookId);
      flash(result.ok ? "Deploy hook supprimé" : result.message, result.ok ? undefined : 5000);
      if (result.ok) await client.invalidateQueries({ queryKey: hooksQueryKey(project.id) });
    } finally {
      busy.current = false;
    }
  };

  return (
    <div style={{ position: "relative", paddingBottom: dirty ? 72 : 0 }}>
      <Section title="Dépôt" desc="Ciphera reçoit les événements push et pull request de ce dépôt via un webhook signé (HMAC SHA-256).">
        <div style={{ borderRadius: "var(--radius-lg)", border: "1px solid var(--border-default)", overflow: "hidden" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 14, padding: "16px 16px" }}>
            <span style={{ width: 40, height: 40, borderRadius: 10, display: "flex", alignItems: "center", justifyContent: "center", background: "var(--surface-selected)", color: "var(--text-strong)", flex: "none" }}>
              <img src={githubMark} alt="" style={{ width: 20, height: 20, objectFit: "contain" }} />
            </span>
            <div style={{ display: "flex", flexDirection: "column", gap: 7, flex: 1, minWidth: 0 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <span style={{ font: "500 14px/1 var(--font-sans)", color: "var(--text-strong)" }}>{rest.length ? rest.join("/") : owner}</span>
                <a href={`https://github.com/${saved.repository}`} target="_blank" rel="noreferrer noopener" aria-label="Ouvrir le dépôt sur GitHub" style={{ display: "flex", color: "var(--text-muted)" }}>
                  <Icon name="ArrowUpRight" size={13} />
                </a>
              </div>
              <span style={{ ...lbl, display: "flex", alignItems: "center", gap: 8 }}>
                {github.data?.app ? `GitHub App · ${github.data.app.slug}` : `GitHub · ${owner}`}
                {github.data && (
                  <>
                    <span style={{ color: "var(--text-faint)" }}>·</span>
                    {github.data.connected ? <StatusDot status="ok" label="Webhook actif" /> : <StatusDot status="off" label="GitHub non connecté" />}
                  </>
                )}
              </span>
            </div>
            <Button size="sm" disabled title={SOON}>
              Changer
            </Button>
            <Button size="sm" variant="ghost" disabled title={SOON}>
              Déconnecter
            </Button>
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "12px 16px", borderTop: hair, background: "var(--surface-raised)", font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" }}>
            <Icon name="GitCommitHorizontal" size={13} />
            Dernier commit déployé sur {mono(saved.branch, "var(--text-body)")}
            <span style={{ flex: 1 }} />
            {lastCommit ? (
              <>
                {mono(shortSha(lastCommit.commit_sha))}
                <span>{timeAgo(lastCommit.created_at)}</span>
              </>
            ) : (
              <span>aucun pour l'instant</span>
            )}
          </div>
        </div>
      </Section>

      <Section title="Branches" desc="La branche de production met à jour vos domaines. Les autres branches correspondant aux motifs obtiennent un déploiement de preview.">
        <div style={{ display: "flex", gap: 16 }}>
          <Field label="Branche de production">
            <input value={s.branch} disabled={!canEdit} onChange={(e) => set("branch", e.target.value)} style={field} />
          </Field>
          <Field label="Dossier racine" hint="Monorepo : chemin de l'application">
            <input value={s.root} disabled={!canEdit} onChange={(e) => set("root", e.target.value)} style={field} />
          </Field>
        </div>
        <Field label="Branches de preview" hint="Arrive avec les previews de pull request (v0.3)">
          <Chips label="Branches de preview" items={[]} onChange={() => undefined} placeholder="feat/*" disabled />
        </Field>
        <Field label="Chemins surveillés" hint="Un build n'est lancé que si un fichier correspondant change">
          <Chips label="Chemins surveillés" items={s.watch} onChange={(v) => set("watch", v)} placeholder="apps/web/**" disabled={!canEdit} />
        </Field>
      </Section>

      <Section title="Déclencheurs" desc="Contrôlez quand un push ou une pull request lance un build.">
        <div style={{ borderRadius: "var(--radius-lg)", border: "1px solid var(--border-default)" }}>
          {(
            [
              ["auto", "Déploiement automatique", `Chaque push sur ${s.branch} déclenche un build et un déploiement en production.`],
              ["prev", "Previews de pull request", "Un environnement éphémère par PR, supprimé à la fusion ou à la fermeture."],
              ["comment", "Commenter les pull requests", "Le bot Ciphera publie l'URL de preview et le statut du build dans la PR."],
              ["skip", "Respecter [skip ci]", "Ignore les commits dont le message contient [skip ci] ou [ciphera skip]."],
              ["ignoreBots", "Ignorer les commits de bots", "Dependabot, Renovate et autres comptes *[bot]."],
            ] as const
          ).map(([k, t, d], i) => (
            <div key={k} style={{ padding: "14px 16px", borderTop: i ? hair : "none" }}>
              {k === "prev" || k === "comment" ? (
                <Switch checked={false} disabled label={t} description={`${d} (${SOON.toLowerCase()})`} />
              ) : (
                <Switch checked={s[k]} disabled={!canEdit} onChange={(v) => set(k, v)} label={t} description={d} />
              )}
            </div>
          ))}
        </div>
      </Section>

      <Section title="Webhook" desc="Configuré automatiquement par l'intégration GitHub. Gitea et GitLab auto-hébergés (webhook générique) : v0.2.">
        <Field label="URL de payload">
          <div style={{ ...field, display: "flex", alignItems: "center", gap: 8, paddingRight: 2 }}>
            <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{whUrl}</span>
            <CopyButton value={whUrl} size={28} label="Copier l'URL de payload" onCopied={() => flash("Copié")} />
          </div>
        </Field>
        <Field label="Secret de signature" hint="Géré par la GitHub App : chiffré, jamais réaffiché.">
          <div style={{ display: "flex", gap: 8 }}>
            <div style={{ ...field, flex: 1, display: "flex", alignItems: "center", gap: 4, paddingRight: 2 }}>
              <span style={{ flex: 1, minWidth: 0, overflow: "hidden" }}>{"•".repeat(24)}</span>
              <IconButton size={28} variant="ghost" icon="Eye" label="Afficher" disabled />
            </div>
            <Button icon="RefreshCw" disabled title={SOON}>
              Régénérer
            </Button>
          </div>
        </Field>
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <span style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-muted)" }}>Livraisons récentes</span>
          <div style={{ borderRadius: "var(--radius-lg)", border: "1px solid var(--border-default)", padding: "13px 14px", ...lbl }}>Historique des livraisons : {SOON.toLowerCase()}.</div>
        </div>
      </Section>

      <Section title="Deploy hooks" desc="URL secrète qui déclenche un déploiement par simple POST, depuis une CI externe ou un CMS." last>
        {!isAdmin ? (
          <span style={lbl}>Réservé aux administrateurs.</span>
        ) : hooks.isError ? (
          <span style={lbl}>Deploy hooks indisponibles pour le moment.</span>
        ) : (
          <>
            {(hooks.data ?? []).map((h) => (
              <div key={h.id} style={{ display: "flex", alignItems: "center", gap: 12, padding: "12px 8px 12px 14px", borderRadius: "var(--radius-lg)", border: "1px solid var(--border-default)" }}>
                <Icon name="Webhook" size={15} color="var(--text-muted)" />
                <div style={{ display: "flex", flexDirection: "column", gap: 6, flex: 1, minWidth: 0 }}>
                  <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>
                    {h.name} <span style={{ ...lbl, fontWeight: 400 }}>· {saved.branch}</span>
                  </span>
                  <span style={{ font: "400 11px/1 var(--font-mono)", color: "var(--text-subtle)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{panelUrl(h.path)}?token=••••••••</span>
                </div>
                <CopyButton value={`curl -X POST '${panelUrl(h.path)}?token=<secret>'`} size={28} label={`Copier la commande de ${h.name}`} onCopied={() => flash("Copié")} />
                <IconButton size={28} variant="ghost" icon="Trash" label={`Supprimer ${h.name}`} onClick={() => void removeHook(h.id)} />
              </div>
            ))}
            <div>
              <Button icon="Plus" disabled={!hooks.isSuccess} onClick={() => void addHook()}>
                Créer un deploy hook
              </Button>
            </div>
          </>
        )}
      </Section>

      {dirty && canEdit && (
        <div style={{ position: "sticky", bottom: 16, marginTop: -56, display: "flex", justifyContent: "center", pointerEvents: "none" }}>
          <div style={{ pointerEvents: "auto", display: "flex", alignItems: "center", gap: 12, padding: "8px 8px 8px 16px", borderRadius: 12, background: "var(--surface-tooltip)", boxShadow: "var(--shadow-tooltip)" }}>
            <span style={{ width: 6, height: 6, borderRadius: 99, background: "#e5b454" }}></span>
            <span style={{ font: "400 13px/1 var(--font-sans)", color: "var(--text-body)" }}>Modifications non enregistrées</span>
            <Button size="sm" variant="ghost" onClick={() => setS(init)}>
              Annuler
            </Button>
            <Button size="sm" variant="inverse" disabled={saving} onClick={() => void save()}>
              Enregistrer
            </Button>
          </div>
        </div>
      )}
      {created && (
        <TokenCreatedModal
          title="Deploy hook créé"
          description={`${created.hook.name} déclenche un déploiement de ${project.name} (branche ${saved.branch}).`}
          label="URL du deploy hook"
          value={panelUrl(created.url_path)}
          example={`curl -X POST '${panelUrl(created.url_path)}'`}
          onClose={() => setCreated(null)}
        />
      )}
    </div>
  );
}
