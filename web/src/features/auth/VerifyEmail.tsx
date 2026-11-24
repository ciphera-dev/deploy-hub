import { useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";

import { Button, Icon, type IconName } from "@/design-system";

import { confirmEmailVerification, fetchEmailVerification, type EmailConfirmResult } from "./api";
import { AuthLayout, type AuthStep } from "./AuthLayout";
import { SignInLink } from "./InviteAccept";
import { StepTitle } from "./ui";

/** Etapes du panneau de gauche : l'adresse est confirmee, la connexion suit. */
const VERIFY_STEPS: AuthStep[] = [
  [1, "Confirmation", "Ouvrez le lien reçu"],
  [2, "Connexion", "Avec votre adresse confirmée"],
  [3, "Accès", "Bienvenue sur Ciphera"],
];

function Notice({ icon, title, text, onSignIn }: { icon: IconName; title: string; text: string; onSignIn: () => void }) {
  return (
    <div className="login-step" style={{ display: "flex", flexDirection: "column", alignItems: "center", textAlign: "center", gap: 14 }}>
      <span style={{ width: 48, height: 48, borderRadius: 99, display: "flex", alignItems: "center", justifyContent: "center", background: "var(--surface-selected)" }}>
        <Icon name={icon} size={20} color="var(--text-strong)" />
      </span>
      <StepTitle>{title}</StepTitle>
      <span style={{ font: "400 13px/1.5 var(--font-sans)", color: "var(--text-muted)" }}>{text}</span>
      <SignInLink onSignIn={onSignIn} />
    </div>
  );
}

function failure(reason: Extract<EmailConfirmResult, { ok: false }>["reason"]): [IconName, string, string] {
  if (reason === "expired") return ["Clock", "Lien expiré", "Ce lien n'est valable que 24 heures. Refaites la demande depuis le panneau (changement d'adresse) ou l'écran d'inscription."];
  if (reason === "invalid") return ["Link2Off", "Lien de vérification invalide", "Ce lien a déjà été utilisé, a été remplacé par une demande plus récente ou n'existe pas."];
  if (reason === "taken") return ["CircleX", "Adresse déjà utilisée", "Cette adresse e-mail est maintenant utilisée par un autre compte. Rien n'a été modifié."];
  return ["CircleX", "Lien indisponible", "Impossible de vérifier le lien pour le moment. Réessayez dans un instant."];
}

export interface VerifyEmailProps {
  token: string;
  onSignIn: () => void;
}

/**
 * Lien de verification d'adresse (usage unique, 24 h) : l'ouverture seule ne change rien,
 * l'adresse est confirmee par le bouton (inscription ouverte ou changement d'adresse).
 */
export function VerifyEmail({ token, onSignIn }: VerifyEmailProps) {
  const lookup = useQuery({ queryKey: ["email-verification", token], queryFn: () => fetchEmailVerification(token), staleTime: Infinity, gcTime: 0, retry: false });
  const [busy, setBusy] = useState(false);
  const [outcome, setOutcome] = useState<EmailConfirmResult | null>(null);

  const confirm = async () => {
    if (busy) return;
    setBusy(true);
    const result = await confirmEmailVerification(token);
    setBusy(false);
    setOutcome(result);
  };

  let content: ReactNode;
  const result = lookup.data;
  if (outcome?.ok && result?.ok) {
    content =
      result.purpose === "change" ? (
        <Notice icon="CircleCheck" title="Adresse e-mail confirmée" text={`Votre compte utilise désormais ${result.email}. Vos autres sessions ont été fermées : connectez-vous avec cette adresse.`} onSignIn={onSignIn} />
      ) : (
        <Notice icon="CircleCheck" title="Adresse e-mail confirmée" text="Votre compte est activé. Connectez-vous pour continuer." onSignIn={onSignIn} />
      );
  } else if (outcome && !outcome.ok) {
    const [icon, title, text] = failure(outcome.reason);
    content = <Notice icon={icon} title={title} text={text} onSignIn={onSignIn} />;
  } else if (!result) {
    content = (
      <div role="status" className="login-step" style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 14, paddingTop: 40 }}>
        <Icon name="Loader" size={22} color="#a78bfa" className="login-spin" />
        <span style={{ font: "500 14px/1 var(--font-sans)", color: "var(--text-strong)" }}>Vérification du lien…</span>
      </div>
    );
  } else if (!result.ok) {
    const [icon, title, text] = failure(result.reason);
    content = <Notice icon={icon} title={title} text={text} onSignIn={onSignIn} />;
  } else {
    content = (
      <div className="login-step" style={{ display: "flex", flexDirection: "column", gap: 16 }}>
        <StepTitle>Confirmer votre adresse e-mail</StepTitle>
        <span style={{ font: "400 13px/1.5 var(--font-sans)", color: "var(--text-muted)", textAlign: "center" }}>
          {result.purpose === "change" ? (
            <>
              Votre compte utilisera <strong style={{ color: "var(--text-strong)" }}>{result.email}</strong> pour se connecter. Vos autres sessions ouvertes seront fermées.
            </>
          ) : (
            <>
              Activez le compte créé avec <strong style={{ color: "var(--text-strong)" }}>{result.email}</strong>.
            </>
          )}
        </span>
        <div style={{ height: 4 }} />
        <Button variant="inverse" block icon={busy ? "Loader" : undefined} disabled={busy} aria-busy={busy || undefined} onClick={() => void confirm()}>
          Confirmer l'adresse
        </Button>
        <SignInLink onSignIn={onSignIn} />
      </div>
    );
  }

  return (
    <AuthLayout steps={VERIFY_STEPS} current={1} subtitle="Confirmez votre adresse e-mail pour accéder à" stepsLabel="Étapes de la confirmation">
      {content}
    </AuthLayout>
  );
}
