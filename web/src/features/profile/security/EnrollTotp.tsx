import { useState } from "react";

import { Button, CodeBlock, CopyButton, Icon, Modal, TextField } from "@/design-system";

import { confirmTotp, manualKey, startTotp, type TotpStart } from "./api";

export interface EnrollTotpProps {
  /** Enrolement impose par l'organisation : ni fermeture ni annulation. */
  forced?: boolean;
  /** Abandon avant activation. */
  onClose: () => void;
  /** TOTP activee et codes de secours vus. */
  onDone: () => void;
  /** Sortie de l'enrolement impose : « Se déconnecter ». */
  onSignOut?: () => void;
  /** Échec de « Se déconnecter » (enrolement impose), affiche sous l'avertissement. */
  signOutError?: string;
}

type Stage = { kind: "password" } | { kind: "loading" } | { kind: "failed" } | { kind: "scan"; setup: TotpStart } | { kind: "codes"; codes: string[] };

const FILE_NAME = "ciphera-codes-de-secours.txt";

/** Image du QR code : le SVG du serveur passe par une URL data:, jamais par le DOM. */
function qrSource(svg: string): string {
  return `data:image/svg+xml;base64,${btoa(svg)}`;
}

/** Fichier texte des codes, genere dans le navigateur (aucune requete). */
function downloadCodes(codes: string[]) {
  const text = [`Codes de secours Ciphera (${window.location.host})`, "Chaque code ne sert qu'une fois.", "", ...codes, ""].join("\n");
  const url = URL.createObjectURL(new Blob([text], { type: "text/plain;charset=utf-8" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = FILE_NAME;
  a.click();
  URL.revokeObjectURL(url);
}

/** Codes de secours affiches une seule fois (bloc de code, telechargement .txt). */
export function RecoveryCodesModal({ codes, lost, onDone }: { codes: string[]; lost: string; onDone: () => void }) {
  return (
    <Modal
      title="Codes de secours"
      description={`Chacun permet une seule connexion si vous n'avez plus accès à ${lost}. Ils ne seront plus affichés : conservez-les en lieu sûr.`}
      footer={
        <>
          <Button icon="Download" onClick={() => downloadCodes(codes)}>
            Télécharger (.txt)
          </Button>
          <Button variant="inverse" onClick={onDone}>
            J'ai enregistré mes codes
          </Button>
        </>
      }
    >
      <CodeBlock code={codes.join("\n")} />
    </Modal>
  );
}

const muted = { margin: 0, font: "400 12px/1.5 var(--font-sans)", color: "var(--text-subtle)" } as const;

/**
 * Activation de la vérification en deux étapes (écran absent des maquettes) :
 * QR code et clé manuelle, vérification d'un code à 6 chiffres, puis les 10
 * codes de secours affichés une seule fois.
 */
export function EnrollTotp({ forced = false, onClose, onDone, onSignOut, signOutError }: EnrollTotpProps) {
  const [stage, setStage] = useState<Stage>({ kind: "password" });
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  // Mot de passe actuel toujours redemande avant d'ajouter un second facteur,
  // y compris en enrolement impose (une session ouverte avant l'obligation
  // peut y avoir ete basculee).
  const start = async () => {
    if (busy) return;
    setBusy(true);
    setError("");
    setStage({ kind: "loading" });
    const result = await startTotp(password);
    setBusy(false);
    if (result.ok) {
      setPassword("");
      setStage({ kind: "scan", setup: result.setup });
    } else if (result.reason === "password" || result.reason === "rate_limited") {
      setStage({ kind: "password" });
      setError(result.reason === "password" ? "Mot de passe incorrect." : "Trop de tentatives. Réessayez dans quelques minutes.");
    } else setStage({ kind: "failed" });
  };

  const valid = /^\d{6}$/.test(code);
  const verify = async () => {
    if (!valid || busy) return;
    setBusy(true);
    setError("");
    const result = await confirmTotp(code);
    setBusy(false);
    if (result.ok) {
      setCode("");
      setStage({ kind: "codes", codes: result.recoveryCodes });
    } else setError(result.reason === "invalid" ? "Code incorrect, réessayez." : "Activation impossible pour le moment. Réessayez.");
  };

  if (stage.kind === "codes") return <RecoveryCodesModal codes={stage.codes} lost="votre application" onDone={onDone} />;

  return (
    <Modal
      title="Activer la vérification en deux étapes"
      description={stage.kind === "password" ? "Confirmez avec votre mot de passe pour ajouter un second facteur." : "Scannez le QR code avec votre application d'authentification (Google Authenticator, 1Password…), puis saisissez le code affiché."}
      onClose={forced ? undefined : onClose}
      footer={
        <>
          {!forced && <Button onClick={onClose}>Annuler</Button>}
          {forced && onSignOut && (
            <Button variant="ghost" icon="LogOut" onClick={onSignOut} style={{ marginRight: "auto" }}>
              Se déconnecter
            </Button>
          )}
          {stage.kind === "password" ? (
            <Button variant="inverse" disabled={!password || busy} aria-busy={busy || undefined} onClick={() => void start()}>
              Continuer
            </Button>
          ) : (
          <Button variant="inverse" disabled={!valid || busy || stage.kind !== "scan"} aria-busy={busy || undefined} onClick={() => void verify()}>
            Vérifier
          </Button>
          )}
        </>
      }
    >
      {forced && (
        <p role="status" style={{ ...muted, color: "var(--text-body)" }}>
          Votre organisation impose la vérification en deux étapes : activez-la pour accéder au tableau de bord.
        </p>
      )}
      {forced && signOutError && (
        <span role="alert" style={{ display: "flex", alignItems: "center", gap: 6, font: "400 12px/1.3 var(--font-sans)", color: "var(--text-negative)" }}>
          <Icon name="CircleX" size={13} />
          {signOutError}
        </span>
      )}
      {stage.kind === "password" && (
        <>
          <TextField label="Mot de passe" type="password" value={password} onChange={(v) => {
            setPassword(v);
            setError("");
          }} />
          {error && (
            <span role="alert" style={{ display: "flex", alignItems: "center", gap: 6, font: "400 12px/1.3 var(--font-sans)", color: "var(--text-negative)" }}>
              <Icon name="CircleX" size={13} />
              {error}
            </span>
          )}
        </>
      )}
      {stage.kind === "loading" && (
        <span role="status" style={{ display: "flex", alignItems: "center", gap: 8, ...muted }}>
          <Icon name="Loader" size={14} />
          Préparation…
        </span>
      )}
      {stage.kind === "failed" && (
        <div role="alert" style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12, ...muted, color: "var(--text-negative)" }}>
          Impossible de démarrer l'activation.
          <Button size="sm" onClick={() => void start()}>
            Réessayer
          </Button>
        </div>
      )}
      {stage.kind === "scan" && (
        <>
          <div style={{ display: "flex", gap: 20, alignItems: "center", flexWrap: "wrap" }}>
            <img src={qrSource(stage.setup.qr_svg)} alt="QR code à scanner avec l'application d'authentification" width={168} height={168} style={{ flex: "none", borderRadius: "var(--radius-lg)", background: "#fff", padding: 6 }} />
            <div style={{ flex: "1 1 200px", minWidth: 0, display: "flex", flexDirection: "column", gap: 8 }}>
              <span style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-muted)" }}>Ou saisissez cette clé dans l'application</span>
              <span style={{ display: "flex", alignItems: "center", gap: 4, padding: "6px 6px 6px 12px", borderRadius: "var(--radius-md)", background: "var(--gray-0)", border: "1px solid var(--border-default)" }}>
                <code style={{ flex: 1, minWidth: 0, font: "500 12px/1.5 var(--font-mono)", color: "var(--text-strong)", wordBreak: "break-all" }}>{manualKey(stage.setup.otpauth_uri)}</code>
                <CopyButton value={manualKey(stage.setup.otpauth_uri).replace(/ /g, "")} label="Copier la clé" size={26} />
              </span>
            </div>
          </div>
          <TextField label="Code à 6 chiffres" mono value={code} placeholder="000000" onChange={(v) => {
            setCode(v.replace(/\s/g, "").slice(0, 6));
            setError("");
          }} />
          {error && (
            <span role="alert" style={{ display: "flex", alignItems: "center", gap: 6, font: "400 12px/1.3 var(--font-sans)", color: "var(--text-negative)" }}>
              <Icon name="CircleX" size={13} />
              {error}
            </span>
          )}
        </>
      )}
    </Modal>
  );
}
