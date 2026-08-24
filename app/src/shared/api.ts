import { createKivalClient } from "kival-sdk";
import type {
  FinishPasskeyAuthenticationInput,
  FinishPasskeyEnrollmentInput,
  FinishPasskeyRegistrationInput,
  PasskeyAuthenticationOptions,
  PasskeyEnrollmentOptions,
  PasskeyResponse,
  PasskeysResponse,
} from "./auth/types";
import { browserTransport } from "./browserTransport";
import { jsonRequest, listParams, pathId, withParams } from "./http";
import type {
  ApiKeyListResponse,
  ApiKeyResponse,
  AuthenticatedSessionResponse,
  CreateApiKeyRequest,
  CreateApiKeyResponse,
  CreateWorkspaceRequest,
  SessionListResponse,
  SessionOnlyResponse,
  UpdateApiKeyRequest,
  UserResponse,
  WorkspaceResponse,
} from "./types";

export const kival = createKivalClient({ transport: browserTransport });

/**
 * Constructs an attachment URL for browser-session navigation.
 *
 * Authentication is supplied by the browser's Kival session cookie.
 */
export function getObjectAttachmentContentUrl(
  workspaceId: string,
  objectId: string,
  attachmentId: string,
) {
  return browserTransport.url(
    `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}` +
      `/attachments/${pathId(attachmentId)}/content`,
  );
}

export function getCurrentIdentity(signal?: AbortSignal) {
  return browserTransport.requestJson<UserResponse>("/auth/whoami", { signal });
}

export function startPasskeyAuthentication(username: string) {
  return browserTransport.requestJson<PasskeyAuthenticationOptions>(
    "/auth/passkey/authentication/options",
    jsonRequest("POST", { username }),
  );
}

export function finishPasskeyAuthentication(input: FinishPasskeyAuthenticationInput) {
  return browserTransport.requestJson<AuthenticatedSessionResponse>(
    "/auth/passkey/authentication/finish",
    jsonRequest("POST", input),
  );
}

export function startPasskeyEnrollment(username: string, code: string) {
  return browserTransport.requestJson<PasskeyEnrollmentOptions>(
    "/auth/passkey/enrollment/options",
    jsonRequest("POST", { username, code }),
  );
}

export function finishPasskeyEnrollment(input: FinishPasskeyEnrollmentInput) {
  return browserTransport.requestJson<AuthenticatedSessionResponse>(
    "/auth/passkey/enrollment/finish",
    jsonRequest("POST", input),
  );
}

export function listPasskeys(signal?: AbortSignal) {
  return browserTransport.requestJson<PasskeysResponse>("/auth/passkeys", { signal });
}

export function startPasskeyRegistration() {
  return browserTransport.requestJson<PasskeyEnrollmentOptions>(
    "/auth/passkeys/registration/options",
    {
      method: "POST",
    },
  );
}

export function finishPasskeyRegistration(input: FinishPasskeyRegistrationInput) {
  return browserTransport.requestJson<PasskeyResponse>(
    "/auth/passkeys/registration/finish",
    jsonRequest("POST", input),
  );
}

export function startFreshPasskeyAuthentication() {
  return browserTransport.requestJson<PasskeyAuthenticationOptions>(
    "/auth/passkeys/fresh/options",
    {
      method: "POST",
    },
  );
}

export function finishFreshPasskeyAuthentication(input: FinishPasskeyAuthenticationInput) {
  return browserTransport.requestVoid("/auth/passkeys/fresh/finish", jsonRequest("POST", input));
}

export function revokePasskey(passkeyId: string) {
  return browserTransport.requestJson<PasskeyResponse>(
    `/auth/passkeys/${pathId(passkeyId)}/revoke`,
    {
      method: "POST",
    },
  );
}

export function listSessions(signal?: AbortSignal) {
  return browserTransport.requestJson<SessionListResponse>("/auth/sessions", { signal });
}

export function revokeSession(sessionId: string) {
  return browserTransport.requestJson<SessionOnlyResponse>(
    `/auth/sessions/${pathId(sessionId)}/revoke`,
    { method: "POST" },
  );
}

export function logout() {
  return browserTransport.requestVoid("/auth/logout", { method: "POST" });
}

export function listApiKeys(cursor?: string | null, signal?: AbortSignal) {
  return browserTransport.requestJson<ApiKeyListResponse>(
    withParams("/auth/api-keys", listParams({ cursor })),
    { signal },
  );
}

export function createApiKey(input: CreateApiKeyRequest) {
  return browserTransport.requestJson<CreateApiKeyResponse>(
    "/auth/api-keys",
    jsonRequest("POST", input),
  );
}

export function updateApiKey(apiKeyId: string, input: UpdateApiKeyRequest) {
  return browserTransport.requestJson<ApiKeyResponse>(
    `/auth/api-keys/${pathId(apiKeyId)}`,
    jsonRequest("PATCH", input),
  );
}

export function revokeApiKey(apiKeyId: string) {
  return browserTransport.requestJson<ApiKeyResponse>(`/auth/api-keys/${pathId(apiKeyId)}/revoke`, {
    method: "POST",
  });
}

export function createWorkspace(input: CreateWorkspaceRequest) {
  return browserTransport.requestJson<WorkspaceResponse>("/workspaces", jsonRequest("POST", input));
}

export function listObjectCommentary(
  workspaceId: string,
  objectId: string,
  cursor?: string | null,
  signal?: AbortSignal,
) {
  return kival.listObjectCommentary({ workspaceId, objectId, cursor, signal });
}

export function listCommentThreadComments(
  workspaceId: string,
  objectId: string,
  threadId: string,
  cursor?: string | null,
  signal?: AbortSignal,
) {
  return kival.listCommentThreadComments({ workspaceId, objectId, threadId, cursor, signal });
}

export function listCommentMentionCandidates(
  workspaceId: string,
  objectId: string,
  q: string,
  signal?: AbortSignal,
) {
  return kival.listCommentMentionCandidates({ workspaceId, objectId, q, signal });
}

export function createCommentThread(
  workspaceId: string,
  objectId: string,
  input: import("kival-sdk").CreateCommentRequest,
) {
  return kival.createCommentThread({ workspaceId, objectId, input });
}

export function replyToCommentThread(
  workspaceId: string,
  objectId: string,
  threadId: string,
  input: import("kival-sdk").CreateCommentRequest,
) {
  return kival.replyToCommentThread({ workspaceId, objectId, threadId, input });
}

export function updateComment(
  workspaceId: string,
  objectId: string,
  commentId: string,
  input: import("kival-sdk").UpdateCommentRequest,
) {
  return kival.updateComment({ workspaceId, objectId, commentId, input });
}

export function deleteComment(workspaceId: string, objectId: string, commentId: string) {
  return kival.deleteComment({ workspaceId, objectId, commentId });
}

export function resolveCommentThread(workspaceId: string, objectId: string, threadId: string) {
  return kival.resolveCommentThread({ workspaceId, objectId, threadId });
}

export function reopenCommentThread(workspaceId: string, objectId: string, threadId: string) {
  return kival.reopenCommentThread({ workspaceId, objectId, threadId });
}
