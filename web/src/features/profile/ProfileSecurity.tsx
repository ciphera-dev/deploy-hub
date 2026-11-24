import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { Button, Icon, Modal, TextField, useToast, type IconName } from "@/design-system";
import { fetchMe, ME_QUERY_KEY } from "@/features/auth/api";

import { changePassword, listSessions, revokeSession, SESSIONS_QUERY_KEY, type SessionInfo } from "./api";
import { SecurityKeyCard } from "./security/SecurityKeyCard";
import { TwoFactorCard } from "./security/TwoFactorCard";
import { Alert, card, hair, Pill, Tile, Title } from "./ui";

// ProfileSecurity.jsx. Retires : l'e-mail de recuperation (non pris en charge)
// et « Supprimer mon compte » (pas en v0.1). La localisation des sessions
// n'est pas connue : seule l'adresse IP est affichee.

function Ring({ v }: { v: number }) {
  const R = 38;
  const C = 2 * Math.PI * R;
  return (
    <div style={{ position: "relative", width: 96, height: 96, flex: "none" }}>
      <svg width="96" height="96" aria-hidden="true" style={{ transform: "rotate(-90deg)" }}>
        <defs>
          <linearGradient id="pf-ring" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stopColor="#22c08a" />
            <stop offset="1" stopColor="#38bdf8" />
          </linearGradient>
        </defs>
        <circle cx="48" cy="48" r={R} fill="none" stroke="rgba(255,255,255,.07)" strokeWidth="8" />
        <circle cx="48" cy="48" r={R} fill="none" stroke="url(#pf-ring)" strokeWidth="8" strokeLinecap="round" strokeDasharray={C} strokeDashoffset={C * (1 - v / 100)} style={{ transition: "stroke-dashoffset .6s cubic-bezier(.2,.8,.2,1)" }} />
      </svg>
      <span style={{ position: "absolute", inset: 0, display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 4 }}>
        <span style={{ font: "600 22px/1 var(--font-sans)", color: "var(--text-strong)" }}>{v}</span>
        <span style={{ font: "400 10px/1 var(--font-sans)", color: "var(--text-subtle)" }}>sur 100</span>
      </span>
    </div>
  );
}

/** Changement du mot de passe (ecran absent des maquettes : bouton « Modifier » de la carte). */
function ChangePasswordModal({ onClose, onDone }: { onClose: () => void; onDone: () => void }) {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const submit = async () => {
    if (busy || !current || !next) return;
    if (next !== confirm) return setError("Les deux mots de passe ne correspondent pas.");
    if ([...next].length < 12) return setError("Le mot de passe doit contenir au moins 12 caractères.");
    setBusy(true);
    setError("");
    const result = await changePassword(current, next);
    setBusy(false);
    if (result.ok) return onDone();
    setCurrent("");
    setError(result.message);
  };
  return (
    <Modal
      title="Modifier le mot de passe"
      description="Vos autres sessions seront déconnectées ; celle-ci reste ouverte."
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Annuler</Button>
          <Button variant="inverse" disabled={busy || !current || !next} aria-busy={busy || undefined} onClick={() => void submit()}>
            Enregistrer
          </Button>
        </>
      }
    >
      <TextField label="Mot de passe actuel" type="password" autoComplete="current-password" value={current} onChange={setCurrent} />
      <TextField label="Nouveau mot de passe" type="password" autoComplete="new-password" hint="12 caractères minimum." value={next} onChange={setNext} />
      <TextField label="Confirmer le nouveau mot de passe" type="password" autoComplete="new-password" value={confirm} onChange={setConfirm} />
      {error && <Alert>{error}</Alert>}
    </Modal>
  );
}

/** Carte « Mot de passe » des methodes de connexion. */
function PasswordCard({ onChanged }: { onChanged: () => void }) {
  const [open, setOpen] = useState(false);
  return (
    <div style={{ ...card, padding: 18, display: "flex", flexDirection: "column", gap: 16 }}>
      <div style={{ display: "flex", alignItems: "flex-start", justifyContent: "space-between" }}>
        <Tile icon="KeyRound" size={40} />
        <Pill ok>Active</Pill>
      </div>
      <span style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        <span style={{ font: "500 14px/1.2 var(--font-sans)", color: "var(--text-strong)" }}>Mot de passe</span>
        <span style={{ font: "400 12px/1.35 var(--font-sans)", color: "var(--text-subtle)" }}>12 caractères minimum</span>
      </span>
      <div style={{ marginTop: "auto", display: "flex", justifyContent: "flex-end" }}>
        <Button size="sm" onClick={() => setOpen(true)}>
          Modifier
        </Button>
      </div>
      {open && (
        <ChangePasswordModal
          onClose={() => setOpen(false)}
          onDone={() => {
            setOpen(false);
            onChanged();
          }}
        />
      )}
    </div>
  );
}

