import { expect, it } from "vitest";

import { assertionJson, base64urlToBuffer, bufferToBase64url, creationOptions, isCancellation, requestOptions } from "./webauthn";

const bytes = (...b: number[]) => new Uint8Array(b).buffer;

it("base64url : aller-retour sans remplissage, caracteres - et _", () => {
  const all = new Uint8Array(256).map((_, i) => i);
  const encoded = bufferToBase64url(all);
  expect(encoded).not.toMatch(/[+/=]/);
  expect(new Uint8Array(base64urlToBuffer(encoded))).toEqual(all);
  expect(bufferToBase64url(bytes(0xfb, 0xff))).toBe("-_8");
  expect(bufferToBase64url(new Uint8Array([9, 1, 2, 3]).subarray(1))).toBe("AQID");
});

it("options du serveur : challenge, identifiants et utilisateur decodes", () => {
  const request = requestOptions({ publicKey: { challenge: "AQID", rpId: "localhost", userVerification: "required", allowCredentials: [{ type: "public-key", id: "BAUG" }] } });
  expect(new Uint8Array(request.challenge as ArrayBuffer)).toEqual(new Uint8Array([1, 2, 3]));
  expect(request.rpId).toBe("localhost");
  expect(request.userVerification).toBe("required");
  expect(new Uint8Array(request.allowCredentials![0].id as ArrayBuffer)).toEqual(new Uint8Array([4, 5, 6]));
  expect(requestOptions({ publicKey: { challenge: "AQID" } }).allowCredentials).toBeUndefined();

  const creation = creationOptions({ publicKey: { challenge: "AQID", rp: { id: "localhost", name: "Ciphera" }, user: { id: "BwgJ", name: "a@b.c", displayName: "A" }, excludeCredentials: [{ type: "public-key", id: "BAUG" }] } });
  expect(new Uint8Array(creation.user.id as ArrayBuffer)).toEqual(new Uint8Array([7, 8, 9]));
  expect(creation.user.name).toBe("a@b.c");
  expect(new Uint8Array(creation.excludeCredentials![0].id as ArrayBuffer)).toEqual(new Uint8Array([4, 5, 6]));
});

it("assertion : octets encodes en base64url, userHandle absent conserve a null", () => {
  const cred = {
    id: "BAUG",
    rawId: bytes(4, 5, 6),
    type: "public-key",
    response: { authenticatorData: bytes(7), clientDataJSON: bytes(8), signature: bytes(9), userHandle: null },
    getClientExtensionResults: () => ({}),
  } as unknown as PublicKeyCredential;
  expect(assertionJson(cred)).toEqual({
    id: "BAUG",
    rawId: "BAUG",
    type: "public-key",
    response: { authenticatorData: "Bw", clientDataJSON: "CA", signature: "CQ", userHandle: null },
    extensions: {},
  });
});

it("annulation : NotAllowedError et AbortError seulement", () => {
  expect(isCancellation(new DOMException("x", "NotAllowedError"))).toBe(true);
  expect(isCancellation(new DOMException("x", "AbortError"))).toBe(true);
  expect(isCancellation(new DOMException("x", "InvalidStateError"))).toBe(false);
  expect(isCancellation(new Error("x"))).toBe(false);
});
