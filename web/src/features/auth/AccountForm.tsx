import { useState, type FormEvent, type ReactNode } from "react";

import { Button, TextField } from "@/design-system";

import { looksLikeEmail } from "./LoginFlow";
import { FormError, StepTitle } from "./ui";

/** Longueur minimale du mot de passe (politique du serveur). */
export const MIN_PASSWORD = 12;

export interface AccountValues {
  name: string;
  email: string;
  password: string;
}

export interface AccountFormProps {
  title: string;
  /** Texte sous le titre (ex. e-mail et role de l'invitation). */
  intro?: ReactNode;
  /** E-mail impose (invitation) : non modifiable, le champ n'est pas affiche. */
  fixedEmail?: string;
  /** Sans mot de passe (inscription ouverte) : il se choisit en confirmant le lien recu par e-mail. */
  withoutPassword?: boolean;
  initialName?: string;
  submitLabel: string;
  /** `undefined` : compte cree ; sinon le refus a afficher. */
  onSubmit: (values: AccountValues) => Promise<string | undefined>;
  footer?: ReactNode;
}

/** Formulaire de creation de compte, porte de register.html (nom, e-mail, mot de passe). */
export function AccountForm({ title, intro, fixedEmail, withoutPassword = false, initialName = "", submitLabel, onSubmit, footer }: AccountFormProps) {
  const [name, setName] = useState(initialName);
  const [email, setEmail] = useState("");
  const [pwd, setPwd] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");

  const valid = name.trim().length > 0 && (fixedEmail !== undefined || looksLikeEmail(email)) && (withoutPassword || [...pwd].length >= MIN_PASSWORD);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!valid || busy) return;
    setErr("");
    setBusy(true);
    const refusal = await onSubmit({ name: name.trim(), email: (fixedEmail ?? email).trim(), password: withoutPassword ? "" : pwd });
    setBusy(false);
    if (refusal) setErr(refusal);
  };

  const edit = (set: (v: string) => void) => (v: string) => {
    set(v);
    setErr("");
  };

  return (
    <form onSubmit={(e) => void submit(e)} noValidate className="login-step" style={{ display: "flex", flexDirection: "column", gap: 16 }}>
      <StepTitle>{title}</StepTitle>
      {intro && <span style={{ font: "400 13px/1.5 var(--font-sans)", color: "var(--text-muted)", textAlign: "center" }}>{intro}</span>}
      <div style={{ height: 4 }} />
      <TextField label="Nom complet" autoComplete="name" value={name} onChange={edit(setName)} />
      {fixedEmail === undefined && <TextField label="E-mail" type="email" autoComplete="email" value={email} onChange={edit(setEmail)} />}
      {!withoutPassword && <TextField label="Mot de passe" type="password" autoComplete="new-password" hint={`${MIN_PASSWORD} caractères minimum.`} value={pwd} onChange={edit(setPwd)} />}
      {err && <FormError>{err}</FormError>}
      <Button type="submit" variant="inverse" block icon={busy ? "Loader" : undefined} disabled={!valid || busy} aria-busy={busy || undefined}>
        {submitLabel}
      </Button>
      {footer}
    </form>
  );
}
