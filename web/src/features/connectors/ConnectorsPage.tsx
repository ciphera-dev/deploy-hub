import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";

import cipheraIcon from "@/assets/brand/ciphera-icon.svg";
import { Button, Icon, IconButton, Modal, StatusDot, Switch, useToast, type IconName } from "@/design-system";

import { completeGithubManifest, disconnectGithub, fetchGithubStatus, GITHUB_STATUS_QUERY_KEY, startGithubManifest, type GithubStatus } from "./api";
import { CATS, LIB, rgb, type Connector } from "./catalog";
import { ConnectorCard, Mark } from "./GithubCard";

// Connectors.jsx#ConnectorsPageV2. Etat reel : la GitHub App de l'instance
// (`/api/connectors/github`), plus rien dans localStorage. Retires faute de
// source en v0.1 : activite et volume d'evenements, declencheurs et journal du
// tiroir de configuration, mise en pause. Les connecteurs non branches sont
// visibles avec leur version (v0.2/v0.3).

const lbl: CSSProperties = { font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" };
const line = "1px solid rgba(255,255,255,.06)";
const box: CSSProperties = { borderRadius: 20, border: "1px solid rgba(255,255,255,.08)", background: "#0c0c0f" };
const github = LIB.find((c) => c.id === "github")!;

/** Permissions demandees par le manifest de l'App (lecture seule, webhook push). */
const PERMS: [string, boolean][] = [
  ["Lire les dépôts et branches", true],
  ["Créer des webhooks de déploiement", true],
  ["Publier le statut des builds", false],
];

const STEPS: [IconName, string, string][] = [
  ["LayoutGrid", "Choisissez un service", "Code source, DNS, sécurité, base de données ou IA : chaque connecteur s'active en un clic."],
  ["KeyRound", "Autorisez l'accès", "Connexion OAuth ou clé d'API. Vous choisissez les permissions, révocables à tout moment."],
  ["Zap", "Laissez Ciphera travailler", "Déploiements, DNS, bannissements et analyses de logs se synchronisent automatiquement."],
];

function ConnectHero({ c, busy }: { c: Connector; busy: boolean }) {
  const C = rgb(c.col);
  const side = (child: ReactNode, name: string, sub: string, brand?: boolean) => (
    <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 10, width: 104, flex: "none" }}>
      <span style={{ width: 60, height: 60, borderRadius: 17, display: "flex", alignItems: "center", justifyContent: "center", background: brand ? "linear-gradient(160deg,rgba(" + C + ",.20),rgba(" + C + ",.05)) , #131316" : "linear-gradient(160deg,#1d1d22,#121215)", border: "1px solid " + (brand ? "rgba(" + C + ",.32)" : "rgba(255,255,255,.12)"), boxShadow: "inset 0 1px 0 rgba(255,255,255,.08)" }}>{child}</span>
      <span style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 4 }}>
        <span style={{ font: "600 12px/1 var(--font-sans)", color: "var(--text-strong)", whiteSpace: "nowrap" }}>{name}</span>
        <span style={{ font: "400 11px/1 var(--font-sans)", color: "var(--text-subtle)", whiteSpace: "nowrap" }}>{sub}</span>
      </span>
    </div>
  );
  return (
    <div style={{ position: "relative", display: "flex", alignItems: "flex-start", justifyContent: "center", padding: "24px 20px 20px", borderRadius: 16, overflow: "hidden", isolation: "isolate", border: "1px solid rgba(255,255,255,.07)", background: "linear-gradient(90deg,rgba(255,255,255,.025),rgba(" + C + ",.07)),#0e0e11" }}>
      {side(<img src={cipheraIcon} alt="Ciphera" style={{ width: 28, height: "auto", display: "block" }} />, "Ciphera", "Votre espace")}
      <div style={{ position: "relative", flex: 1, maxWidth: 200, height: 60, display: "flex", alignItems: "center", justifyContent: "center" }}>
        <span style={{ position: "absolute", left: -6, right: -6, top: "50%", height: 1, background: "linear-gradient(90deg,rgba(255,255,255,.16),rgba(" + C + ",.45))" }} />
        <span style={{ position: "relative", display: "flex", alignItems: "center", gap: 6, height: 26, padding: "0 10px", borderRadius: 99, background: "#141417", border: "1px solid rgba(255,255,255,.12)", font: "500 11px/1 var(--font-sans)", color: "var(--text-body)", whiteSpace: "nowrap" }}>
          <Icon name={busy ? "Loader" : "Lock"} size={11} color="var(--text-subtle)" />
          {busy ? "Redirection…" : "GitHub App"}
        </span>
      </div>
      {side(c.img ? <img src={c.img} alt="" style={{ width: 28, height: 28, objectFit: "contain" }} /> : null, c.n, c.org || c.cat, true)}
    </div>
  );
}

