import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { Button, Icon, IconButton, Modal, TextField } from "@/design-system";
import { fetchWebauthnStatus, ME_QUERY_KEY } from "@/features/auth/api";
import { SESSION_EXPIRED_MESSAGE } from "@/lib/api/session";

import { RecoveryCodesModal } from "./EnrollTotp";
import { deletePasskey, listPasskeys, PASSKEYS_QUERY_KEY, registerPasskey, type DeleteResult, type Passkey, type RegisterResult } from "./api";

const muted = { margin: 0, font: "400 12px/1.5 var(--font-sans)", color: "var(--text-subtle)" } as const;

const dateFormat = new Intl.DateTimeFormat("fr-FR", { day: "numeric", month: "short", year: "numeric" });
const day = (unix: number) => dateFormat.format(new Date(unix * 1000));

function registerMessage(result: Extract<RegisterResult, { ok: false }>): string {
  if (result.reason === "password") return "Mot de passe incorrect.";
  if (result.reason === "rate_limited") return "Trop de tentatives. Réessayez dans quelques minutes.";
  if (result.reason === "cancelled") return "Opération annulée ou délai dépassé. Réessayez.";
  if (result.reason === "exists") return "Cette clé est déjà enregistrée.";
  if (result.reason === "invalid") return "La clé n'a pas pu être vérifiée. Réessayez.";
  return "Enregistrement impossible pour le moment. Réessayez.";
}

function deleteMessage(result: Extract<DeleteResult, { ok: false }>): string {
  if (result.reason === "password") return "Mot de passe incorrect.";
  if (result.reason === "session_expired") return SESSION_EXPIRED_MESSAGE;
  if (result.reason === "required_by_org") return "Votre organisation impose la vérification en deux étapes : gardez au moins une méthode.";
  if (result.reason === "rate_limited") return "Trop de tentatives. Réessayez dans quelques minutes.";
  return "Suppression impossible pour le moment. Réessayez.";
}

function Alert({ children }: { children: string }) {
  return (
    <span role="alert" style={{ display: "flex", alignItems: "center", gap: 6, font: "400 12px/1.3 var(--font-sans)", color: "var(--text-negative)" }}>
      <Icon name="CircleX" size={13} />
      {children}
    </span>
  );
}

/**
 * Nom de la nouvelle cle, puis creation par le navigateur (ecran absent des
 * maquettes) ; codes de secours affiches une fois s'ils viennent d'etre emis.
 */
function AddKey({ onClose, onAdded, onDone }: { onClose: () => void; onAdded: () => void; onDone: () => void }) {
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [codes, setCodes] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const add = async () => {
    if (busy || !password) return;
    setBusy(true);
    setError("");
    const result = await registerPasskey(name.trim(), password);
    setBusy(false);
    // Mot de passe garde si la verification a reussi (annulation du navigateur, cle deja enregistree).
    if (result.ok || result.reason === "password" || result.reason === "rate_limited") setPassword("");
    if (!result.ok) return setError(registerMessage(result));
    onAdded();
    if (result.recoveryCodes.length > 0) setCodes(result.recoveryCodes);
    else onDone();
  };
  if (codes.length > 0) return <RecoveryCodesModal codes={codes} lost="votre clé de sécurité" onDone={onDone} />;
  return (
    <Modal
      title="Ajouter une clé de sécurité"
      description="Confirmez avec votre mot de passe. YubiKey, Touch ID, Windows Hello ou passkey de votre téléphone : votre navigateur vous demandera ensuite de la confirmer."
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Annuler</Button>
          <Button variant="inverse" disabled={busy || !password} aria-busy={busy || undefined} onClick={() => void add()}>
            Continuer
          </Button>
        </>
      }
    >
      <TextField label="Nom de la clé" value={name} placeholder="Clé de sécurité" onChange={(v) => setName(v.slice(0, 64))} />
      <TextField label="Mot de passe" type="password" value={password} onChange={setPassword} />
      {busy && (
        <span role="status" style={{ display: "flex", alignItems: "center", gap: 8, ...muted }}>
          <Icon name="Loader" size={14} />
          En attente de votre clé…
        </span>
      )}
      {error && <Alert>{error}</Alert>}
    </Modal>
  );
}

/** Suppression d'une cle, mot de passe redemande (ecran absent des maquettes). */
function RemoveKey({ passkey, onClose, onDone }: { passkey: Passkey; onClose: () => void; onDone: () => void }) {
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const remove = async () => {
    if (busy || !password) return;
    setBusy(true);
    setError("");
    const result = await deletePasskey(passkey.id, password);
    setBusy(false);
    setPassword("");
    if (result.ok) onDone();
    else setError(deleteMessage(result));
  };
  return (
    <Modal
      title={`Supprimer « ${passkey.name} »`}
      description="Confirmez avec votre mot de passe. Cette clé ne permettra plus de vous connecter."
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Annuler</Button>
          <Button variant="inverse" disabled={busy || !password} aria-busy={busy || undefined} onClick={() => void remove()}>
            Supprimer
          </Button>
        </>
      }
    >
      <TextField label="Mot de passe" type="password" value={password} onChange={setPassword} />
      {error && <Alert>{error}</Alert>}
    </Modal>
  );
}

