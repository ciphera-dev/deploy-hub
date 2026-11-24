import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";

import { Button, Icon, Modal, TextField, useToast, type IconName } from "@/design-system";
import { fetchMe, ME_QUERY_KEY, type Me } from "@/features/auth/api";
import { ROLE_NAMES } from "@/features/auth/InviteAccept";

import { updateProfile, type ProfileUpdate } from "./api";
import { ProfileAvatar } from "./Avatar";
import { ProfileNotif } from "./ProfileNotif";
import { ProfileSecurity } from "./ProfileSecurity";
import { Alert, card, Card, hair } from "./ui";

// ProfilePage.jsx. Retires de la maquette : le bloc « Sécurité » redondant
// (remplace par ProfileSecurity), l'onglet « Activité du compte », les donnees
// de facturation (adresse, telephone, entreprise, devise), l'e-mail de
// recuperation et les compteurs du bandeau (aucune donnee en v0.1).

type Tab = "info" | "security" | "notif";
const SUB: [Tab, IconName, string][] = [
  ["info", "UserRound", "Informations du compte"],
  ["security", "ShieldCheck", "Sécurité"],
  ["notif", "Bell", "Notifications"],
];

const LANGUAGES: [string, string][] = [
  ["fr", "Français"],
  ["en", "English"],
];

/** Fuseaux IANA proposes a la saisie (si le navigateur sait les lister). */
const TIMEZONES: string[] = (() => {
  try {
    return (Intl as { supportedValuesOf?: (key: string) => string[] }).supportedValuesOf?.("timeZone") ?? [];
  } catch {
    return [];
  }
})();

function RoundButton({ icon, label, onClick, primary }: { icon: IconName; label: string; onClick: () => void; primary?: boolean }) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      style={{ width: 32, height: 32, borderRadius: 99, cursor: "pointer", display: "flex", alignItems: "center", justifyContent: "center", border: primary ? 0 : "1px solid var(--border-strong)", background: primary ? "#ededed" : "transparent", color: primary ? "#0b0b0b" : "var(--text-body)" }}
    >
      <Icon name={icon} size={14} />
    </button>
  );
}

interface RowProps {
  label: string;
  value: string;
  /** renvoie `false` pour rester en edition (valeur refusee) */
  onSave?: (v: string) => Promise<boolean>;
  type?: string;
  list?: string;
  render?: ReactNode;
  last?: boolean;
}

