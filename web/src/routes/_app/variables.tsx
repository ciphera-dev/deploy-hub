import { createFileRoute } from "@tanstack/react-router";

import { GlobalVariablesPage } from "@/features/variables/GlobalVariables";

export const Route = createFileRoute("/_app/variables")({
  staticData: { title: "Variables globales" },
  component: GlobalVariablesPage,
});
