import { useState, type CSSProperties } from "react";

import { Button, Modal } from "@/design-system";
import { FormError } from "@/features/auth/ui";

// Création d'un deploy hook : un hook déclenche des déploiements sans session,
// le mot de passe du compte est donc redemandé. Durée de vie : 90 jours par
// défaut (modifiable par l'API).

const DAY_MS = 86_400_000;
export const LIFETIMES = [
  { days: 30, label: "30 jours" },
  { days: 90, label: "90 jours (par défaut)" },
  { days: 180, label: "180 jours" },
  { days: 365, label: "1 an" },
] as const;

const fld: CSSProperties = { height: 36, padding: "0 12px", borderRadius: 10, border: "1px solid var(--border-default)", background: "var(--surface-raised)", color: "var(--text-strong)", font: "400 13px/1 var(--font-sans)", outline: "none", width: "100%", boxSizing: "border-box" };
const label: CSSProperties = { display: "flex", flexDirection: "column", gap: 8, font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" };

export interface NewHookModalProps {
  name: string;
  onSubmit: (password: string, expiresAt: string) => Promise<string | null>;
  onClose: () => void;
}

export function NewHookModal({ name, onSubmit, onClose }: NewHookModalProps) {
  const [password, setPassword] = useState("");
  const [days, setDays] = useState<number>(90);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const submit = async () => {
    if (busy || !password) return;
    setBusy(true);
    const failure = await onSubmit(password, new Date(Date.now() + days * DAY_MS).toISOString());
    setBusy(false);
    if (failure) {
      setError(failure);
      setPassword("");
    }
  };

  return (
    <Modal
      title="Créer un deploy hook"
      description={`${name} déclenchera des déploiements sans session : confirmez avec votre mot de passe.`}
      onClose={onClose}
      width={460}
      footer={
        <>
          <Button onClick={onClose}>Annuler</Button>
          <Button variant="inverse" disabled={busy || !password} onClick={() => void submit()}>
            Créer
          </Button>
        </>
      }
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
        style={{ display: "flex", flexDirection: "column", gap: 16 }}
      >
        <label style={label}>
          Mot de passe du compte
          <input autoFocus type="password" autoComplete="current-password" value={password} onChange={(e) => { setPassword(e.target.value); setError(""); }} style={fld} />
        </label>
        <label style={label}>
          Expire après
          <select value={days} onChange={(e) => setDays(Number(e.target.value))} style={fld}>
            {LIFETIMES.map((l) => (
              <option key={l.days} value={l.days}>
                {l.label}
              </option>
            ))}
          </select>
        </label>
        {error && <FormError>{error}</FormError>}
      </form>
    </Modal>
  );
}
