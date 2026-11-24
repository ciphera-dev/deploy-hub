import { useState } from "react";

import { Button, Modal, TextField } from "@/design-system";
import { FormError } from "@/features/auth/ui";
import { createGlobalVariable, ENVIRONMENTS } from "@/features/variables/api";

const KEY = /^[A-Z_][A-Z0-9_]{0,127}$/;

/**
 * ResourcePage.jsx#SecretModal : secret partagé par tous les projets, enregistré comme variable
 * globale secrète (chiffrée côté serveur, jamais relue). Portée fixe : tous les projets et environnements.
 */
export function SecretModal({ onClose, onSaved }: { onClose: () => void; onSaved: (key: string) => void }) {
  const [name, setName] = useState("");
  const [value, setValue] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const key = name.trim().toUpperCase().replace(/[^A-Z0-9_]/g, "_");
  const ok = KEY.test(key) && value !== "" && !busy;

  const save = async () => {
    if (!ok) return;
    setBusy(true);
    const result = await createGlobalVariable({ key, value, secret: true, environments: [...ENVIRONMENTS], all_projects: true, projects: [] });
    setBusy(false);
    if (!result.ok) {
      setErr(result.message);
      return;
    }
    setValue("");
    onSaved(key);
  };

  return (
    <Modal
      title="Ajouter un secret"
      description="La valeur est chiffrée immédiatement. Elle ne pourra plus être lue, seulement remplacée."
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Annuler</Button>
          <Button variant="inverse" disabled={!ok} icon={busy ? "Loader" : undefined} onClick={() => void save()}>
            Enregistrer
          </Button>
        </>
      }
    >
      <TextField label="Nom" mono placeholder="API_KEY" value={name} onChange={(v) => { setName(v); setErr(""); }} />
      <TextField label="Valeur" mono type="password" placeholder="••••••••" value={value} onChange={(v) => { setValue(v); setErr(""); }} autoComplete="new-password" />
      <TextField label="Portée" value="Tous les projets" />
      {err && <FormError>{err}</FormError>}
    </Modal>
  );
}
