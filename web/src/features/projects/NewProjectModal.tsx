import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Fragment, useState, type CSSProperties } from "react";

import { fetchGithubRepos, GITHUB_REPOS_QUERY_KEY, type GithubRepo } from "@/features/connectors/api";
import { Badge, Button, Icon, Modal, RadioCard, Select, StatusDot, Switch, TextField, type IconName } from "@/design-system";

import { createDeployment, DEPLOYMENTS_QUERY_KEY, deploymentQueryKey, fetchDeployment, isActive, POLL_MS, type Deployment } from "@/features/deployments/api";

import { createProject, type Build, type NewProject, type Project } from "./api";
import { updateServiceSecurity } from "./overview/api";
import { DeployProgress } from "./DeployProgress";
import { timeAgo } from "./format";

// Projects.jsx#NewProjectModal. Liste des depots : ceux de la GitHub App de
// l'instance (`/api/git/providers/github/repos`) ; sans App connectee (ou sans
// depot installe), saisie libre du depot. Retires de la maquette faute de
// backend en v0.1 : dossier racine, commandes Railpack, variables
// d'environnement, serveur cible, deploiement automatique et previews (aucun
// champ de l'API). La creation lance le premier deploiement (POST
// /api/projects/{id}/redeploy) et l'etape finale suit son avancement
// (DeployProgress, GET /api/deployments/{id} toutes les 3 s).
// Fournisseurs sans prise en charge : tuiles visibles mais desactivees, badge
// v0.2 (GitLab, Gitea, Bitbucket, URL Git + cle de deploiement, decision du
// 2026-11-23).

/** Utilisateur proposé par « Exécuter sans root ». */
const NON_ROOT = "1000:1000";
const lbl: CSSProperties = { font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" };
const hair = "1px solid var(--border-default)";

type Provider = "github" | "gitlab" | "gitea" | "bitbucket" | "git" | "docker";
const PROVIDERS: Record<Provider, { icon: IconName; name: string; supported: boolean }> = {
  github: { icon: "GitBranch", name: "GitHub", supported: true },
  gitlab: { icon: "GitMerge", name: "GitLab", supported: false },
  gitea: { icon: "GitBranch", name: "Gitea", supported: false },
  bitbucket: { icon: "GitFork", name: "Bitbucket", supported: false },
  git: { icon: "Link", name: "URL Git", supported: false },
  docker: { icon: "Container", name: "Image Docker", supported: true },
};

const STEPS = ["Source", "Configuration", "Déploiement"];

function Stepper({ step }: { step: number }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
      {STEPS.map((s, i) => (
        <Fragment key={s}>
          {i > 0 && <span style={{ width: 24, height: 1, background: "var(--border-strong)" }} />}
          <span style={{ display: "flex", alignItems: "center", gap: 8, font: "500 12px/1 var(--font-sans)", color: i <= step ? "var(--text-strong)" : "var(--text-faint)" }}>
            <span style={{ width: 18, height: 18, borderRadius: 99, display: "flex", alignItems: "center", justifyContent: "center", background: i < step ? "var(--surface-inverse)" : i === step ? "var(--surface-selected)" : "transparent", border: i > step ? "1px solid var(--border-strong)" : "none", color: i < step ? "var(--text-inverse)" : "inherit", fontSize: 10 }}>
              {i < step ? <Icon name="Check" size={11} /> : i + 1}
            </span>
            {s}
          </span>
        </Fragment>
      ))}
    </div>
  );
}

/** Nom de projet propose : dernier segment du depot ou de l'image, en minuscules sans tag. */
function suggestName(provider: Provider, input: string): string {
  const withoutTag = provider === "docker" ? input.trim().split("@")[0].replace(/:[^/]*$/, "") : input.trim().replace(/\.git$/, "").replace(/\/$/, "");
  const last = withoutTag.split("/").pop() ?? "";
  return last
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 63);
}

