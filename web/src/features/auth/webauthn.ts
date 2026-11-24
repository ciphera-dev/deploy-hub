/**
 * Passkeys (WebAuthn) cote navigateur : conversion entre le JSON du serveur
 * (octets en base64url) et l'API `navigator.credentials`, sans bibliotheque.
 */

type Json = Record<string, unknown>;

export function base64urlToBuffer(value: string): ArrayBuffer {
  const b64 = value.replace(/-/g, "+").replace(/_/g, "/").padEnd(Math.ceil(value.length / 4) * 4, "=");
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes.buffer;
}

export function bufferToBase64url(buffer: ArrayBuffer | ArrayBufferView): string {
  const bytes = buffer instanceof ArrayBuffer ? new Uint8Array(buffer) : new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength);
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** Le navigateur sait faire WebAuthn (contexte securise, API presente). */
export function webauthnSupported(): boolean {
  return typeof window !== "undefined" && typeof window.PublicKeyCredential === "function" && typeof navigator.credentials?.get === "function";
}

const withIds = (list: unknown): PublicKeyCredentialDescriptor[] | undefined =>
  Array.isArray(list) ? list.map((c: Json) => ({ ...(c as object), id: base64urlToBuffer(c.id as string) }) as PublicKeyCredentialDescriptor) : undefined;

/** `options.publicKey` de `POST /api/me/webauthn/register/options`, pret pour `credentials.create()`. */
export function creationOptions(options: unknown): PublicKeyCredentialCreationOptions {
  const pk = (options as { publicKey: Json }).publicKey;
  const user = pk.user as Json;
  return {
    ...(pk as object),
    challenge: base64urlToBuffer(pk.challenge as string),
    user: { ...(user as object), id: base64urlToBuffer(user.id as string) } as PublicKeyCredentialUserEntity,
    excludeCredentials: withIds(pk.excludeCredentials),
  } as PublicKeyCredentialCreationOptions;
}

/** `options.publicKey` de `POST /api/auth/webauthn/assert/options`, pret pour `credentials.get()`. */
export function requestOptions(options: unknown): PublicKeyCredentialRequestOptions {
  const pk = (options as { publicKey: Json }).publicKey;
  return {
    ...(pk as object),
    challenge: base64urlToBuffer(pk.challenge as string),
    allowCredentials: withIds(pk.allowCredentials),
  } as PublicKeyCredentialRequestOptions;
}

/** Reponse d'enregistrement, au format JSON attendu par le serveur. */
export function registrationJson(cred: PublicKeyCredential): Json {
  const r = cred.response as AuthenticatorAttestationResponse;
  return {
    id: cred.id,
    rawId: bufferToBase64url(cred.rawId),
    type: cred.type,
    response: {
      attestationObject: bufferToBase64url(r.attestationObject),
      clientDataJSON: bufferToBase64url(r.clientDataJSON),
      transports: typeof r.getTransports === "function" ? r.getTransports() : undefined,
    },
    extensions: cred.getClientExtensionResults(),
  };
}

/** Reponse d'assertion, au format JSON attendu par le serveur. */
export function assertionJson(cred: PublicKeyCredential): Json {
  const r = cred.response as AuthenticatorAssertionResponse;
  return {
    id: cred.id,
    rawId: bufferToBase64url(cred.rawId),
    type: cred.type,
    response: {
      authenticatorData: bufferToBase64url(r.authenticatorData),
      clientDataJSON: bufferToBase64url(r.clientDataJSON),
      signature: bufferToBase64url(r.signature),
      userHandle: r.userHandle ? bufferToBase64url(r.userHandle) : null,
    },
    extensions: cred.getClientExtensionResults(),
  };
}

/** L'utilisateur a ferme la fenetre du navigateur, ou le delai a expire. */
export function isCancellation(error: unknown): boolean {
  return error instanceof DOMException && (error.name === "NotAllowedError" || error.name === "AbortError");
}
