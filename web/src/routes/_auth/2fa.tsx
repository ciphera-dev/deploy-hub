import { createFileRoute, useNavigate } from "@tanstack/react-router";

import { LoginFlow } from "@/features/auth/LoginFlow";

/** Session ouverte par le mot de passe, en attente du second facteur (ex. page rechargee pendant la verification). */
export const Route = createFileRoute("/_auth/2fa")({
  component: TwoFactorPage,
});

function TwoFactorPage() {
  const navigate = useNavigate();
  return <LoginFlow initialStep="2fa" onSignedIn={() => void navigate({ to: "/", replace: true })} />;
}
