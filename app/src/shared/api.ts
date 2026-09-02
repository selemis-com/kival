import type {
  FavoriteState,
  InboxEntry,
  InboxListParams,
  InboxUnreadCountResponse,
  InboxUpdatedResponse,
  ListResponse,
  MarkInboxReadRequest,
  ObjectNotificationPreference,
  PinState,
  UpdateInboxEntryRequest,
  UpdateObjectNotificationPreferenceRequest,
} from "kival-sdk";
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

export function setWorkspacePin(workspaceId: string, pinned: boolean, signal?: AbortSignal) {
  return browserTransport.requestJson<PinState>(`/workspaces/${pathId(workspaceId)}/pin`, {
    method: pinned ? "POST" : "DELETE",
    signal,
  });
}

export function setObjectFavorite(
  workspaceId: string,
  objectId: string,
  favorited: boolean,
  signal?: AbortSignal,
) {
  return browserTransport.requestJson<FavoriteState>(
    `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/favorite`,
    { method: favorited ? "POST" : "DELETE", signal },
  );
}

export function setObjectPin(
  workspaceId: string,
  objectId: string,
  pinned: boolean,
  signal?: AbortSignal,
) {
  return browserTransport.requestJson<PinState>(
    `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/pin`,
    { method: pinned ? "POST" : "DELETE", signal },
  );
}

export function getObjectNotificationPreference(
  workspaceId: string,
  objectId: string,
  signal?: AbortSignal,
) {
  return browserTransport.requestJson<ObjectNotificationPreference>(
    `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/notification-preference`,
    { signal },
  );
}

export function updateObjectNotificationPreference(
  workspaceId: string,
  objectId: string,
  input: UpdateObjectNotificationPreferenceRequest,
  signal?: AbortSignal,
) {
  return browserTransport.requestJson<ObjectNotificationPreference>(
    `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/notification-preference`,
    jsonRequest("PATCH", input, signal),
  );
}

export function listInbox(params: InboxListParams = {}, signal?: AbortSignal) {
  const query = listParams(params);
  if (params.unread_only != null) {
    query.set("unread_only", params.unread_only ? "true" : "false");
  }
  if (params.workspace_id != null) {
    query.set("workspace_id", params.workspace_id);
  }
  return browserTransport.requestJson<ListResponse<InboxEntry>>(withParams("/inbox", query), {
    signal,
  });
}

export function getInboxUnreadCount(signal?: AbortSignal) {
  return browserTransport.requestJson<InboxUnreadCountResponse>("/inbox/unread-count", { signal });
}

export function updateInboxEntry(
  inboxEntryId: string,
  input: UpdateInboxEntryRequest,
  signal?: AbortSignal,
) {
  return browserTransport.requestJson<InboxEntry>(
    `/inbox/${pathId(inboxEntryId)}`,
    jsonRequest("PATCH", input, signal),
  );
}

export function markInboxRead(input: MarkInboxReadRequest, signal?: AbortSignal) {
  return browserTransport.requestJson<InboxUpdatedResponse>(
    "/inbox/read",
    jsonRequest("POST", input, signal),
  );
}

const sdkClient = createKivalClient({ transport: browserTransport });

/** Browser-session client used by the Kival web application. */
export const kival = Object.assign(sdkClient, {
  getInboxUnreadCount: ({ signal }: { signal?: AbortSignal } = {}) => getInboxUnreadCount(signal),
  getObjectNotificationPreference: ({
    workspaceId,
    objectId,
    signal,
  }: {
    workspaceId: string;
    objectId: string;
    signal?: AbortSignal;
  }) => getObjectNotificationPreference(workspaceId, objectId, signal),
  listInbox: ({ signal, ...params }: InboxListParams & { signal?: AbortSignal } = {}) =>
    listInbox(params, signal),
  markInboxRead: ({ input, signal }: { input: MarkInboxReadRequest; signal?: AbortSignal }) =>
    markInboxRead(input, signal),
  setObjectFavorite: ({
    workspaceId,
    objectId,
    favorited,
    signal,
  }: {
    workspaceId: string;
    objectId: string;
    favorited: boolean;
    signal?: AbortSignal;
  }) => setObjectFavorite(workspaceId, objectId, favorited, signal),
  setObjectPin: ({
    workspaceId,
    objectId,
    pinned,
    signal,
  }: {
    workspaceId: string;
    objectId: string;
    pinned: boolean;
    signal?: AbortSignal;
  }) => setObjectPin(workspaceId, objectId, pinned, signal),
  setWorkspacePin: ({
    workspaceId,
    pinned,
    signal,
  }: {
    workspaceId: string;
    pinned: boolean;
    signal?: AbortSignal;
  }) => setWorkspacePin(workspaceId, pinned, signal),
  updateInboxEntry: ({
    inboxEntryId,
    input,
    signal,
  }: {
    inboxEntryId: string;
    input: UpdateInboxEntryRequest;
    signal?: AbortSignal;
  }) => updateInboxEntry(inboxEntryId, input, signal),
  updateObjectNotificationPreference: ({
    workspaceId,
    objectId,
    input,
    signal,
  }: {
    workspaceId: string;
    objectId: string;
    input: UpdateObjectNotificationPreferenceRequest;
    signal?: AbortSignal;
  }) => updateObjectNotificationPreference(workspaceId, objectId, input, signal),
});

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
