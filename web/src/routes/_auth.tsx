import { createFileRoute, Outlet } from "@tanstack/react-router";

export const Route = createFileRoute("/_auth")({
  component: () => (
    <main style={{ minHeight: "100vh", background: "var(--surface-page)", color: "var(--text-body)" }}>
      <Outlet />
    </main>
  ),
});