/** Ligne de ProfilePage.jsx : libelle, valeur, crayon pour modifier sur place. */
function Row({ label, value, onSave, type = "text", list, render, last }: RowProps) {
  const [editing, setEditing] = useState(false);
  const [v, setV] = useState(value);
  const edit = () => {
    setV(value);
    setEditing(true);
  };
  const cancel = () => {
    setV(value);
    setEditing(false);
  };
  const save = async () => {
    if (!onSave) return;
    if (v.trim() === value) return setEditing(false);
    if (await onSave(v)) setEditing(false);
  };
  return (
    <div style={{ display: "grid", gridTemplateColumns: "220px minmax(0,1fr) 76px", gap: 16, alignItems: "center", minHeight: 58, padding: "0 16px 0 20px", borderBottom: last ? "none" : hair }}>
      <span style={{ font: "400 13px/1 var(--font-sans)", color: "var(--text-muted)", whiteSpace: "nowrap" }}>{label}</span>
      {editing ? (
        <input
          autoFocus
          aria-label={label}
          type={type}
          list={list}
          value={v}
          onChange={(e) => setV(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") void save();
            if (e.key === "Escape") cancel();
          }}
          style={{ height: 36, padding: "0 12px", borderRadius: 9, border: "1px solid var(--gray-600)", background: "var(--gray-0)", color: "var(--text-strong)", font: "400 13px/1 var(--font-sans)", outline: "none", minWidth: 0 }}
        />
      ) : (
        (render ?? <span style={{ font: "400 13px/1.3 var(--font-sans)", color: value ? "var(--text-strong)" : "var(--text-faint)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{value || "—"}</span>)
      )}
      <div style={{ display: "flex", gap: 8, justifyContent: "flex-end", minWidth: 32 }}>
        {editing ? (
          <>
            <RoundButton icon="X" label="Annuler" onClick={cancel} />
            <RoundButton icon="Check" label="Enregistrer" primary onClick={() => void save()} />
          </>
        ) : (
          onSave && <RoundButton icon="Pencil" label={`Modifier ${label.toLowerCase()}`} onClick={edit} />
        )}
      </div>
    </div>
  );
}

/** Mot de passe actuel redemande pour changer d'adresse e-mail (ecran absent des maquettes). */
function ConfirmEmail({ email, onClose, onDone }: { email: string; onClose: () => void; onDone: () => void }) {
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const confirm = async () => {
    if (busy || !password) return;
    setBusy(true);
    setError("");
    const result = await updateProfile({ email, current_password: password });
    setBusy(false);
    setPassword("");
    if (result.ok) onDone();
    else setError(result.message);
  };
  return (
    <Modal
      title="Changer d'adresse e-mail"
      description={`Confirmez avec votre mot de passe. Un avis sera envoyé à votre adresse actuelle ; vous vous connecterez ensuite avec ${email}.`}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Annuler</Button>
          <Button variant="inverse" disabled={busy || !password} aria-busy={busy || undefined} onClick={() => void confirm()}>
            Confirmer
          </Button>
        </>
      }
    >
      <TextField label="Mot de passe actuel" type="password" autoComplete="current-password" value={password} onChange={setPassword} />
      {error && <Alert>{error}</Alert>}
    </Modal>
  );
}

function Info({ me }: { me: Me }) {
  const queryClient = useQueryClient();
  const { flash } = useToast();
  const [error, setError] = useState("");
  const [pendingEmail, setPendingEmail] = useState<string | null>(null);
  const role = ROLE_NAMES[me.role] ?? me.role;
  const name = me.name || me.email;

  const refresh = () => void queryClient.invalidateQueries({ queryKey: ME_QUERY_KEY, exact: true });
  const save = async (body: ProfileUpdate) => {
    setError("");
    const result = await updateProfile(body);
    if (!result.ok) {
      setError(result.message);
      return false;
    }
    refresh();
    flash("Modifié");
    return true;
  };

  return (
    <>
      <div style={{ ...card, position: "relative" }}>
        <div style={{ position: "relative", height: 132, overflow: "hidden", background: "radial-gradient(60% 140% at 0% 0%,#4c3a9e 0%,transparent 60%),radial-gradient(50% 120% at 100% 0%,#1e4e8c 0%,transparent 65%),radial-gradient(40% 90% at 60% 100%,#6b2a6e 0%,transparent 70%),#17142a" }}>
          <div style={{ position: "absolute", inset: 0, backgroundImage: "linear-gradient(rgba(255,255,255,.045) 1px,transparent 1px),linear-gradient(90deg,rgba(255,255,255,.045) 1px,transparent 1px)", backgroundSize: "28px 28px", maskImage: "linear-gradient(180deg,rgba(0,0,0,.9),transparent)", WebkitMaskImage: "linear-gradient(180deg,rgba(0,0,0,.9),transparent)" }} />
          <div style={{ position: "absolute", inset: 0, background: "linear-gradient(180deg,transparent 55%,var(--surface-app))" }} />
        </div>
        <div style={{ display: "flex", alignItems: "flex-end", gap: 18, padding: "0 22px 20px", marginTop: -46, position: "relative" }}>
          <span style={{ position: "relative", padding: 4, borderRadius: 99, background: "var(--surface-app)" }}>
            <ProfileAvatar name={name} src={me.avatar_url} size={88} editable />
            <span style={{ position: "absolute", right: 2, bottom: 2, width: 28, height: 28, borderRadius: 99, background: "#ededed", color: "#0b0b0b", display: "flex", alignItems: "center", justifyContent: "center", boxShadow: "0 0 0 3px var(--surface-app)", pointerEvents: "none" }}>
              <Icon name="Camera" size={13} />
            </span>
          </span>
          <div style={{ display: "flex", flexDirection: "column", gap: 9, flex: 1, minWidth: 0, paddingBottom: 6 }}>
            <span style={{ display: "flex", alignItems: "center", gap: 10 }}>
              <span style={{ font: "600 22px/1.1 var(--font-sans)", color: "var(--text-strong)", letterSpacing: "-0.01em" }}>{name}</span>
              <span style={{ display: "flex", alignItems: "center", gap: 5, height: 22, padding: "0 9px", borderRadius: 99, background: "rgba(229,180,84,.12)", border: "1px solid rgba(229,180,84,.3)", color: "#e5b454", font: "500 11px/1 var(--font-sans)" }}>
                {me.role === "owner" && <Icon name="Crown" size={11} />}
                {role}
              </span>
            </span>
            <span style={{ display: "flex", alignItems: "center", gap: 6, font: "400 13px/1 var(--font-sans)", color: "var(--text-subtle)" }}>
              <Icon name="Mail" size={13} />
              {me.email}
            </span>
          </div>
        </div>
      </div>
      {error && <Alert>{error}</Alert>}
      <Card icon="IdCard" title="Données personnelles">
        <Row label="Nom" value={me.name} onSave={(v) => save({ name: v })} last />
      </Card>
      <Card icon="UserRoundCog" title="Paramètres du compte" desc="Connexion et préférences">
        <Row
          label="Adresse e-mail"
          value={me.email}
          type="email"
          onSave={async (v) => {
            setError("");
            setPendingEmail(v.trim());
            return true;
          }}
        />
        <Row
          label="Langue"
          value={me.language}
          render={
            <select
              aria-label="Langue"
              value={me.language}
              onChange={(e) => void save({ language: e.target.value })}
              style={{ height: 34, padding: "0 10px", borderRadius: 9, border: "1px solid var(--border-strong)", background: "var(--surface-control)", color: "var(--text-strong)", colorScheme: "dark", width: 200, font: "400 13px/1 var(--font-sans)", outline: "none" }}
            >
              {LANGUAGES.map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          }
        />
        <Row label="Fuseau horaire" value={me.timezone} list="profile-timezones" onSave={(v) => save({ timezone: v.trim() })} last />
        <datalist id="profile-timezones">
          {TIMEZONES.map((tz) => (
            <option key={tz} value={tz} />
          ))}
        </datalist>
      </Card>
      {pendingEmail !== null && (
        <ConfirmEmail
          email={pendingEmail}
          onClose={() => setPendingEmail(null)}
          onDone={() => {
            setPendingEmail(null);
            refresh();
            flash("Adresse e-mail modifiée");
          }}
        />
      )}
    </>
  );
}

/** Page Profil : navigation laterale (informations, securite, notifications). */
export function ProfilePage() {
  const me = useQuery({ queryKey: ME_QUERY_KEY, queryFn: fetchMe });
  const [tab, setTab] = useState<Tab>("info");
  const account = typeof me.data === "object" && me.data !== null ? me.data : null;
  return (
    <div style={{ display: "grid", gridTemplateColumns: "240px minmax(0,1fr)", minHeight: "100%" }}>
      <nav aria-label="Profil" style={{ padding: "20px 12px", borderRight: hair, display: "flex", flexDirection: "column", gap: 4 }}>
        <span style={{ padding: "0 10px 12px", font: "500 12px/1 var(--font-sans)", color: "var(--text-subtle)" }}>Profil</span>
        {SUB.map(([key, icon, label]) => {
          const on = key === tab;
          return (
            <button
              key={key}
              type="button"
              aria-current={on ? "page" : undefined}
              onClick={() => setTab(key)}
              style={{ display: "flex", alignItems: "center", gap: 11, height: 42, padding: "0 10px", borderRadius: 11, border: 0, outline: "none", cursor: "pointer", textAlign: "left", background: on ? "var(--surface-selected)" : "transparent", color: on ? "var(--text-strong)" : "var(--text-muted)", font: "500 13px/1 var(--font-sans)", transition: "background .15s" }}
            >
              <span style={{ width: 26, height: 26, borderRadius: 8, display: "flex", alignItems: "center", justifyContent: "center", background: on ? "linear-gradient(180deg,#2a2a2f,#18181b)" : "transparent", border: "1px solid " + (on ? "rgba(255,255,255,.08)" : "transparent"), boxShadow: on ? "inset 0 1px 0 rgba(255,255,255,.07)" : "none" }}>
                <Icon name={icon} size={15} color={on ? "#ededed" : "var(--text-muted)"} />
              </span>
              <span style={{ flex: 1, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{label}</span>
            </button>
          );
        })}
      </nav>
      <div style={{ padding: "28px 40px 48px", display: "flex", flexDirection: "column", gap: 18, maxWidth: 960, width: "100%", margin: "0 auto", boxSizing: "border-box" }}>
        {tab === "info" && account && <Info me={account} />}
        {tab === "security" && <ProfileSecurity />}
        {tab === "notif" && <ProfileNotif email={account?.email ?? ""} />}
      </div>
    </div>
  );
}
