import { jsonRequest, pathId, requestInit, withParams } from "../internal/utils.js";
import type {
  InboxEntry,
  InboxListParams,
  InboxUnreadCountResponse,
  InboxUpdatedResponse,
  ListResponse,
  MarkInboxReadRequest,
  ObjectNotificationPreference,
  UpdateInboxEntryRequest,
  UpdateObjectNotificationPreferenceRequest,
  UUID,
} from "../types.js";
import { setBoolean, setNumber, setString } from "./params.js";
import type { KivalClientBase, WithSignal } from "./types.js";

/** Parameters for {@link getObjectNotificationPreference}. */
export type GetObjectNotificationPreferenceParameters = WithSignal<{
  workspaceId: UUID;
  objectId: UUID;
}>;

/** Gets the effective notification preference for one object. */
export function getObjectNotificationPreference(
  client: KivalClientBase,
  parameters: GetObjectNotificationPreferenceParameters,
): Promise<ObjectNotificationPreference> {
  const { workspaceId, objectId, signal } = parameters;
  return client.requestJson<ObjectNotificationPreference>(
    `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/notification-preference`,
    requestInit({}, signal),
  );
}

/** Parameters for {@link updateObjectNotificationPreference}. */
export type UpdateObjectNotificationPreferenceParameters = WithSignal<{
  workspaceId: UUID;
  objectId: UUID;
  input: UpdateObjectNotificationPreferenceRequest;
}>;

/** Changes the explicit notification preference for one object. */
export function updateObjectNotificationPreference(
  client: KivalClientBase,
  parameters: UpdateObjectNotificationPreferenceParameters,
): Promise<ObjectNotificationPreference> {
  const { workspaceId, objectId, input, signal } = parameters;
  return client.requestJson<ObjectNotificationPreference>(
    `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/notification-preference`,
    jsonRequest("PATCH", input, signal),
  );
}

/** Parameters for {@link listInbox}. */
export type ListInboxParameters = WithSignal<InboxListParams>;

/** Lists currently authorized personal inbox entries. */
export function listInbox(
  client: KivalClientBase,
  parameters: ListInboxParameters = {},
): Promise<ListResponse<InboxEntry>> {
  const params = new URLSearchParams();
  setNumber(params, "limit", parameters.limit);
  setString(params, "cursor", parameters.cursor);
  setBoolean(params, "unread_only", parameters.unread_only);
  setString(params, "workspace_id", parameters.workspace_id);
  return client.requestJson<ListResponse<InboxEntry>>(
    withParams("/inbox", params),
    requestInit({}, parameters.signal),
  );
}

/** Gets the current unread inbox count. */
export function getInboxUnreadCount(
  client: KivalClientBase,
  parameters: WithSignal = {},
): Promise<InboxUnreadCountResponse> {
  return client.requestJson<InboxUnreadCountResponse>(
    "/inbox/unread-count",
    requestInit({}, parameters.signal),
  );
}

/** Parameters for {@link updateInboxEntry}. */
export type UpdateInboxEntryParameters = WithSignal<{
  inboxEntryId: UUID;
  input: UpdateInboxEntryRequest;
}>;

/** Changes one inbox entry's read state. */
export function updateInboxEntry(
  client: KivalClientBase,
  parameters: UpdateInboxEntryParameters,
): Promise<InboxEntry> {
  const { inboxEntryId, input, signal } = parameters;
  return client.requestJson<InboxEntry>(
    `/inbox/${pathId(inboxEntryId)}`,
    jsonRequest("PATCH", input, signal),
  );
}

/** Parameters for {@link markInboxRead}. */
export type MarkInboxReadParameters = WithSignal<{
  input: MarkInboxReadRequest;
}>;

/** Marks currently authorized inbox entries read. */
export function markInboxRead(
  client: KivalClientBase,
  parameters: MarkInboxReadParameters,
): Promise<InboxUpdatedResponse> {
  const { input, signal } = parameters;
  return client.requestJson<InboxUpdatedResponse>(
    "/inbox/read",
    jsonRequest("POST", input, signal),
  );
}
