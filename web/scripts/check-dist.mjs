// Echoue le build si le worker MSW (outil de dev) se retrouve dans dist, embarque ensuite dans le binaire.
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

if (!existsSync("dist/index.html")) {
  console.error("check-dist: dist/index.html est absent");
  process.exit(1);
}
if (existsSync("dist/mockServiceWorker.js")) {
  console.error("check-dist: dist/mockServiceWorker.js ne doit pas etre livre en production");
  process.exit(1);
}

// Garde CSP (crates/control-plane/src/http/security_headers.rs) : img-src autorise data:, mais
// font-src (default-src 'self') et les autres directives non. Toute URI data: d'un autre type
// que image/* dans le CSS, le JS ou le HTML ferait violer la CSP en production.
function walk(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(join(dir, e.name)) : [join(dir, e.name)]));
}
const refused = [];
for (const file of walk("dist").filter((f) => /\.(css|js|mjs|html)$/.test(f))) {
  const text = readFileSync(file, "utf8");
  for (const m of text.matchAll(/data:([a-z0-9.+-]+\/[a-z0-9.+-]+)?[;,]/gi)) {
    const type = (m[1] ?? "").toLowerCase();
    if (!type.startsWith("image/")) refused.push(`${file} : data:${type || "(sans type)"}`);
  }
}
if (refused.length) {
  console.error("check-dist: URI data: refusees par la CSP (polices : voir assetsInlineLimit dans vite.config.ts) :");
  for (const r of [...new Set(refused)]) console.error("  " + r);
  process.exit(1);
}
console.log("check-dist: OK");
