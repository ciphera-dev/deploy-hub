import { createFileRoute, useNavigate } from "@tanstack/react-router";

import { ResetPassword } from "@/features/auth/ResetPassword";

/** Lien de reinitialisation (usage unique, 30 min) : nouveau mot de passe, puis connexion. */
export const Route = createFileRoute("/_auth/reset/$token")({
  component: ResetPage,
});

function ResetPage() {
  const { token } = Route.useParams();
  const navigate = useNavigate();
  return (
    <ResetPassword
      token={token}
      onDone={() => void navigate({ to: "/login", search: { reset: 1 }, replace: true })}
      onSignIn={() => void navigate({ to: "/login" })}
    />
  );
}
