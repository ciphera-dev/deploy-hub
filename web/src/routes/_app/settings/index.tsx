import { createFileRoute, redirect } from "@tanstack/react-router";

// Pages2.jsx#SettingsV2 s'ouvre sur la section Général.
export const Route = createFileRoute("/_app/settings/")({
  beforeLoad: () => {
    throw redirect({ to: "/settings/$section", params: { section: "general" }, replace: true });
  },
});
