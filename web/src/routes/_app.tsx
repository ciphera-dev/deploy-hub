import { createFileRoute } from "@tanstack/react-router";

import { AppShell } from "@/app/layout/AppShell";

export const Route = createFileRoute("/_app")({ component: AppShell });
