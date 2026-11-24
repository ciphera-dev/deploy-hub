import { useQuery } from "@tanstack/react-query";
import { useState, type FormEvent, type ReactNode } from "react";

import { Button, Icon, TextField, type IconName } from "@/design-system";

import { MIN_PASSWORD } from "./AccountForm";
import { fetchResetToken, resetPassword, type ResetResult } from "./api";
import { AuthLayout, type AuthStep } from "./AuthLayout";
import { SignInLink } from "./InviteAccept";
import { tooManyAttempts } from "./LoginFlow";
import { FormError, StepTitle } from "./ui";

/** Etapes du panneau de gauche : le mot de passe change, la connexion suit. */
const RESET_STEPS: AuthStep[] = [
  [1, "Nouveau mot de passe", "Choisissez-le"],
  [2, "Connexion", "Avec le nouveau mot de passe"],
  [3, "Accès", "Bienvenue sur Ciphera"],
];

function failureMessage(result: Extract<ResetResult, { ok: false }>): string {
  if (result.reason === "expired") return "Ce lien a expiré. Faites une nouvelle demande depuis l'écran de connexion.";
  if (result.reason === "invalid") return "Ce lien a déjà été utilisé ou n'existe pas.";
  if (result.reason === "rejected") return result.message ?? "Vérifiez le mot de passe.";
  if (result.reason === "rate_limited") return tooManyAttempts(result.retryAfter);
  return "Enregistrement impossible pour le moment. Réessayez.";
}

export interface ResetPasswordProps {
  token: string;
  /** Mot de passe change (sessions fermees) : la connexion suit. */
  onDone: () => void;
  onSignIn: () => void;
}

/** Lien de reinitialisation (usage unique, 30 min) : nouveau mot de passe, sans ouvrir de session. */
export function ResetPassword({ token, onDone, onSignIn }: ResetPasswordProps) {
  const lookup = useQuery({ queryKey: ["password-reset", token], queryFn: () => fetchResetToken(token), staleTime: Infinity, gcTime: 0, retry: false });
  const [pwd, setPwd] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");

  const mismatch = confirm.length > 0 && confirm !== pwd;
  const valid = [...pwd].length >= MIN_PASSWORD && confirm === pwd;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!valid || busy) return;
    setErr("");
    setBusy(true);
    const result = await resetPassword(token, pwd);
    setBusy(false);
    if (result.ok) onDone();
    else setErr(failureMessage(result));
  };

  const edit = (set: (v: string) => void) => (v: string) => {
    set(v);
    setErr("");
  };

  let content: ReactNode;
  const result = lookup.data;
  if (!result) {
    content = (
      <div role="status" className="login-step" style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 14, paddingTop: 40 }}>
        <Icon name="Loader" size={22} color="#a78bfa" className="login-spin" />
        <span style={{ font: "500 14px/1 var(--font-sans)", color: "var(--text-strong)" }}>Vérification du lien…</span>
      </div>
    );
  } else if (!result.ok) {
    const [icon, title, text]: [IconName, string, string] =
      result.reason === "expired"
        ? ["Clock", "Lien expiré", "Ce lien n'est valable que 30 minutes. Faites une nouvelle demande depuis l'écran de connexion (« mot de passe oublié »)."]
        : result.reason === "invalid"
          ? ["Link2Off", "Lien de réinitialisation invalide", "Ce lien a déjà été utilisé, a été remplacé par une demande plus récente ou n'existe pas."]
          : ["CircleX", "Lien indisponible", "Impossible de vérifier le lien pour le moment. Réessayez dans un instant."];
    content = (
      <div className="login-step" style={{ display: "flex", flexDirection: "column", alignItems: "center", textAlign: "center", gap: 14 }}>
        <span style={{ width: 48, height: 48, borderRadius: 99, display: "flex", alignItems: "center", justifyContent: "center", background: "var(--surface-selected)" }}>
          <Icon name={icon} size={20} color="var(--text-strong)" />
        </span>
        <StepTitle>{title}</StepTitle>
        <span style={{ font: "400 13px/1.5 var(--font-sans)", color: "var(--text-muted)" }}>{text}</span>
        <SignInLink onSignIn={onSignIn} />
      </div>
    );
  } else {
    content = (
      <form onSubmit={(e) => void submit(e)} noValidate className="login-step" style={{ display: "flex", flexDirection: "column", gap: 16 }}>
        <StepTitle>Choisir un nouveau mot de passe</StepTitle>
        <span style={{ font: "400 13px/1.5 var(--font-sans)", color: "var(--text-muted)", textAlign: "center" }}>Toutes vos sessions ouvertes seront fermées. La double authentification reste active.</span>
        <div style={{ height: 4 }} />
        <TextField label="Nouveau mot de passe" type="password" autoComplete="new-password" hint={`${MIN_PASSWORD} caractères minimum.`} value={pwd} onChange={edit(setPwd)} />
        <TextField label="Confirmer le mot de passe" type="password" autoComplete="new-password" value={confirm} onChange={edit(setConfirm)} />
        {mismatch && <span style={{ font: "400 12px/1.3 var(--font-sans)", color: "#f87171" }}>Les deux mots de passe ne correspondent pas.</span>}
        {err && <FormError>{err}</FormError>}
        <Button type="submit" variant="inverse" block icon={busy ? "Loader" : undefined} disabled={!valid || busy} aria-busy={busy || undefined}>
          Enregistrer le mot de passe
        </Button>
        <SignInLink onSignIn={onSignIn} />
      </form>
    );
  }

  return (
    <AuthLayout steps={RESET_STEPS} current={1} subtitle="Choisissez un nouveau mot de passe pour accéder à" stepsLabel="Étapes de la réinitialisation">
      {content}
    </AuthLayout>
  );
}
