import { renderHook } from "@testing-library/react";
import { expect, it } from "vitest";

import { useUrlToken } from "./url-token";

it("token_removed_from_url : l'adresse perd le jeton, la page le garde en mémoire", () => {
  window.history.pushState({}, "", "/reset/jeton-secret");
  const before = window.history.length;
  const { result, rerender } = renderHook(({ t }) => useUrlToken(t), { initialProps: { t: "jeton-secret" } });
  expect(window.location.pathname).toBe("/login");
  expect(window.location.href).not.toContain("jeton-secret");
  expect(result.current).toBe("jeton-secret");
  // Pas de nouvelle entrée d'historique, et le jeton survit aux rendus suivants.
  expect(window.history.length).toBe(before);
  rerender({ t: "autre" });
  expect(result.current).toBe("jeton-secret");
});
