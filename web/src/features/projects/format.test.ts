import { expect, it } from "vitest";

import { timeAgo } from "./format";

const NOW = Date.parse("2026-05-15T12:00:00Z");

it("timeAgo : unités françaises", () => {
  const ago = (seconds: number) => timeAgo(new Date(NOW - seconds * 1000).toISOString(), NOW);
  expect(ago(5)).toBe("à l'instant");
  expect(ago(4 * 60)).toBe("il y a 4 min");
  expect(ago(3 * 3600)).toBe("il y a 3 h");
  expect(ago(2 * 86400)).toBe("il y a 2 j");
  expect(ago(65 * 86400)).toBe("il y a 2 mois");
  expect(timeAgo("pas une date", NOW)).toBe("—");
});
