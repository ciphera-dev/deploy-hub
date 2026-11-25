import { afterEach, expect, it } from "vitest";
import { getNonce } from "get-nonce";

import { applyCspNonce, readCspNonce } from "./csp-nonce";

afterEach(() => {
  document.head.innerHTML = "";
});

const meta = (content: string) => {
  document.head.innerHTML = `<meta name="csp-nonce" content="${content}">`;
};

it("lit le nonce de la balise meta et le donne à get-nonce (Radix)", () => {
  meta("abc123_-");
  expect(readCspNonce()).toBe("abc123_-");
  applyCspNonce();
  expect(getNonce()).toBe("abc123_-");
});

it("ignore le marqueur non remplacé (dev) et l'absence de balise", () => {
  meta("__CSP_NONCE__");
  expect(readCspNonce()).toBeUndefined();
  document.head.innerHTML = "";
  expect(readCspNonce()).toBeUndefined();
});
