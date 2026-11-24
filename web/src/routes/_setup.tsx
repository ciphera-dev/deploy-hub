import { createFileRoute, Outlet } from "@tanstack/react-router";

export const Route = createFileRoute("/_setup")({
  component: () => (
    <div style={{ minHeight: "100vh", background: "var(--surface-page)", color: "var(--text-body)" }}>
      <Outlet />
    </div>
  ),
});
