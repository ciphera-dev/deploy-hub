import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { AppProviders } from "@/app/providers";
import { applyCspNonce } from "@/lib/csp-nonce";
import "@/styles/index.css";

async function demarrer() {
  // MSW uniquement en developpement et sur demande explicite.
  if (import.meta.env.DEV && import.meta.env.VITE_MSW === "1") {
    const { worker } = await import("@/mocks/browser");
    await worker.start();
  }

  applyCspNonce();
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <AppProviders />
    </StrictMode>,
  );
}

void demarrer();
