import { createFileRoute, useNavigate } from "@tanstack/react-router";

import { VerifyEmail } from "@/features/auth/VerifyEmail";
import { useUrlToken } from "@/lib/url-token";

/** Lien de verification d'adresse e-mail (usage unique, 24 h) : inscription ouverte ou changement d'adresse. */
export const Route = createFileRoute("/_auth/verify-email/$token")({
  component: VerifyEmailPage,
});

function VerifyEmailPage() {
  // Le jeton quitte l'URL dès la lecture (voir useUrlToken).
  const token = useUrlToken(Route.useParams().token);
  const navigate = useNavigate();
  return <VerifyEmail token={token} onSignIn={() => void navigate({ to: "/login" })} />;
}
