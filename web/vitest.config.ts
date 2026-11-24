import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";

export default defineConfig({
  plugins: [react()],
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    css: true,
    include: ["src/**/*.test.{ts,tsx}"],
    // Premier test d'un fichier : jsdom, transformation et chargement
    // paresseux des routes se paient au demarrage du worker ; sous charge
    // (workers paralleles, cargo en meme temps) cela depassait les 5 s par
    // defaut. Les attentes de Testing Library ont leur propre plafond (setup).
    testTimeout: 15_000,
  },
});

