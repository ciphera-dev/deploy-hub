import { createFileRoute, useNavigate } from "@tanstack/react-router";

import { LinkGone } from "@/features/auth/LinkGone";
import { VerifyEmail } from "@/features/auth/VerifyEmail";
import { heldToken } from "@/lib/url-token";

/** Lien de vérification d'adresse e-mail (usage unique, 24 h) : inscription ouverte ou changement d'adresse. */
export const Route = createFileRoute("/_auth/verify-email/")({
  component: VerifyEmailPage,
});

function VerifyEmailPage() {
  const token = heldToken("verify-email");
  const navigate = useNavigate();
  const onSignIn = () => void navigate({ to: "/login" });
  if (!token) return <LinkGone onSignIn={onSignIn} />;
  return <VerifyEmail token={token} onSignIn={onSignIn} />;
}
