import { afterEach, expect, it, vi } from "vitest";

import { copyText } from "./clipboard";

afterEach(() => vi.unstubAllGlobals());

it("copy_failure_no_copied_label : l'échec ou l'absence de presse-papiers renvoie false", async () => {
  Object.defineProperty(navigator, "clipboard", { value: { writeText: vi.fn().mockRejectedValue(new Error("denied")) }, configurable: true });
  expect(await copyText("x")).toBe(false);
  Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true });
  expect(await copyText("x")).toBe(false);
  const writeText = vi.fn(async () => undefined);
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  expect(await copyText("x")).toBe(true);
  expect(writeText).toHaveBeenCalledWith("x");
});