export interface NewProjectModalProps {
  onClose: () => void;
  /** « Ouvrir le projet » (etape finale) */
  onOpen: (projectId: string) => void;
  onCreated?: (project: Project) => void;
}

export function NewProjectModal({ onClose, onOpen, onCreated }: NewProjectModalProps) {
  const [step, setStep] = useState(0);
  const [provider, setProvider] = useState<Provider>("github");
  const [source, setSource] = useState("");
  const [name, setName] = useState("");
  const [branch, setBranch] = useState("main");
  const [kind, setKind] = useState<Build["kind"]>("railpack");
  const [dockerfilePath, setDockerfilePath] = useState("Dockerfile");
  const [composePath, setComposePath] = useState("compose.yaml");
  const [composeService, setComposeService] = useState("");
  const [pending, setPending] = useState(false);
  // Source image : exécuter sans root (uid 1000:1000). L'USER de l'image n'est pas lisible à la création : décoché par défaut.
  const [nonRoot, setNonRoot] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [created, setCreated] = useState<Project | null>(null);
  const [first, setFirst] = useState<Deployment | null>(null);
  const [launchError, setLaunchError] = useState<string | null>(null);
  const queryClient = useQueryClient();
  const isImage = provider === "docker";
  // `null` : GitHub non connecte (ou injoignable) -> saisie libre.
  const reposQuery = useQuery({ queryKey: GITHUB_REPOS_QUERY_KEY, queryFn: fetchGithubRepos, retry: false, staleTime: 60_000 });
  const repos = reposQuery.data ?? null;
  const owners = repos ? [...new Set(repos.map((r) => r.owner))] : [];
  const [owner, setOwner] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const currentOwner = owner ?? owners[0] ?? "";
  const shown = (repos ?? []).filter((r) => r.owner === currentOwner && r.full_name.toLowerCase().includes(q.trim().toLowerCase()));
  const progress = useQuery({ queryKey: deploymentQueryKey(first?.id ?? ""), queryFn: () => fetchDeployment(first!.id), enabled: !!first, refetchInterval: (q) => (q.state.data && !isActive(q.state.data) ? false : POLL_MS) });
  const live = progress.data ?? first;
  const picker = provider === "github" && repos !== null && repos.length > 0;

  const next = () => {
    setName(suggestName(provider, source));
    setError(null);
    setStep(1);
  };

  const pick = (r: GithubRepo) => {
    setSource(r.full_name);
    setBranch(r.default_branch);
    setName(suggestName("github", r.full_name));
    setError(null);
    setStep(1);
  };

  const submit = async () => {
    const build: Build = kind === "railpack" ? { kind } : kind === "dockerfile" ? { kind, path: dockerfilePath.trim() } : { kind, path: composePath.trim(), service: composeService.trim() };
    const body: NewProject = isImage ? { name: name.trim(), source: { type: "image", image: source.trim() } } : { name: name.trim(), source: { type: "git", repository: source.trim(), branch: branch.trim() }, build };
    setPending(true);
    setError(null);
    const result = await createProject(body);
    setPending(false);
    if (!result.ok) return setError(result.message);
    setCreated(result.project);
    onCreated?.(result.project);
    setStep(2);
    // Sans root : réglé avant le premier déploiement ; un refus laisse le projet créé, sans le lancer en root.
    const serviceId = result.project.services[0]?.id;
    if (isImage && nonRoot && serviceId) {
      const security = await updateServiceSecurity(result.project.id, serviceId, { run_as: NON_ROOT, read_only_rootfs: false });
      if (!security.ok) return setLaunchError(`Le réglage « sans root » n'a pas été enregistré : ${security.message} Le projet est créé, aucun déploiement n'a démarré.`);
    }
    // Le premier deploiement part des la creation ; un refus laisse le projet cree.
    const launched = await createDeployment(result.project.id);
    if (launched.ok) {
      setFirst(launched.deployment);
      void queryClient.invalidateQueries({ queryKey: DEPLOYMENTS_QUERY_KEY });
    } else setLaunchError(launched.message);
  };

  const footer =
    step === 0 ? (
      <Button onClick={onClose}>Annuler</Button>
    ) : step === 1 ? (
      <>
        <Button onClick={() => setStep(0)} icon="ArrowLeft">
          Retour
        </Button>
        <Button variant="inverse" icon="Rocket" disabled={pending || !name.trim()} onClick={() => void submit()}>
          Créer le projet
        </Button>
      </>
    ) : (
      <>
        <Button onClick={onClose}>Fermer</Button>
        <Button variant="inverse" iconRight="ArrowRight" onClick={() => created && onOpen(created.id)}>
          Ouvrir le projet
        </Button>
      </>
    );

  return (
    <Modal title="Nouveau projet" description={<Stepper step={step} />} onClose={onClose} width={720} footer={footer}>
      <div style={{ maxHeight: "62vh", overflowY: "auto", display: "flex", flexDirection: "column", gap: 20, margin: "0 -2px", padding: "0 2px" }} className="ds-scroll-hidden">
        {step === 0 && (
          <>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(6,minmax(0,1fr))", gap: 6 }}>
              {(Object.entries(PROVIDERS) as [Provider, (typeof PROVIDERS)[Provider]][]).map(([k, v]) => (
                <button
                  key={k}
                  type="button"
                  disabled={!v.supported}
                  title={v.supported ? undefined : "Disponible en v0.2"}
                  onClick={() => {
                    setProvider(k);
                    setSource("");
                  }}
                  style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 8, padding: "14px 6px", borderRadius: "var(--radius-lg)", cursor: v.supported ? "pointer" : "not-allowed", opacity: v.supported ? 1 : 0.5, background: provider === k ? "var(--surface-selected)" : "var(--surface-control)", border: "1px solid " + (provider === k ? "var(--gray-500)" : "var(--border-strong)"), color: provider === k ? "var(--text-strong)" : "var(--text-muted)", font: "500 12px/1 var(--font-sans)" }}
                >
                  <Icon name={v.icon} size={18} />
                  {v.name}
                  {!v.supported && <Badge>v0.2</Badge>}
                </button>
              ))}
            </div>
            {picker && (
              <>
                <div style={{ display: "flex", gap: 8 }}>
                  {owners.length > 1 && <Select options={owners} value={currentOwner} onChange={setOwner} />}
                  <div style={{ display: "flex", alignItems: "center", gap: 8, height: 32, padding: "0 10px", flex: 1, borderRadius: "var(--radius-md)", border: "1px solid var(--border-strong)", background: "var(--surface-control)" }}>
                    <Icon name="Search" size={14} color="var(--text-subtle)" />
                    <input autoFocus value={q} onChange={(e) => setQ(e.target.value)} placeholder="Rechercher un dépôt…" aria-label="Rechercher un dépôt" style={{ flex: 1, background: "transparent", border: 0, outline: "none", color: "var(--text-strong)", font: "400 13px/1 var(--font-sans)" }} />
                  </div>
                </div>
                <div style={{ display: "flex", flexDirection: "column", border: "1px solid var(--border-default)", borderRadius: "var(--radius-lg)" }}>
                  {shown.map((r, i) => (
                    <div key={r.full_name} style={{ display: "flex", alignItems: "center", gap: 12, padding: "12px 14px", borderTop: i ? hair : "none" }}>
                      <Icon name="GitBranch" size={15} />
                      <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>{r.full_name}</span>
                      {r.private && <Icon name="Lock" size={11} color="var(--text-faint)" aria-label="Privé" />}
                      <span style={lbl}>
                        {r.default_branch} · {r.updated_at ? timeAgo(r.updated_at) : "—"}
                      </span>
                      <span style={{ flex: 1 }} />
                      <Button size="sm" variant="filled" aria-label={"Importer " + r.full_name} onClick={() => pick(r)}>
                        Importer
                      </Button>
                    </div>
                  ))}
                  {!shown.length && <div style={{ padding: "20px 14px", ...lbl }}>Aucun dépôt trouvé.</div>}
                </div>
              </>
            )}
            {!picker && (
            <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
              <TextField
                label={isImage ? "Image" : "Dépôt GitHub"}
                mono
                value={source}
                onChange={setSource}
                placeholder={isImage ? "ghcr.io/org/app:latest" : "https://github.com/org/app ou org/app"}
                hint={isImage ? "Image publique d'un registre (ghcr.io, docker.io…)." : "Dépôt public. Connectez la GitHub App (Connecteurs) pour importer vos dépôts privés."}
              />
              <div>
                <Button variant="filled" iconRight="ArrowRight" disabled={!source.trim()} onClick={next}>
                  Continuer
                </Button>
              </div>
            </div>
            )}
          </>
        )}
        {step === 1 && (
          <>
            <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "12px 14px", borderRadius: "var(--radius-lg)", border: "1px solid var(--border-default)" }}>
              <Icon name={PROVIDERS[provider].icon} size={16} />
              <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)", wordBreak: "break-all" }}>{source.trim()}</span>
              <span style={{ flex: 1 }} />
              {!isImage && <TextField label="Branche" mono value={branch} onChange={setBranch} style={{ width: 200 }} />}
            </div>
            <TextField label="Nom du projet" value={name} onChange={setName} hint="Minuscules, chiffres et tirets." />
            {isImage && <Switch label="Exécuter sans root (recommandé)" description="Le conteneur tourne sous l'utilisateur 1000:1000 au lieu de celui de l'image. Si l'image déclare déjà un USER non root, laissez décoché. Modifiable ensuite dans Sécurité." checked={nonRoot} onChange={setNonRoot} />}
            {!isImage && (
              <>
                <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                  <span style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-muted)" }}>Méthode de build</span>
                  <div style={{ display: "grid", gridTemplateColumns: "repeat(3,minmax(0,1fr))", gap: 8 }}>
                    <RadioCard icon="Package" title="Railpack" description="Détection auto" selected={kind === "railpack"} onClick={() => setKind("railpack")} />
                    <RadioCard icon="FileCode" title="Dockerfile" description="./Dockerfile" selected={kind === "dockerfile"} onClick={() => setKind("dockerfile")} />
                    <RadioCard icon="Boxes" title="Compose" description="compose.yaml" selected={kind === "compose"} onClick={() => setKind("compose")} />
                  </div>
                </div>
                {kind === "dockerfile" && <TextField label="Chemin du Dockerfile" mono value={dockerfilePath} onChange={setDockerfilePath} />}
                {kind === "compose" && (
                  <div style={{ display: "flex", gap: 16 }}>
                    <TextField label="Fichier Compose" mono value={composePath} onChange={setComposePath} style={{ flex: 1 }} />
                    <TextField label="Service exposé" mono value={composeService} onChange={setComposeService} placeholder="web" style={{ flex: 1 }} />
                  </div>
                )}
              </>
            )}
            {error && (
              <div role="alert" style={{ padding: "10px 14px", borderRadius: "var(--radius-lg)", background: "rgba(239,68,68,.05)", border: "1px solid rgba(239,68,68,.18)", font: "400 12px/1.4 var(--font-sans)", color: "var(--text-body)" }}>
                {error}
              </div>
            )}
          </>
        )}
        {step === 2 && created && !launchError && <DeployProgress deployment={live} subtitle={`${created.name} · ${isImage ? source.trim() : branch.trim()} · production`} />}
        {step === 2 && created && launchError && (
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              <span style={{ font: "500 16px/1.2 var(--font-sans)", color: "var(--text-strong)" }}>Projet créé</span>
              <span style={lbl}>{created.name} · environnement production. Le premier déploiement n'a pas pu être lancé : {launchError}</span>
            </div>
            <StatusDot status="off" label="Pas encore déployé" />
          </div>
        )}
      </div>
    </Modal>
  );
}
