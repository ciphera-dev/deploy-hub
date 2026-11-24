// Echoue le build si le worker MSW (outil de dev) se retrouve dans dist, embarque ensuite dans le binaire.
import { existsSync } from "node:fs";

if (!existsSync("dist/index.html")) {
  console.error("check-dist: dist/index.html est absent");
  process.exit(1);
}
if (existsSync("dist/mockServiceWorker.js")) {
  console.error("check-dist: dist/mockServiceWorker.js ne doit pas etre livre en production");
  process.exit(1);
}
console.log("check-dist: OK");