/** Appareil et navigateur lus dans le User-Agent (approximation pour l'affichage). */
export function describeAgent(ua: string | null | undefined): { icon: IconName; device: string; browser: string } {
  const s = ua ?? "";
  const icon: IconName = /iPhone|Android.*Mobile|Mobile/.test(s) ? "Smartphone" : /Macintosh|Mac OS X|CrOS|Linux|X11/.test(s) && !/Android/.test(s) ? "Laptop" : "Monitor";
  const device = /iPhone/.test(s) ? "iPhone" : /iPad/.test(s) ? "iPad" : /Android/.test(s) ? "Android" : /Macintosh|Mac OS X/.test(s) ? "Mac" : /Windows/.test(s) ? "PC Windows" : /CrOS/.test(s) ? "Chromebook" : /Linux|X11/.test(s) ? "Linux" : "Appareil inconnu";
  const version = (re: RegExp) => s.match(re)?.[1]?.split(".")[0];
  const browser = /Edg\//.test(s)
    ? `Edge ${version(/Edg\/([\d.]+)/) ?? ""}`
    : /Firefox\//.test(s)
      ? `Firefox ${version(/Firefox\/([\d.]+)/) ?? ""}`
      : /Chrome\//.test(s)
        ? `Chrome ${version(/Chrome\/([\d.]+)/) ?? ""}`
        : /Safari\//.test(s)
          ? `Safari ${version(/Version\/([\d.]+)/) ?? ""}`
          : "Navigateur inconnu";
  return { icon, device, browser: browser.trim() };
}

