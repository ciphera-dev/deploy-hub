import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { Button, Modal, TextField, type IconName } from "@/design-system";
import { asksForCode, hasSecondFactor } from "@/features/auth/step-up";
import { FormError } from "@/features/auth/ui";

import type { Failure, Result, StepUp } from "./api";

/**
 * Confirmation renforcee (features/auth/step-up, comme la suppression de l'organisation et les
 * deploy hooks) : mot de passe de l'auteur, plus le code de son second facteur s'il en a un ou si
 * le serveur le demande. Champs vides apres un refus.
 */
export function useStepUp() {
  const client = useQueryClient();
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const [needsCode, setNeedsCode] = useState(() => hasSecondFactor(client));
  const trimmed = code.trim();
  return {
    ready: !!password && (!needsCode || !!trimmed),
    value: (): StepUp => (needsCode && trimmed ? { password, code: trimmed } : { password }),
    refused: (failure: Failure) => {
      setPassword("");
      setCode("");
      if (asksForCode(failure.code)) setNeedsCode(true);
    },
    fields: (onEdit: () => void) => (
      <>
        <TextField label="Votre mot de passe" type="password" autoComplete="current-password" value={password} onChange={(v) => { setPassword(v); onEdit(); }} />
        {needsCode && <TextField label="Code de vérification (application ou code de secours)" autoComplete="one-time-code" value={code} onChange={(v) => { setCode(v); onEdit(); }} />}
      </>
    ),
  };
}

/** Action sensible (equipe, reglages SMTP) confirmee par le step-up ; `onDone` recoit la reponse du serveur. */
export function StepUpModal<T>({ title, description, action, icon, run, onClose, onDone }: { title: string; description: string; action: string; icon: IconName; run: (stepUp: StepUp) => Promise<Result<T>>; onClose: () => void; onDone: (data: T) => void }) {
  const stepUp = useStepUp();
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const go = async () => {
    if (!stepUp.ready || busy) return;
    setBusy(true);
    const result = await run(stepUp.value());
    setBusy(false);
    if (!result.ok) {
      setErr(result.message);
      stepUp.refused(result);
      return;
    }
    onDone(result.data);
  };
  return (
    <Modal
      title={title}
      description={description}
      width={460}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Annuler</Button>
          <Button variant="inverse" icon={busy ? "Loader" : icon} disabled={!stepUp.ready || busy} onClick={() => void go()}>
            {action}
          </Button>
        </>
      }
    >
      {stepUp.fields(() => setErr(""))}
      {err && <FormError>{err}</FormError>}
    </Modal>
  );
}
