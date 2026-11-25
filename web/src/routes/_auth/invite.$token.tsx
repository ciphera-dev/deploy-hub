import { createFileRoute, useNavigate } from "@tanstack/react-router";

import { InviteAccept } from "@/features/auth/InviteAccept";
import { useUrlToken } from "@/lib/url-token";

/** Lien d'invitation (usage unique, 72 h) : creation du compte, puis connexion. */
export const Route = createFileRoute("/_auth/invite/$token")({
  component: InvitePage,
});

function InvitePage() {
  // Le jeton quitte l'URL dès la lecture (voir useUrlToken).
  const token = useUrlToken(Route.useParams().token);
  const navigate = useNavigate();
  return (
    <InviteAccept
      token={token}
      onAccepted={() => void navigate({ to: "/login", search: { created: 1 }, replace: true })}
      onSignIn={() => void navigate({ to: "/login" })}
    />
  );
}
