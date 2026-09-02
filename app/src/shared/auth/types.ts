import type { Timestamp, UUID } from "kival-sdk";

export type PasskeyAuthenticationOptions = {
  ceremonyId: string;
  publicKey: {
    challenge: string;
    rpId: string;
    allowCredentials: Array<{
      type: PublicKeyCredentialType;
      id: string;
      transports?: AuthenticatorTransport[];
    }>;
    timeout: number;
    userVerification: UserVerificationRequirement;
  };
};

export type PasskeyAuthenticationCredential = {
  id: string;
  rawId: string;
  type: PublicKeyCredentialType;
  response: {
    authenticatorData: string;
    clientDataJSON: string;
    signature: string;
    userHandle: string | null;
  };
};

export type FinishPasskeyAuthenticationInput = {
  ceremonyId: string;
  credential: PasskeyAuthenticationCredential;
};

export type PasskeyEnrollmentOptions = {
  ceremonyId: string;
  publicKey: {
    challenge: string;
    rp: PublicKeyCredentialRpEntity;
    user: { id: string; name: string; displayName: string };
    pubKeyCredParams: PublicKeyCredentialParameters[];
    timeout: number;
    attestation: AttestationConveyancePreference;
    authenticatorSelection: AuthenticatorSelectionCriteria;
    excludeCredentials: Array<{
      type: PublicKeyCredentialType;
      id: string;
      transports?: AuthenticatorTransport[];
    }>;
  };
};

export type PasskeyRegistrationCredential = {
  id: string;
  rawId: string;
  type: PublicKeyCredentialType;
  response: { clientDataJSON: string; attestationObject: string };
};

export type FinishPasskeyEnrollmentInput = {
  username: string;
  code: string;
  ceremonyId: string;
  label: string;
  credential: PasskeyRegistrationCredential;
};

export type FinishPasskeyRegistrationInput = {
  ceremonyId: string;
  label: string;
  credential: PasskeyRegistrationCredential;
};

export type Passkey = {
  id: UUID;
  userId: UUID;
  credentialId: string;
  label: string | null;
  createdAt: Timestamp;
  updatedAt: Timestamp;
  lastUsedAt: Timestamp | null;
  revokedAt: Timestamp | null;
};

export type PasskeysResponse = { items: Passkey[] };
export type PasskeyResponse = { passkey: Passkey };
