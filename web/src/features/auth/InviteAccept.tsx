import { useQuery } from "@tanstack/react-query";
import type { ReactNode } from "react";

import { Icon, type IconName } from "@/design-system";

import { AccountForm } from "./AccountForm";
import { acceptInvitation, fetchInvitation, type SignupResult } from "./api";
import { AuthLayout, SIGNUP_COPY, SIGNUP_STEPS } from "./AuthLayout";
import { tooManyAttempts } from "./LoginFlow";
import { StepTitle } from "./ui";

/** Noms des roles, repris de Team.jsx. */
export const ROLE_NAMES: Record<string, string> = {
  owner: "Propriétaire",
  admin: "Administrateur",
  developer: "Développeur",
  ops: "Ops",
  viewer: "Lecteur",
};

/** Refus de creation d'un compte, commun a l'invitation et a l'inscription. */
export function signupMessage(result: Extract<SignupResult, { ok: false }>): string {
  if (result.reason === "invalid") return "Lien d'invitation invalide ou déjà utilisé.";
  if (result.reason === "expired") return "Ce lien d'invitation a expiré. Demandez-en un nouveau à un administrateur.";
  if (result.reason === "taken") return "Un compte existe déjà pour cet e-mail. Connectez-vous.";
  if (result.reason === "disabled") return "Les comptes de cette instance sont créés sur invitation.";
  if (result.reason === "rejected") return result.message ?? "Vérifiez les champs du formulaire.";
  if (result.reason === "rate_limited") return tooManyAttempts(result.retryAfter);
  return "Création du compte impossible pour le moment. Réessayez.";
}

/** Lien « Déjà un compte ? Se connecter » de register.html. */
export function SignInLink({ onSignIn }: { onSignIn: () => void }) {
  return (
    <span style={{ textAlign: "center", marginTop: 6, font: "400 13px/1 var(--font-sans)", color: "var(--text-muted)" }}>
      Déjà un compte ?{" "}
      <button type="button" className="login-link" onClick={onSignIn}>
        Se connecter
      </button>
    </span>
  );
}

export interface InviteAcceptProps {
  token: string;
  /** Compte cree : la connexion suit (double authentification comprise). */
  onAccepted: () => void;
  onSignIn: () => void;
}

/** Acceptation d'une invitation : nom et mot de passe ; e-mail et role fixes. */
export function InviteAccept({ token, onAccepted, onSignIn }: InviteAcceptProps) {
  const lookup = useQuery({ queryKey: ["invitation", token], queryFn: () => fetchInvitation(token), staleTime: Infinity, gcTime: 0, retry: false });

  let content: ReactNode;
  const result = lookup.data;
  if (!result) {
    content = (
      <div role="status" className="login-step" style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 14, paddingTop: 40 }}>
        <Icon name="Loader" size={22} color="#a78bfa" className="login-spin" />
        <span style={{ font: "500 14px/1 var(--font-sans)", color: "var(--text-strong)" }}>Chargement de l'invitation…</span>
      </div>
    );
  } else if (!result.ok) {
    const [icon, title, text]: [IconName, string, string] =
      result.reason === "expired"
        ? ["Clock", "Invitation expirée", "Ce lien n'est valable que 72 heures. Demandez-en un nouveau à un administrateur."]
        : result.reason === "invalid"
          ? ["Link2Off", "Lien d'invitation invalide", "Ce lien a déjà été utilisé ou n'existe pas. Demandez une nouvelle invitation à un administrateur."]
          : ["CircleX", "Invitation indisponible", "Impossible de lire l'invitation pour le moment. Réessayez dans un instant."];
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
    const { invitation } = result;
    content = (
      <AccountForm
        title="Créer votre compte"
        intro={
          <>
            Invitation pour <strong style={{ color: "var(--text-strong)", fontWeight: 500 }}>{invitation.email}</strong>, rôle {ROLE_NAMES[invitation.role] ?? invitation.role}.
          </>
        }
        fixedEmail={invitation.email}
        initialName={invitation.name}
        submitLabel="Créer le compte"
        onSubmit={async ({ name, password }) => {
          const accepted = await acceptInvitation(token, { name, password });
          if (accepted.ok) {
            onAccepted();
            return undefined;
          }
          return signupMessage(accepted);
        }}
        footer={<SignInLink onSignIn={onSignIn} />}
      />
    );
  }

  return (
    <AuthLayout steps={SIGNUP_STEPS} current={1} {...SIGNUP_COPY}>
      {content}
    </AuthLayout>
  );
}
