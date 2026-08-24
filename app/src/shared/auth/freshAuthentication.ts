import { finishFreshPasskeyAuthentication, startFreshPasskeyAuthentication } from "../api";
import { authenticationCredential, authenticationRequestOptions } from "./webauthn";

export async function freshAuthenticate() {
  if (!window.PublicKeyCredential || !navigator.credentials?.get) {
    throw new Error("This browser or device does not support passkeys.");
  }

  const options = await startFreshPasskeyAuthentication();
  const assertion = await navigator.credentials.get({
    publicKey: authenticationRequestOptions(options.publicKey),
  });

  if (!(assertion instanceof PublicKeyCredential)) {
    throw new Error("The authenticator did not return a passkey.");
  }

  await finishFreshPasskeyAuthentication({
    ceremonyId: options.ceremonyId,
    credential: authenticationCredential(assertion),
  });
}

export function passkeyActionError(cause: unknown, fallback: string) {
  if (cause instanceof DOMException && cause.name === "NotAllowedError") {
    return "The passkey operation was cancelled or was not allowed by this device.";
  }

  return cause instanceof Error ? cause.message : fallback;
}