function ConnectModal({ onClose }: { onClose: () => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const go = async () => {
    setBusy(true);
    setError(null);
    const result = await startGithubManifest();
    if (!result.ok) {
      setBusy(false);
      setError(result.message);
    }
  };
  return (
    <Modal
      title="Connecter GitHub"
      onClose={onClose}
      width={520}
      footer={
        <>
          <Button onClick={onClose}>Annuler</Button>
          <Button variant="inverse" icon={busy ? "Loader" : "ExternalLink"} disabled={busy} onClick={() => void go()}>
            {busy ? "En attente de GitHub…" : "Autoriser sur GitHub"}
          </Button>
        </>
      }
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
        <ConnectHero c={github} busy={busy} />
        <div style={{ display: "flex", flexDirection: "column", gap: 6, textAlign: "center" }}>
          <span style={{ font: "400 13px/1.5 var(--font-sans)", color: "var(--text-muted)" }}>Ciphera crée une GitHub App propre à cette instance. Installez-la ensuite sur les dépôts à déployer.</span>
        </div>
        <div style={{ borderRadius: 12, border: line, overflow: "hidden" }}>
          {PERMS.map(([p, on], i) => (
            <div key={p} style={{ display: "flex", alignItems: "center", gap: 12, padding: "12px 14px", borderTop: i ? line : "none" }}>
              <Icon name={on ? "CircleCheck" : "Circle"} size={15} color={on ? github.col : "var(--text-faint)"} />
              <span style={{ flex: 1, font: "400 13px/1 var(--font-sans)", color: "var(--text-body)" }}>{p}</span>
              <Switch checked={on} disabled ariaLabel={p} />
            </div>
          ))}
        </div>
        {error && (
          <div role="alert" style={{ padding: "10px 14px", borderRadius: "var(--radius-lg)", background: "rgba(239,68,68,.05)", border: "1px solid rgba(239,68,68,.18)", font: "400 12px/1.4 var(--font-sans)", color: "var(--text-body)" }}>
            {error}
          </div>
        )}
        <span style={{ display: "flex", alignItems: "center", gap: 8, justifyContent: "center", ...lbl, fontSize: 11 }}>
          <Icon name="Lock" size={11} />
          Clé privée et secret de webhook chiffrés, jamais renvoyés par l'API.
        </span>
      </div>
    </Modal>
  );
}

function ConfigDrawer({ status, onClose, onDisconnect }: { status: GithubStatus; onClose: () => void; onDisconnect: () => void }) {
  const C = rgb(github.col);
  const app = status.app;
  return (
    <div style={{ position: "fixed", inset: 0, zIndex: 70, display: "flex", justifyContent: "flex-end", background: "rgba(0,0,0,.5)", backdropFilter: "blur(2px)" }} onClick={onClose}>
      <div role="dialog" aria-label="Configuration de GitHub" onClick={(e) => e.stopPropagation()} style={{ width: 440, height: "100%", display: "flex", flexDirection: "column", background: "#0c0c0f", borderLeft: "1px solid rgba(255,255,255,.08)", boxShadow: "-24px 0 60px rgba(0,0,0,.5)", animation: "drawerIn .3s cubic-bezier(.2,.8,.2,1)" }}>
        <div style={{ position: "relative", padding: "22px 20px 18px", background: "radial-gradient(120% 100% at 0% 0%,rgba(" + C + ",.18),transparent 60%)", borderBottom: line }}>
          <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
            <Mark c={github} size={48} />
            <span style={{ display: "flex", flexDirection: "column", gap: 7, flex: 1 }}>
              <span style={{ font: "600 17px/1 var(--font-sans)", color: "var(--text-strong)" }}>{github.n}</span>
              <StatusDot status="ok" label={"Connecté · " + (app?.owner ?? "")} />
            </span>
            <IconButton size={30} variant="ghost" icon="X" label="Fermer" onClick={onClose} />
          </div>
        </div>
        <div style={{ flex: 1, overflow: "auto", padding: 20, display: "flex", flexDirection: "column", gap: 22 }} className="log-scroll">
          <section style={{ display: "flex", flexDirection: "column", gap: 10 }}>
            <span style={{ font: "500 12px/1 var(--font-sans)", color: "var(--text-muted)" }}>Permissions accordées</span>
            <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
              {["Contenu (lecture)", "Métadonnées (lecture)", "Pull requests (lecture)", "Webhook push"].map((s) => (
                <span key={s} style={{ display: "flex", alignItems: "center", gap: 6, height: 26, padding: "0 10px", borderRadius: 8, background: "rgba(" + C + ",.08)", border: "1px solid rgba(" + C + ",.22)", font: "500 12px/1 var(--font-sans)", color: "var(--text-strong)" }}>
                  <Icon name="Check" size={11} color={github.col} />
                  {s}
                </span>
              ))}
            </div>
          </section>
          {app && (
            <section style={{ display: "flex", flexDirection: "column", gap: 10 }}>
              <span style={{ font: "500 12px/1 var(--font-sans)", color: "var(--text-muted)" }}>Application</span>
              <span style={{ font: "400 12px/1 var(--font-mono)", color: "var(--text-body)" }}>{app.name}</span>
              <a href={app.install_url} target="_blank" rel="noopener noreferrer" style={{ display: "flex", alignItems: "center", gap: 6, font: "500 12px/1 var(--font-sans)", color: "var(--text-strong)" }}>
                <Icon name="ArrowUpRight" size={12} />
                Installer sur des dépôts
              </a>
            </section>
          )}
        </div>
        <div style={{ display: "flex", gap: 8, padding: "14px 20px", borderTop: line }}>
          <Button icon="Unplug" variant="ghost" onClick={onDisconnect}>
            Déconnecter
          </Button>
          <span style={{ flex: 1 }} />
          <Button variant="inverse" onClick={onClose}>
            Fermer
          </Button>
        </div>
      </div>
    </div>
  );
}

function ConnRow({ status, onConfig, onDisconnect }: { status: GithubStatus; onConfig: () => void; onDisconnect: () => void }) {
  const [h, setH] = useState(false);
  return (
    <div onMouseEnter={() => setH(true)} onMouseLeave={() => setH(false)} style={{ display: "grid", gridTemplateColumns: "minmax(0,1.5fr) minmax(0,1.2fr) 120px 200px", gap: 20, alignItems: "center", padding: "14px 16px 14px 18px", background: h ? "rgba(255,255,255,.02)" : "transparent", transition: "background .15s" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12, minWidth: 0 }}>
        <span style={{ width: 34, height: 34, borderRadius: 9, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", background: "#16161a", border: "1px solid rgba(255,255,255,.08)" }}>
          <img src={github.img} alt="" style={{ width: 18, height: 18, objectFit: "contain" }} />
        </span>
        <span style={{ display: "flex", flexDirection: "column", gap: 6, minWidth: 0 }}>
          <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>{github.n}</span>
          <span style={{ ...lbl, fontSize: 11 }}>{github.cat}</span>
        </span>
      </div>
      <span style={{ display: "flex", alignItems: "center", gap: 6, minWidth: 0, font: "400 12px/1 var(--font-mono)", color: "var(--text-muted)" }}>
        <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{status.app?.owner}</span>
      </span>
      <StatusDot status="ok" label="Actif" />
      <div style={{ display: "flex", alignItems: "center", gap: 4, justifyContent: "flex-end" }}>
        {status.app && (
          <a href={status.app.install_url} target="_blank" rel="noopener noreferrer" style={{ font: "500 12px/1 var(--font-sans)", color: "var(--text-muted)", padding: "0 8px" }}>
            Installer
          </a>
        )}
        <Button size="sm" variant="ghost" onClick={onConfig}>
          Configurer
        </Button>
        <IconButton size={28} variant="ghost" icon="Unplug" label="Déconnecter" onClick={onDisconnect} />
      </div>
    </div>
  );
}

export interface ConnectorsPageProps {
  /** Parametres de retour de GitHub (`/connectors?code=…&state=…` ou `?installation_id=…`). */
  search: { code?: string; state?: string; installation_id?: string };
  /** Retire ces parametres de l'URL. */
  clearSearch: () => void;
}

export function ConnectorsPage({ search, clearSearch }: ConnectorsPageProps) {
  const queryClient = useQueryClient();
  const { flash } = useToast();
  const statusQuery = useQuery({ queryKey: GITHUB_STATUS_QUERY_KEY, queryFn: fetchGithubStatus });
  const status = statusQuery.data;
  const isOn = (c: Connector) => c.id === "github" && !!status?.connected;
  const [cat, setCat] = useState("Tous");
  const [q, setQ] = useState("");
  const [connecting, setConnecting] = useState(false);
  const [config, setConfig] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [callbackError, setCallbackError] = useState<string | null>(null);
  const handled = useRef(false);

  // Retour du flux manifest : le code n'est converti qu'une fois.
  const { code, state, installation_id } = search;
  useEffect(() => {
    if (handled.current) return;
    if (code && state) {
      handled.current = true;
      void completeGithubManifest(code, state).then((result) => {
        clearSearch();
        if (!result.ok) return setCallbackError(result.message);
        queryClient.setQueryData(GITHUB_STATUS_QUERY_KEY, result.status);
        flash("GitHub connecté : installez l'App sur vos dépôts");
      });
    } else if (installation_id) {
      handled.current = true;
      clearSearch();
      flash("GitHub App installée");
    }
  }, [code, state, installation_id, clearSearch, queryClient, flash]);

  const match = (c: Connector) => (cat === "Tous" || c.cat === cat) && (c.n + c.d).toLowerCase().includes(q.toLowerCase());
  const nOn = LIB.filter(isOn).length;
  const available = LIB.filter((c) => !isOn(c) && match(c));
  const showConnected = !!status?.connected && match(github);

  const disconnect = async () => {
    setConfirm(false);
    setConfig(false);
    if (await disconnectGithub()) {
      await queryClient.invalidateQueries({ queryKey: GITHUB_STATUS_QUERY_KEY });
      flash("GitHub déconnecté");
    } else flash("Déconnexion impossible. Réessayez.");
  };

  return (
    <div style={{ padding: "20px 32px 40px 34px", display: "flex", flexDirection: "column", gap: 16 }}>
      <div style={{ ...box, display: "grid", gridTemplateColumns: "minmax(0,1.1fr) minmax(0,1fr)", gap: 32, alignItems: "center", padding: "32px 32px 32px 36px", position: "relative", overflow: "hidden", isolation: "isolate" }}>
        <span style={{ position: "absolute", inset: 0, zIndex: -1, background: "radial-gradient(60% 90% at 100% 0%,rgba(139,92,246,.10),transparent 70%)" }} />
        <div style={{ display: "flex", flexDirection: "column", gap: 16, alignItems: "flex-start" }}>
          <span style={{ display: "flex", alignItems: "center", gap: 6, height: 24, padding: "0 10px", borderRadius: 99, background: "rgba(167,139,250,.10)", border: "1px solid rgba(167,139,250,.22)", font: "500 11px/1 var(--font-sans)", color: "#c4b5fd" }}>
            <Icon name="PlugZap" size={12} />
            {nOn} connectés sur {LIB.length}
          </span>
          <span style={{ font: "600 30px/1.15 var(--font-sans)", color: "var(--text-strong)", letterSpacing: "-0.02em", textWrap: "balance" }}>Connectez vos outils à votre infrastructure</span>
          <span style={{ font: "400 14px/1.6 var(--font-sans)", color: "var(--text-muted)", maxWidth: 480, textWrap: "pretty" }}>Reliez vos dépôts, votre DNS, votre sécurité et vos modèles d'IA. Ciphera synchronise tout en arrière-plan, sans configuration manuelle.</span>
          <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
            <button type="button" disabled={!!status?.connected} onClick={() => setConnecting(true)} style={{ display: "flex", alignItems: "center", gap: 8, height: 38, padding: "0 16px", borderRadius: 11, cursor: status?.connected ? "default" : "pointer", border: 0, background: "#ededed", color: "#0b0b0b", font: "600 13px/1 var(--font-sans)", boxShadow: "0 0 0 1px rgba(167,139,250,.5), 0 6px 18px -6px rgba(139,92,246,.6)", opacity: status?.connected ? 0.6 : 1 }}>
              <Icon name="Plus" size={14} />
              Ajouter un connecteur
            </button>
          </div>
        </div>
        <div style={{ position: "relative", padding: "28px 28px 44px", borderRadius: 16, background: "rgba(255,255,255,.025)", border: "1px solid rgba(255,255,255,.05)" }}>
          <div style={{ borderRadius: 12, border: "1px solid rgba(255,255,255,.09)", background: "#111115", overflow: "hidden" }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "10px 12px", borderBottom: line, font: "500 12px/1 var(--font-mono)", color: "var(--text-body)" }}>
              <Icon name="Layers" size={12} color="var(--text-subtle)" />
              ciphera-web
              <span style={{ flex: 1 }} />
              <span style={{ display: "flex", alignItems: "center", gap: 5, ...lbl, fontSize: 11 }}>
                <Icon name="Star" size={11} />
                Production
              </span>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 10, padding: "14px 12px" }}>
              {["github", "cloudflare", "claude"].map((id, i) => {
                const c = LIB.find((x) => x.id === id)!;
                return (
                  <div key={c.id} style={{ display: "flex", alignItems: "center", gap: 10 }}>
                    <Mark c={c} size={26} />
                    <span style={{ flex: 1, font: "500 12px/1 var(--font-sans)", color: "var(--text-strong)" }}>{c.n}</span>
                    <span style={{ height: 5, width: [70, 52, 38][i], borderRadius: 99, background: "rgba(255,255,255,.07)" }} />
                    <Icon name="CircleCheck" size={13} color="#86efac" />
                  </div>
                );
              })}
            </div>
          </div>
          <div style={{ position: "absolute", left: 44, right: 16, bottom: 14, display: "flex", alignItems: "center", gap: 10, padding: "10px 12px", borderRadius: 11, background: "#18181c", border: "1px solid rgba(255,255,255,.1)", boxShadow: "0 14px 30px -10px rgba(0,0,0,.7)" }}>
            <span style={{ width: 18, height: 18, borderRadius: 99, display: "flex", alignItems: "center", justifyContent: "center", background: "rgba(134,239,172,.15)" }}>
              <Icon name="Check" size={11} color="#86efac" />
            </span>
            <span style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              <span style={{ font: "600 12px/1 var(--font-sans)", color: "var(--text-strong)" }}>Webhook GitHub reçu</span>
              <span style={{ ...lbl, fontSize: 11 }}>Déploiement de main lancé automatiquement</span>
            </span>
          </div>
        </div>
      </div>
      <div style={{ ...box, padding: "26px 28px 28px", display: "flex", flexDirection: "column", gap: 18 }}>
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          <span style={{ font: "600 16px/1 var(--font-sans)", color: "var(--text-strong)" }}>Comment ça marche</span>
          <span style={{ font: "400 13px/1 var(--font-sans)", color: "var(--text-muted)" }}>Choisissez, autorisez, c'est synchronisé.</span>
        </div>
        <div style={{ display: "grid", gridTemplateColumns: "repeat(3,minmax(0,1fr))", gap: 12 }}>
          {STEPS.map(([ic, t, d], i) => (
            <div key={t} style={{ display: "flex", flexDirection: "column", gap: 14, padding: 18, borderRadius: 14, background: "rgba(255,255,255,.03)", border: "1px solid rgba(255,255,255,.05)" }}>
              <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
                <span style={{ width: 38, height: 38, borderRadius: 10, display: "flex", alignItems: "center", justifyContent: "center", background: "#16161a", border: "1px solid rgba(255,255,255,.1)" }}>
                  <Icon name={ic} size={16} color="var(--text-strong)" />
                </span>
                <span style={{ ...lbl, fontSize: 11 }}>Étape {i + 1}</span>
              </div>
              <span style={{ font: "600 14px/1.2 var(--font-sans)", color: "var(--text-strong)" }}>{t}</span>
              <span style={{ font: "400 12px/1.55 var(--font-sans)", color: "var(--text-muted)", textWrap: "pretty" }}>{d}</span>
            </div>
          ))}
        </div>
        <div style={{ display: "flex", gap: 12, padding: "14px 16px", borderRadius: 12, background: "rgba(139,92,246,.08)", border: "1px solid rgba(139,92,246,.18)" }}>
          <Icon name="ShieldCheck" size={16} color="#c4b5fd" />
          <span style={{ display: "flex", flexDirection: "column", gap: 5 }}>
            <span style={{ font: "600 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>Vos identifiants restent chiffrés.</span>
            <span style={{ font: "400 12px/1.5 var(--font-sans)", color: "var(--text-muted)" }}>Clés et secrets sont chiffrés avec age côté serveur, jamais renvoyés par l'API, et peuvent être révoqués à tout moment.</span>
          </span>
        </div>
      </div>
      {callbackError && (
        <div role="alert" style={{ padding: "10px 14px", borderRadius: "var(--radius-lg)", background: "rgba(239,68,68,.05)", border: "1px solid rgba(239,68,68,.18)", font: "400 12px/1.4 var(--font-sans)", color: "var(--text-body)" }}>
          {callbackError}
        </div>
      )}
      <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap", marginTop: 8 }}>
        <div style={{ display: "flex", gap: 2, padding: 3, borderRadius: 11, border: "1px solid rgba(255,255,255,.07)", background: "rgba(255,255,255,.02)", flexWrap: "wrap" }}>
          {CATS.map((x) => {
            const on = cat === x;
            const n = x === "Tous" ? LIB.length : LIB.filter((c) => c.cat === x).length;
            return (
              <button key={x} type="button" onClick={() => setCat(x)} style={{ display: "flex", alignItems: "center", gap: 6, height: 30, padding: "0 11px", border: 0, borderRadius: 8, cursor: "pointer", font: "500 12px/1 var(--font-sans)", background: on ? "rgba(255,255,255,.09)" : "transparent", color: on ? "var(--text-strong)" : "var(--text-muted)", transition: "all .15s" }}>
                {x}
                <span style={{ font: "500 10px/1 var(--font-mono)", color: on ? "var(--text-muted)" : "var(--text-faint)" }}>{n}</span>
              </button>
            );
          })}
        </div>
        <span style={{ flex: 1 }} />
        <div style={{ display: "flex", alignItems: "center", gap: 8, height: 34, padding: "0 12px", width: 240, borderRadius: 10, border: "1px solid rgba(255,255,255,.08)", background: "rgba(255,255,255,.02)" }}>
          <Icon name="Search" size={14} color="var(--text-subtle)" />
          <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Rechercher un service" aria-label="Rechercher un service" style={{ flex: 1, minWidth: 0, background: "transparent", border: 0, outline: "none", color: "var(--text-strong)", font: "400 13px/1 var(--font-sans)" }} />
        </div>
      </div>
      {showConnected && status && (
        <section style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <span style={{ display: "flex", alignItems: "center", gap: 8, font: "600 14px/1 var(--font-sans)", color: "var(--text-strong)" }}>
            Connectés<span style={{ ...lbl, fontFamily: "var(--font-mono)" }}>1</span>
          </span>
          <div style={{ borderRadius: 14, border: "1px solid rgba(255,255,255,.08)", background: "#0c0c0f", overflow: "hidden" }}>
            <div style={{ display: "grid", gridTemplateColumns: "minmax(0,1.5fr) minmax(0,1.2fr) 120px 200px", gap: 20, padding: "10px 16px 10px 18px", borderBottom: line, ...lbl, fontSize: 11 }}>
              <span>Service</span>
              <span>Compte</span>
              <span>Statut</span>
              <span />
            </div>
            <ConnRow status={status} onConfig={() => setConfig(true)} onDisconnect={() => setConfirm(true)} />
          </div>
        </section>
      )}
      {!status?.connected && statusQuery.isSuccess && <div style={{ padding: "12px 0", textAlign: "center", ...lbl }}>Aucun connecteur actif.</div>}
      {statusQuery.isError && <div role="alert" style={{ padding: "12px 0", textAlign: "center", ...lbl }}>État des connecteurs indisponible.</div>}
      {available.length > 0 && (
        <section style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill,minmax(260px,1fr))", gap: 12 }}>
          {available.map((c) => (
            <ConnectorCard key={c.id} c={c} connected={false} onConnect={c.id === "github" ? () => setConnecting(true) : undefined} />
          ))}
        </section>
      )}
      {connecting && <ConnectModal onClose={() => setConnecting(false)} />}
      {config && status && <ConfigDrawer status={status} onClose={() => setConfig(false)} onDisconnect={() => setConfirm(true)} />}
      {confirm && (
        <Modal
          title="Déconnecter GitHub ?"
          description="Ciphera oublie la GitHub App et ses clés ; les déploiements par push s'arrêtent. Supprimez aussi l'App dans les réglages GitHub. Vos projets restent en ligne."
          onClose={() => setConfirm(false)}
          width={440}
          footer={
            <>
              <Button onClick={() => setConfirm(false)}>Annuler</Button>
              <Button variant="inverse" icon="Unplug" onClick={() => void disconnect()}>
                Déconnecter
              </Button>
            </>
          }
        />
      )}
    </div>
  );
}
