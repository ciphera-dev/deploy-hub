import { createFileRoute, useNavigate } from "@tanstack/react-router";

import { InviteAccept } from "@/features/auth/InviteAccept";
import { LinkGone } from "@/features/auth/LinkGone";
import { heldToken } from "@/lib/url-token";

/** Lien d'invitation (usage unique, 72 h) : création du compte, puis connexion. */
export const Route = createFileRoute("/_auth/invite/")({
  component: InvitePage,
});

function InvitePage() {
  const token = heldToken("invite");
  const navigate = useNavigate();
  const onSignIn = () => void navigate({ to: "/login" });
  if (!token) return <LinkGone onSignIn={onSignIn} />;
  return <InviteAccept token={token} onAccepted={() => void navigate({ to: "/login", search: { created: 1 }, replace: true })} onSignIn={onSignIn} />;
}
