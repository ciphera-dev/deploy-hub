import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { Button, Modal, Switch, TextField, useToast } from "@/design-system";
import { fetchMe, ME_QUERY_KEY } from "@/features/auth/api";

import { disableTotp } from "../api";
import { Alert, card, Pill, Tile } from "../ui";
import { EnrollTotp } from "./EnrollTotp";

/** Desactivation de la TOTP, mot de passe redemande (ecran absent des maquettes). */
function DisableTotp({ onClose, onDone }: { onClose: () => void; onDone: () => void }) {
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const confirm = async () => {
    if (busy || !password) return;
    setBusy(true);
    setError("");
    const result = await disableTotp(password);
    setBusy(false);
    setPassword("");
    if (result.ok) onDone();
    else setError(result.message);
  };
  return (
    <Modal
      title="Désactiver l'application d'authentification"
      description="Confirmez avec votre mot de passe. Les codes de l'application ne seront plus demandés."
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Annuler</Button>
          <Button variant="inverse" disabled={busy || !password} aria-busy={busy || undefined} onClick={() => void confirm()}>
            Désactiver
          </Button>
        </>
      }
    >
      <TextField label="Mot de passe" type="password" autoComplete="current-password" value={password} onChange={setPassword} />
      {error && <Alert>{error}</Alert>}
    </Modal>
  );
}

/**
 * Carte « Application d'authentification » de ProfileSecurity.jsx : l'interrupteur
 * lance l'enrolement (activation) ou la desactivation (mot de passe redemande).
 */
export function TwoFactorCard() {
  const queryClient = useQueryClient();
  const { flash } = useToast();
  const me = useQuery({ queryKey: ME_QUERY_KEY, queryFn: fetchMe });
  const [dialog, setDialog] = useState<"enroll" | "disable" | null>(null);
  const enabled = typeof me.data === "object" && me.data !== null && me.data.totp_enabled;

  const done = (message: string) => {
    setDialog(null);
    void queryClient.invalidateQueries({ queryKey: ME_QUERY_KEY });
    flash(message);
  };

  return (
    <div style={{ ...card, padding: 18, display: "flex", flexDirection: "column", gap: 16 }}>
      <div style={{ display: "flex", alignItems: "flex-start", justifyContent: "space-between" }}>
        <Tile icon="Smartphone" size={40} />
        <Pill ok={enabled}>{enabled ? "Active" : "Inactive"}</Pill>
      </div>
      <span style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        <span style={{ font: "500 14px/1.2 var(--font-sans)", color: "var(--text-strong)" }}>Application d'authentification</span>
        <span style={{ font: "400 12px/1.35 var(--font-sans)", color: "var(--text-subtle)" }}>Google Authenticator, 1Password…</span>
      </span>
      <div style={{ marginTop: "auto", display: "flex", justifyContent: "flex-end" }}>
        <Switch checked={enabled} onChange={(on) => setDialog(on ? "enroll" : "disable")} ariaLabel="Application d'authentification" />
      </div>
      {dialog === "enroll" && <EnrollTotp onClose={() => setDialog(null)} onDone={() => done("2FA activée")} />}
      {dialog === "disable" && <DisableTotp onClose={() => setDialog(null)} onDone={() => done("2FA désactivée")} />}
    </div>
  );
}
