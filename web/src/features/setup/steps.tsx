import { Icon, RadioCard } from "@/design-system";

import { RowCard, SetupInput, Spinner, StepHeading } from "./ui";

export interface AdminForm {
  first: string;
  last: string;
  email: string;
  pwd: string;
  pwd2: string;
}

export const MIN_PASSWORD = 12;
const EMAIL = /^[^@\s]+@[^@\s]+\.[a-z]{2,}$/i;

export function adminValid(a: AdminForm): boolean {
  return a.first.trim().length > 0 && a.last.trim().length > 0 && EMAIL.test(a.email.trim()) && [...a.pwd].length >= MIN_PASSWORD && a.pwd === a.pwd2;
}

export function AdminStep({ value, onChange }: { value: AdminForm; onChange: (next: AdminForm) => void }) {
  const set = (key: keyof AdminForm) => (v: string) => onChange({ ...value, [key]: v });
  return (
    <>
      <StepHeading title="Compte administrateur" subtitle="Ce compte aura tous les droits sur l'instance." />
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
        <SetupInput label="Prénom" value={value.first} onChange={set("first")} autoFocus />
        <SetupInput label="Nom" value={value.last} onChange={set("last")} />
      </div>
      <SetupInput label="E-mail" value={value.email} onChange={set("email")} placeholder="vous@entreprise.com" />
      <SetupInput label="Mot de passe" type="password" value={value.pwd} onChange={set("pwd")} hint="12 caractères minimum." />
      <SetupInput
        label="Confirmer le mot de passe"
        type="password"
        value={value.pwd2}
        onChange={set("pwd2")}
        hint={value.pwd2 && value.pwd !== value.pwd2 ? "Les mots de passe ne correspondent pas." : "La double authentification sera proposée à la première connexion."}
      />
    </>
  );
}

/** Tailscale (v0.3) reste masque : seule l'option « domaine plus tard » existe. */
export function AccessStep() {
  return (
    <>
      <StepHeading title="Accès au panel" subtitle="Choisissez comment votre équipe accédera au dashboard." />
      <RadioCard icon="Globe" title="Domaine plus tard" description="Le panel reste joignable en HTTPS sur l'adresse du serveur." selected />
      <span style={{ display: "flex", alignItems: "center", gap: 8, padding: "10px 12px", borderRadius: 10, background: "var(--surface-raised)", font: "400 12px/1.4 var(--font-sans)", color: "var(--text-muted)" }}>
        <Icon name="Info" size={14} />
        Vous pourrez rattacher un domaine plus tard dans Paramètres › Domaine du panel.
      </span>
    </>
  );
}

const FIN = ["Création du compte administrateur", "Configuration de l'organisation", "Ouverture de la session", "Verrouillage de l'assistant"];

export interface FinalizeStepProps {
  pending: boolean;
  error: string | null;
}

export function FinalizeStep({ pending, error }: FinalizeStepProps) {
  return (
    <>
      <StepHeading title={error ? "L'installation a échoué" : "Finalisation…"} subtitle={error ?? "Quelques secondes, ne fermez pas cette page."} />
      <RowCard>
        {FIN.map((t, i) => (
          <div key={t} style={{ display: "flex", alignItems: "center", gap: 12, padding: "14px 16px", borderTop: i ? "1px solid var(--border-subtle)" : "none" }}>
            {error ? <Icon name="Circle" size={16} color="var(--border-strong)" /> : pending ? <Spinner /> : <Icon name="CircleCheck" size={16} color="var(--green-500)" />}
            <span style={{ font: "400 13px/1 var(--font-sans)", color: error ? "var(--text-muted)" : "var(--text-strong)" }}>{t}</span>
          </div>
        ))}
      </RowCard>
    </>
  );
}
