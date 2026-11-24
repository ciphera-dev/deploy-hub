import { createFileRoute } from "@tanstack/react-router";

import { ScreenPlaceholder } from "@/app/layout/ScreenPlaceholder";

export const Route = createFileRoute("/_app/shortcuts")({
  staticData: { title: "Raccourcis clavier" },
  component: () => <ScreenPlaceholder title="Raccourcis clavier" />,
});