/** « il y a 2 h » a partir d'un horodatage Unix (secondes). */
export function since(unix: number, now = Date.now()): string {
  const minutes = Math.max(0, Math.floor((now - unix * 1000) / 60000));
  if (minutes < 2) return "Active maintenant";
  if (minutes < 60) return `il y a ${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `il y a ${hours} h`;
  return `il y a ${Math.floor(hours / 24)} j`;
}

function Sessions() {
  const queryClient = useQueryClient();
  const { flash } = useToast();
  const sessions = useQuery({ queryKey: SESSIONS_QUERY_KEY, queryFn: listSessions });
  const [error, setError] = useState("");
  const list = sessions.data ?? [];
  const others = list.filter((x) => !x.current);
  const refresh = () => void queryClient.invalidateQueries({ queryKey: SESSIONS_QUERY_KEY });

  const revoke = async (session: SessionInfo) => {
    setError("");
    const ok = await revokeSession(session.id);
    refresh();
    if (ok) flash(`${describeAgent(session.user_agent).device} déconnecté`);
    else setError("Déconnexion impossible pour le moment. Réessayez.");
  };
  const revokeOthers = async () => {
    setError("");
    let ok = true;
    for (const session of others) ok = (await revokeSession(session.id)) && ok;
    refresh();
    if (ok) flash("Autres sessions déconnectées");
    else setError("Certaines sessions n'ont pas pu être déconnectées. Réessayez.");
  };

  return (
    <>
      <Title
        t="Sessions actives"
        d={`${list.length} appareil${list.length > 1 ? "s" : ""} connecté${list.length > 1 ? "s" : ""}`}
        right={
          others.length > 0 && (
            <Button size="sm" icon="LogOut" onClick={() => void revokeOthers()}>
              Déconnecter les autres
            </Button>
          )
        }
      />
      {error && <Alert>{error}</Alert>}
      <ul aria-label="Sessions actives" style={{ ...card, listStyle: "none", margin: 0, padding: 0 }}>
        {list.map((x, i) => {
          const agent = describeAgent(x.user_agent);
          return (
            <li key={x.id} style={{ display: "grid", gridTemplateColumns: "44px minmax(0,1.3fr) minmax(0,1fr) 130px 40px", gap: 14, alignItems: "center", padding: "14px 16px 14px 18px", borderTop: i ? hair : "none" }}>
              <span style={{ position: "relative", width: 44, height: 44, borderRadius: 12, display: "flex", alignItems: "center", justifyContent: "center", background: "var(--surface-raised)", border: "1px solid var(--border-default)" }}>
                <Icon name={agent.icon} size={18} color="var(--text-body)" />
                {x.current && <span style={{ position: "absolute", right: -2, top: -2, width: 10, height: 10, borderRadius: 99, background: "var(--green-500)", boxShadow: "0 0 0 2px var(--surface-app)" }} />}
              </span>
              <span style={{ display: "flex", flexDirection: "column", gap: 6, minWidth: 0 }}>
                <span style={{ display: "flex", alignItems: "center", gap: 8, font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>
                  {agent.device}
                  {x.current && <Pill ok>Cet appareil</Pill>}
                </span>
                <span style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" }}>{agent.browser}</span>
              </span>
              <span style={{ font: "400 11px/1 var(--font-mono)", color: "var(--text-subtle)", overflow: "hidden", textOverflow: "ellipsis" }}>{x.ip ?? "—"}</span>
              <span style={{ font: "400 12px/1 var(--font-sans)", color: x.current ? "var(--green-500)" : "var(--text-muted)" }}>{x.current ? "Active maintenant" : since(x.last_seen_at)}</span>
              {x.current ? (
                <span />
              ) : (
                <button
                  type="button"
                  title="Déconnecter"
                  aria-label={`Déconnecter ${agent.device} (${agent.browser})`}
                  onClick={() => void revoke(x)}
                  style={{ width: 34, height: 34, borderRadius: 10, border: "1px solid rgba(239,68,68,.25)", background: "rgba(239,68,68,.06)", color: "#f87171", cursor: "pointer", display: "flex", alignItems: "center", justifyContent: "center" }}
                >
                  <Icon name="LogOut" size={14} />
                </button>
              )}
            </li>
          );
        })}
      </ul>
    </>
  );
}

/** Onglet Sécurité du Profil : score, methodes de connexion, sessions. */
export function ProfileSecurity() {
  const queryClient = useQueryClient();
  const { flash } = useToast();
  const me = useQuery({ queryKey: ME_QUERY_KEY, queryFn: fetchMe });
  const account = typeof me.data === "object" && me.data !== null ? me.data : null;
  const checks: [string, boolean][] = [
    ["Mot de passe robuste", true],
    ["Application d'authentification", account?.totp_enabled ?? false],
    ["Clé de sécurité", account?.webauthn_enabled ?? false],
  ];
  const score = 40 + checks.filter((c) => c[1]).length * 20;

  return (
    <>
      <div style={{ ...card, display: "flex", flexWrap: "wrap", alignItems: "center", gap: 24, padding: "22px 24px" }}>
        <Ring v={score} />
        <div style={{ display: "flex", flexDirection: "column", gap: 8, flex: "1 1 180px", minWidth: 0 }}>
          <span style={{ font: "600 17px/1.2 var(--font-sans)", color: "var(--text-strong)" }}>{score >= 90 ? "Compte très bien protégé" : score >= 70 ? "Bonne protection" : "Protection à renforcer"}</span>
          <span style={{ font: "400 12px/1.45 var(--font-sans)", color: "var(--text-subtle)" }}>Activez toutes les méthodes pour atteindre 100.</span>
        </div>
        <ul style={{ flex: "1 1 320px", minWidth: 0, display: "grid", gridTemplateColumns: "repeat(auto-fill,minmax(200px,1fr))", gap: 8, listStyle: "none", margin: 0, padding: 0 }}>
          {checks.map(([t, ok]) => (
            <li key={t} style={{ display: "flex", alignItems: "center", gap: 10, height: 40, padding: "0 12px", borderRadius: 10, background: "var(--surface-raised)" }}>
              <span style={{ width: 18, height: 18, borderRadius: 99, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", background: ok ? "var(--green-500)" : "transparent", border: ok ? "none" : "1.5px dashed var(--gray-500)" }}>{ok && <Icon name="Check" size={11} color="#0b0b0b" />}</span>
              <span style={{ flex: 1, font: "400 12px/1.2 var(--font-sans)", color: ok ? "var(--text-body)" : "var(--text-muted)" }}>{t}</span>
            </li>
          ))}
        </ul>
      </div>
      <Title t="Méthodes de connexion" d="Combinez-les pour sécuriser l'accès à la plateforme." />
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill,minmax(220px,1fr))", gap: 12, alignItems: "stretch" }}>
        <PasswordCard
          onChanged={() => {
            void queryClient.invalidateQueries({ queryKey: SESSIONS_QUERY_KEY });
            flash("Mot de passe modifié");
          }}
        />
        <TwoFactorCard />
        <SecurityKeyCard />
      </div>
      <Sessions />
    </>
  );
}
