import { expect, type Page } from "@playwright/test";

const seen = new WeakMap<Page, string[]>();

/**
 * Note toute violation de la CSP sur la page : evenement `securitypolicyviolation`
 * (y compris les navigations suivantes) et messages de console du navigateur.
 * A appeler avant la premiere navigation.
 */
export async function watchCsp(page: Page): Promise<void> {
  const violations: string[] = [];
  seen.set(page, violations);
  await page.addInitScript(() => {
    document.addEventListener("securitypolicyviolation", (e) => {
      console.error(`CSP-VIOLATION ${e.violatedDirective} ${e.blockedURI}`);
    });
  });
  page.on("console", (message) => {
    const text = message.text();
    if (text.startsWith("CSP-VIOLATION") || /Content[- ]Security[- ]Policy/i.test(text)) violations.push(text);
  });
}

/** Echoue si la page a rapporte au moins une violation de la CSP. */
export function expectNoCspViolation(page: Page): void {
  expect(seen.get(page) ?? [], "violations de la CSP").toEqual([]);
}
