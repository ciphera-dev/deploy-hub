import { afterEach, expect, it, vi } from "vitest";

import { setSessionExpiredHandler } from "@/lib/api/session";

import { disableTotp, updateProfile } from "./api";
import { deletePasskey } from "./security/api";

const reply = (status: number, code: string) =>
  vi.fn(async () => new Response(JSON.stringify({ error: { code, message: "x" } }), { status, headers: { "content-type": "application/json" } }));

afterEach(() => {
  vi.unstubAllGlobals();
  setSessionExpiredHandler(null);
});

it("expired_session_message : un 401 unauthenticated n'est pas « mot de passe incorrect »", async () => {
  const expire = vi.fn();
  setSessionExpiredHandler(expire);

  vi.stubGlobal("fetch", reply(401, "unauthenticated"));
  const profile = await updateProfile({ name: "A" });
  expect(profile).toEqual({ ok: false, message: "Votre session a expiré. Reconnectez-vous." });
  expect(await disableTotp("mdp")).toEqual({ ok: false, message: "Votre session a expiré. Reconnectez-vous." });
  expect(await deletePasskey("k1", "mdp")).toEqual({ ok: false, reason: "session_expired" });
  expect(expire).toHaveBeenCalledTimes(3);
});

it("un 401 invalid_credentials reste « mot de passe incorrect », sans redirection", async () => {
  const expire = vi.fn();
  setSessionExpiredHandler(expire);

  vi.stubGlobal("fetch", reply(401, "invalid_credentials"));
  expect(await updateProfile({ name: "A" })).toEqual({ ok: false, message: "Mot de passe actuel incorrect." });
  expect(await disableTotp("mdp")).toEqual({ ok: false, message: "Mot de passe incorrect." });
  expect(await deletePasskey("k1", "mdp")).toEqual({ ok: false, reason: "password" });
  expect(expire).not.toHaveBeenCalled();
});
