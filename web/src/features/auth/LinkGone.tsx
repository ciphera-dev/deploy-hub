import { Icon } from "@/design-system";

import { AuthLayout, SIGNUP_STEPS } from "./AuthLayout";
import { SignInLink } from "./InviteAccept";
import { StepTitle } from "./ui";

/** Page sans jeton (rechargement après lecture du lien) : le lien est à rouvrir depuis l'e-mail. */
export function LinkGone({ onSignIn }: { onSignIn: () => void }) {
  return (
    <AuthLayout steps={SIGNUP_STEPS} current={1} subtitle="Lien expiré ou déjà utilisé pour accéder à" stepsLabel="Étapes">
      <div className="login-step" style={{ display: "flex", flexDirection: "column", alignItems: "center", textAlign: "center", gap: 14 }}>
        <span style={{ width: 48, height: 48, borderRadius: 99, display: "flex", alignItems: "center", justifyContent: "center", background: "var(--surface-selected)" }}>
          <Icon name="Link2Off" size={20} color="var(--text-strong)" />
        </span>
        <StepTitle>Lien expiré ou déjà utilisé</StepTitle>
        <span style={{ font: "400 13px/1.5 var(--font-sans)", color: "var(--text-muted)" }}>Rouvrez le lien reçu par e-mail.</span>
        <SignInLink onSignIn={onSignIn} />
      </div>
    </AuthLayout>
  );
}
