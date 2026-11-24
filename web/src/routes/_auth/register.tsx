import { createFileRoute, useNavigate } from "@tanstack/react-router";

import { RegisterScreen } from "@/features/auth/RegisterScreen";

export const Route = createFileRoute("/_auth/register")({
  component: RegisterPage,
});

function RegisterPage() {
  const navigate = useNavigate();
  return (
    <RegisterScreen
      onCreated={() => void navigate({ to: "/login", search: { verify: 1 }, replace: true })}
      onSignIn={() => void navigate({ to: "/login" })}
      onSignedIn={() => void navigate({ to: "/", replace: true })}
    />
  );
}
