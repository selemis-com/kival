import type {
  PasskeyAuthenticationCredential,
  PasskeyAuthenticationOptions,
  PasskeyEnrollmentOptions,
  PasskeyRegistrationCredential,
} from "./types";

const ENROLLMENT_CODE_STORAGE_KEY = "kival.passkeyEnrollmentCode";

export function rememberEnrollmentCode(code: string): void {
  try {
    window.sessionStorage.setItem(ENROLLMENT_CODE_STORAGE_KEY, code);
  } catch {
    return;
  }
}

export function readEnrollmentCode(): string | null {
  try {
    return window.sessionStorage.getItem(ENROLLMENT_CODE_STORAGE_KEY);
  } catch {
    return null;
  }
}

export function clearEnrollmentCode(): void {
  try {
    window.sessionStorage.removeItem(ENROLLMENT_CODE_STORAGE_KEY);
  } catch {
    return;
  }
}

export function decodeBase64Url(value: string): ArrayBuffer {
  const base64 = value.replace(/-/g, "+").replace(/_/g, "/");
  const padded = base64.padEnd(Math.ceil(base64.length / 4) * 4, "=");
  const binary = window.atob(padded);
  const bytes = new Uint8Array(binary.length);

  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }

  return bytes.buffer;
}

export function authenticationRequestOptions(
  options: PasskeyAuthenticationOptions["publicKey"],
): PublicKeyCredentialRequestOptions {
  return {
    ...options,
    challenge: decodeBase64Url(options.challenge),
    allowCredentials: options.allowCredentials.map((credential) => ({
      ...credential,
      id: decodeBase64Url(credential.id),
    })),
  };
}

export function registrationRequestOptions(
  options: PasskeyEnrollmentOptions["publicKey"],
): PublicKeyCredentialCreationOptions {
  return {
    ...options,
    challenge: decodeBase64Url(options.challenge),
    user: {
      ...options.user,
      id: decodeBase64Url(options.user.id),
    },
    excludeCredentials: options.excludeCredentials?.map((credential) => ({
      ...credential,
      id: decodeBase64Url(credential.id),
    })),
  };
}

function encodeBase64Url(value: ArrayBuffer): string {
  const bytes = new Uint8Array(value);
  let binary = "";

  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }

  return window.btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/u, "");
}

export function registrationCredential(
  credential: PublicKeyCredential,
): PasskeyRegistrationCredential {
  if (!(credential.response instanceof AuthenticatorAttestationResponse)) {
    throw new Error("The authenticator returned an unexpected response.");
  }

  return {
    id: credential.id,
    rawId: encodeBase64Url(credential.rawId),
    type: "public-key",
    response: {
      clientDataJSON: encodeBase64Url(credential.response.clientDataJSON),
      attestationObject: encodeBase64Url(credential.response.attestationObject),
    },
  };
}

export function authenticationCredential(
  credential: PublicKeyCredential,
): PasskeyAuthenticationCredential {
  if (!(credential.response instanceof AuthenticatorAssertionResponse)) {
    throw new Error("The authenticator returned an unexpected response.");
  }

  return {
    id: credential.id,
    rawId: encodeBase64Url(credential.rawId),
    type: "public-key",
    response: {
      authenticatorData: encodeBase64Url(credential.response.authenticatorData),
      clientDataJSON: encodeBase64Url(credential.response.clientDataJSON),
      signature: encodeBase64Url(credential.response.signature),
      userHandle: credential.response.userHandle
        ? encodeBase64Url(credential.response.userHandle)
        : null,
    },
  };
}
