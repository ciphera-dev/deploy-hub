import { createFileRoute, redirect } from "@tanstack/react-router";

import { isSection, SettingsPage } from "@/features/settings/SettingsPage";

export const Route = createFileRoute("/_app/settings/$section")({
  staticData: { title: "Paramètres" },
  // Section inconnue : retour a Général.
  beforeLoad: ({ params }) => {
    if (!isSection(params.section)) throw redirect({ to: "/settings/$section", params: { section: "general" }, replace: true });
  },
  component: SettingsSectionPage,
});

function SettingsSectionPage() {
  const { section } = Route.useParams();
  return <SettingsPage section={isSection(section) ? section : "general"} />;
}
