import { createFileRoute, useNavigate } from "@tanstack/react-router";

import { LoginFlow } from "@/features/auth/LoginFlow";

type LoginSearch = { expired?: 1; created?: 1; verify?: 1; reset?: 1 };

const isSet = (value: unknown) => value === 1 || value === "1";

export const Route = createFileRoute("/_auth/login")({
  validateSearch: (search: Record<string, unknown>): LoginSearch => ({
    ...(isSet(search.expired) ? { expired: 1 } : {}),
    ...(isSet(search.created) ? { created: 1 } : {}),
    ...(isSet(search.verify) ? { verify: 1 } : {}),
    ...(isSet(search.reset) ? { reset: 1 } : {}),
  }),
  component: LoginPage,
});

function LoginPage() {
  const { expired, created, verify, reset } = Route.useSearch();
  const navigate = useNavigate();
  return <LoginFlow expired={expired === 1} created={created === 1} verifyEmail={verify === 1} passwordReset={reset === 1} onSignedIn={() => void navigate({ to: "/", replace: true })} />;
}
