import { defineConfig, type Plugin } from "vite";
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

// Sert le worker MSW uniquement en developpement (apply: "serve") ; il n'est jamais copie dans dist.
function mswWorkerDev(): Plugin {
  return {
    name: "ciphera:msw-worker-dev",
    apply: "serve",
    configureServer(server) {
      server.middlewares.use("/mockServiceWorker.js", (_req, res) => {
        res.setHeader("Content-Type", "text/javascript");
        res.end(readFileSync(new URL("./dev-assets/mockServiceWorker.js", import.meta.url)));
      });
    },
  };
}

export default defineConfig({
  plugins: [tanstackRouter({ target: "react", autoCodeSplitting: false }), react(), tailwindcss(), mswWorkerDev()],
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  build: {
    outDir: "dist",
    // La CSP (font-src retombe sur default-src 'self') refuse les polices en data: :
    // elles restent des fichiers sous /assets/, jamais inlinees.
    assetsInlineLimit: (filePath) => (/\.(woff2?|ttf|otf|eot)$/i.test(filePath) ? false : undefined),
  },
});