/**
 * Carte « Clé de sécurité » de ProfileSecurity.jsx (passkeys) : nombre de
 * cles, ajout, et liste avec suppression. Le bouton d'ajout est masque quand
 * le panneau est ouvert par une adresse IP (WebAuthn indisponible).
 */
export function SecurityKeyCard() {
  const queryClient = useQueryClient();
  const keys = useQuery({ queryKey: PASSKEYS_QUERY_KEY, queryFn: listPasskeys });
  const available = useQuery({ queryKey: ["webauthn-status"], queryFn: fetchWebauthnStatus, staleTime: Infinity });
  const [adding, setAdding] = useState(false);
  const [removing, setRemoving] = useState<Passkey | null>(null);
  const list = keys.data ?? [];
  const enabled = list.length > 0;

  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: PASSKEYS_QUERY_KEY });
    void queryClient.invalidateQueries({ queryKey: ME_QUERY_KEY, exact: true });
  };

  return (
    <div style={{ borderRadius: 16, border: "1px solid var(--border-default)", background: "var(--surface-app)", padding: 18, display: "flex", flexDirection: "column", gap: 16 }}>
      <div style={{ display: "flex", alignItems: "flex-start", justifyContent: "space-between" }}>
        <span style={{ width: 40, height: 40, borderRadius: 11, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", background: "linear-gradient(180deg,#232327,#141417)", border: "1px solid rgba(255,255,255,.08)", boxShadow: "inset 0 1px 0 rgba(255,255,255,.07), 0 1px 2px rgba(0,0,0,.4)" }}>
          <Icon name="FingerprintPattern" size={17} color="#ededed" />
        </span>
        <span style={{ display: "inline-flex", alignItems: "center", gap: 6, height: 22, padding: "0 9px", borderRadius: 99, background: enabled ? "rgba(34,192,138,.12)" : "rgba(229,180,84,.12)", color: enabled ? "var(--green-500)" : "#e5b454", font: "500 11px/1 var(--font-sans)", whiteSpace: "nowrap" }}>
          <span style={{ width: 6, height: 6, borderRadius: 99, background: "currentColor" }} />
          {enabled ? "Active" : "Inactive"}
        </span>
      </div>
      <span style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        <span style={{ font: "500 14px/1.2 var(--font-sans)", color: "var(--text-strong)" }}>Clé de sécurité</span>
        <span style={{ font: "400 12px/1.35 var(--font-sans)", color: "var(--text-subtle)" }}>YubiKey, Touch ID · {enabled ? `${list.length} clé${list.length > 1 ? "s" : ""}` : "aucune"}</span>
      </span>
      {enabled && (
        <ul aria-label="Clés enregistrées" style={{ listStyle: "none", margin: 0, padding: 0, display: "flex", flexDirection: "column", gap: 8 }}>
          {list.map((k) => (
            <li key={k.id} style={{ display: "flex", alignItems: "center", gap: 10, padding: "8px 8px 8px 12px", borderRadius: 10, background: "var(--surface-raised)" }}>
              <span style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 4 }}>
                <span style={{ font: "500 13px/1.2 var(--font-sans)", color: "var(--text-strong)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{k.name}</span>
                <span style={muted}>
                  Ajoutée le {day(k.created_at)}
                  {k.last_used_at != null && ` · utilisée le ${day(k.last_used_at)}`}
                </span>
              </span>
              <IconButton icon="Trash" label={`Supprimer ${k.name}`} onClick={() => setRemoving(k)} />
            </li>
          ))}
        </ul>
      )}
      {available.data?.available === false && (
        <p style={muted}>
          {available.data.publicUrl
            ? `Les clés de sécurité ne fonctionnent qu'à l'adresse publique du panneau : ouvrez ${available.data.publicUrl} pour en ajouter une.`
            : "Ouvrez le panneau par son nom de domaine (et non par son adresse IP) pour ajouter une clé."}
        </p>
      )}
      {available.data?.available && (
        <div style={{ marginTop: "auto", display: "flex", justifyContent: "flex-end" }}>
          <Button size="sm" onClick={() => setAdding(true)}>
            Ajouter
          </Button>
        </div>
      )}
      {adding && (
        <AddKey onClose={() => setAdding(false)} onAdded={refresh} onDone={() => setAdding(false)} />
      )}
      {removing && (
        <RemoveKey
          passkey={removing}
          onClose={() => setRemoving(null)}
          onDone={() => {
            setRemoving(null);
            refresh();
          }}
        />
      )}
    </div>
  );
}
