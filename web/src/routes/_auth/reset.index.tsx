import { createFileRoute, useNavigate } from "@tanstack/react-router";

import { LinkGone } from "@/features/auth/LinkGone";
import { ResetPassword } from "@/features/auth/ResetPassword";
import { heldToken } from "@/lib/url-token";

/** Lien de réinitialisation (usage unique, 30 min) : nouveau mot de passe, puis connexion. */
export const Route = createFileRoute("/_auth/reset/")({
  component: ResetPage,
});

function ResetPage() {
  const token = heldToken("reset");
  const navigate = useNavigate();
  const onSignIn = () => void navigate({ to: "/login" });
  if (!token) return <LinkGone onSignIn={onSignIn} />;
  return <ResetPassword token={token} onDone={() => void navigate({ to: "/login", search: { reset: 1 }, replace: true })} onSignIn={onSignIn} />;
}
