import { jsonRequest, listParams, pathId, requestInit, withParams } from "../internal/utils.js";
import type {
  CreateObjectRequest,
  FavoriteState,
  ListParams,
  ListResponse,
  ObjectAttachment,
  ObjectAttachmentResponse,
  ObjectBacklinksParams,
  ObjectBacklinksResponse,
  ObjectListItem,
  ObjectListParams,
  ObjectResponse,
  ObjectVersion,
  ObjectVersionResponse,
  PinState,
  ReuseObjectAttachmentRequest,
  UpdateObjectRequest,
  UploadObjectAttachmentParams,
  UUID,
} from "../types.js";
import { setBoolean, setNumber, setString } from "./params.js";
import type { KivalClientBase, KivalResponseClient, WithSignal } from "./types.js";

/** Parameters for {@link listObjects}. */
export type ListObjectsParameters = WithSignal<
  ObjectListParams & {
    workspaceId: UUID;
  }
>;

/** Return type for {@link listObjects}. */
export type ListObjectsReturnType = ListResponse<ObjectListItem>;

/** Lists objects in a workspace. */
export function listObjects(
  client: KivalClientBase,
  parameters: ListObjectsParameters,
): Promise<ListObjectsReturnType> {
  const { workspaceId, ...options } = parameters;
  const params = listParams(options);
  params.set("status", options.status ?? "active");
  params.set("order", options.order ?? "created");
  setBoolean(params, "favorited", options.favorited);
  setBoolean(params, "pinned", options.pinned);
  return client.requestJson<ListObjectsReturnType>(
    withParams(`/workspaces/${pathId(workspaceId)}/objects`, params),
    requestInit({}, options.signal),
  );
}

/** Sets or clears a personal object favorite. */
export function setObjectFavorite(
  client: KivalClientBase,
  parameters: WithSignal<{ workspaceId: UUID; objectId: UUID; favorited: boolean }>,
): Promise<FavoriteState> {
  const { workspaceId, objectId, favorited, signal } = parameters;
  return client.requestJson<FavoriteState>(
    `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/favorite`,
    requestInit({ method: favorited ? "POST" : "DELETE" }, signal),
  );
}

/** Sets or clears a personal object pin. */
export function setObjectPin(
  client: KivalClientBase,
  parameters: WithSignal<{ workspaceId: UUID; objectId: UUID; pinned: boolean }>,
): Promise<PinState> {
  const { workspaceId, objectId, pinned, signal } = parameters;
  return client.requestJson<PinState>(
    `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/pin`,
    requestInit({ method: pinned ? "POST" : "DELETE" }, signal),
  );
}

/** Parameters for {@link getObject}. */
export type GetObjectParameters = WithSignal<{
  workspaceId: UUID;
  objectId: UUID;
}>;

/** Return type for {@link getObject}. */
export type GetObjectReturnType = ObjectResponse;

/** Gets an object by ID. */
export function getObject(
  client: KivalClientBase,
  parameters: GetObjectParameters,
): Promise<GetObjectReturnType> {
  const { workspaceId, objectId, signal } = parameters;
  return client.requestJson<ObjectResponse>(
    `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}`,
    requestInit({}, signal),
  );
}

/** Parameters for {@link createObject}. */
export type CreateObjectParameters = WithSignal<{
  workspaceId: UUID;
  input: CreateObjectRequest;
}>;

/** Return type for {@link createObject}. */
export type CreateObjectReturnType = ObjectResponse;

/** Creates an object and its initial version. */
export function createObject(
  client: KivalClientBase,
  parameters: CreateObjectParameters,
): Promise<CreateObjectReturnType> {
  const { workspaceId, input, signal } = parameters;
  return client.requestJson<ObjectResponse>(
    `/workspaces/${pathId(workspaceId)}/objects`,
    jsonRequest("POST", input, signal),
  );
}

/** Parameters for {@link updateObject}. */
export type UpdateObjectParameters = WithSignal<{
  workspaceId: UUID;
  objectId: UUID;
  input: UpdateObjectRequest;
}>;

/** Return type for {@link updateObject}. */
export type UpdateObjectReturnType = ObjectResponse;

/** Updates an object, creating a new current version only when state changes. */
export function updateObject(
  client: KivalClientBase,
  parameters: UpdateObjectParameters,
): Promise<UpdateObjectReturnType> {
  const { workspaceId, objectId, input, signal } = parameters;
  return client.requestJson<ObjectResponse>(
    `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}`,
    jsonRequest("PATCH", input, signal),
  );
}

/** Parameters for {@link archiveObject}. */
export type ArchiveObjectParameters = WithSignal<{
  workspaceId: UUID;
  objectId: UUID;
}>;

/** Return type for {@link archiveObject}. */
export type ArchiveObjectReturnType = ObjectResponse;

/** Archives an object. */
export function archiveObject(
  client: KivalClientBase,
  parameters: ArchiveObjectParameters,
): Promise<ArchiveObjectReturnType> {
  const { workspaceId, objectId, signal } = parameters;
  return client.requestJson<ObjectResponse>(
    `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/archive`,
    requestInit({ method: "POST" }, signal),
  );
}

/** Parameters for {@link unarchiveObject}. */
export type UnarchiveObjectParameters = WithSignal<{
  workspaceId: UUID;
  objectId: UUID;
}>;

/** Return type for {@link unarchiveObject}. */
export type UnarchiveObjectReturnType = ObjectResponse;

/** Unarchives an object. */
export function unarchiveObject(
  client: KivalClientBase,
  parameters: UnarchiveObjectParameters,
): Promise<UnarchiveObjectReturnType> {
  const { workspaceId, objectId, signal } = parameters;
  return client.requestJson<ObjectResponse>(
    `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/unarchive`,
    requestInit({ method: "POST" }, signal),
  );
}

/** Parameters for {@link uploadObjectAttachment}. */
export type UploadObjectAttachmentParameters = WithSignal<{
  workspaceId: UUID;
  objectId: UUID;
  params: UploadObjectAttachmentParams;
  body: BodyInit;
}>;

/** Return type for {@link uploadObjectAttachment}. */
export type UploadObjectAttachmentReturnType = ObjectAttachment;

/**
 * Uploads a body supported by the runtime's Fetch API and creates an object attachment.
 *
 * Use a streaming `BodyInit` for large or otherwise non-buffered uploads.
 */
export function uploadObjectAttachment(
  client: KivalClientBase,
  parameters: UploadObjectAttachmentParameters,
): Promise<UploadObjectAttachmentReturnType> {
  const { workspaceId, objectId, params, body, signal } = parameters;
  const searchParams = new URLSearchParams();
  setString(searchParams, "version_id", params.version_id);
  setString(searchParams, "name", params.name);
  setString(searchParams, "media_type", params.media_type);
  setString(searchParams, "metadata", params.metadata);

  return client
    .requestJson<ObjectAttachmentResponse>(
      withParams(
        `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/attachments/upload`,
        searchParams,
      ),
      requestInit(
        {
          method: "POST",
          ...(params.media_type ? { headers: { "content-type": params.media_type } } : {}),
          body,
        },
        signal,
      ),
    )
    .then((response) => response.attachment);
}

/** Parameters for {@link listObjectAttachments}. */
export type ListObjectAttachmentsParameters = WithSignal<
  ListParams & {
    workspaceId: UUID;
    objectId: UUID;
  }
>;

/** Return type for {@link listObjectAttachments}. */
export type ListObjectAttachmentsReturnType = ListResponse<ObjectAttachment>;

/** Lists object attachments. */
export function listObjectAttachments(
  client: KivalClientBase,
  parameters: ListObjectAttachmentsParameters,
): Promise<ListObjectAttachmentsReturnType> {
  const { workspaceId, objectId, ...options } = parameters;
  return client.requestJson<ListObjectAttachmentsReturnType>(
    withParams(
      `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/attachments`,
      listParams(options),
    ),
    requestInit({}, options.signal),
  );
}

/** Parameters for {@link getObjectAttachment}. */
export type GetObjectAttachmentParameters = WithSignal<{
  workspaceId: UUID;
  objectId: UUID;
  attachmentId: UUID;
}>;

/** Return type for {@link getObjectAttachment}. */
export type GetObjectAttachmentReturnType = ObjectAttachment;

/** Gets object-attachment metadata by ID. */
export function getObjectAttachment(
  client: KivalClientBase,
  parameters: GetObjectAttachmentParameters,
): Promise<GetObjectAttachmentReturnType> {
  const { workspaceId, objectId, attachmentId, signal } = parameters;
  return client
    .requestJson<ObjectAttachmentResponse>(
      `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}` +
        `/attachments/${pathId(attachmentId)}`,
      requestInit({}, signal),
    )
    .then((response) => response.attachment);
}

/** Parameters for {@link reuseObjectAttachment}. */
export type ReuseObjectAttachmentParameters = WithSignal<{
  workspaceId: UUID;
  objectId: UUID;
  input: ReuseObjectAttachmentRequest;
}>;

/** Return type for {@link reuseObjectAttachment}. */
export type ReuseObjectAttachmentReturnType = ObjectAttachment;

/** Creates an attachment by reusing an authorized source attachment. */
export function reuseObjectAttachment(
  client: KivalClientBase,
  parameters: ReuseObjectAttachmentParameters,
): Promise<ReuseObjectAttachmentReturnType> {
  const { workspaceId, objectId, input, signal } = parameters;
  return client
    .requestJson<ObjectAttachmentResponse>(
      `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/attachments/reuse`,
      jsonRequest("POST", input, signal),
    )
    .then((response) => response.attachment);
}

/** Parameters for {@link getObjectAttachmentContent}. */
export type GetObjectAttachmentContentParameters = WithSignal<{
  workspaceId: UUID;
  objectId: UUID;
  attachmentId: UUID;
}>;

/** Return type for {@link getObjectAttachmentContent}. */
export type GetObjectAttachmentContentReturnType = Uint8Array;

/**
 * Fetches object-attachment content as bytes.
 *
 * This convenience action buffers the complete response. Use
 * {@link getObjectAttachmentContentResponse} to consume large bodies incrementally.
 */
export function getObjectAttachmentContent(
  client: KivalResponseClient,
  parameters: GetObjectAttachmentContentParameters,
): Promise<GetObjectAttachmentContentReturnType> {
  const { workspaceId, objectId, attachmentId, signal } = parameters;
  return client.requestBytes(
    `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}` +
      `/attachments/${pathId(attachmentId)}/content`,
    requestInit({}, signal),
  );
}

/** Parameters for {@link getObjectAttachmentContentResponse}. */
export type GetObjectAttachmentContentResponseParameters = GetObjectAttachmentContentParameters;

/** Return type for {@link getObjectAttachmentContentResponse}. */
export type GetObjectAttachmentContentResponseReturnType = Response;

/**
 * Fetches an object-attachment response without buffering its body.
 *
 * The transport timeout covers receiving the response headers. Body-stream errors after this
 * action resolves are reported by the runtime's Fetch API.
 */
export function getObjectAttachmentContentResponse(
  client: KivalResponseClient,
  parameters: GetObjectAttachmentContentResponseParameters,
): Promise<GetObjectAttachmentContentResponseReturnType> {
  const { workspaceId, objectId, attachmentId, signal } = parameters;
  return client.requestResponse(
    `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}` +
      `/attachments/${pathId(attachmentId)}/content`,
    requestInit({}, signal),
  );
}

/** Parameters for {@link listObjectVersions}. */
export type ListObjectVersionsParameters = WithSignal<
  ListParams & {
    workspaceId: UUID;
    objectId: UUID;
  }
>;

/** Return type for {@link listObjectVersions}. */
export type ListObjectVersionsReturnType = ListResponse<ObjectVersion>;

/** Lists object versions. */
export function listObjectVersions(
  client: KivalClientBase,
  parameters: ListObjectVersionsParameters,
): Promise<ListObjectVersionsReturnType> {
  const { workspaceId, objectId, ...options } = parameters;
  return client.requestJson<ListObjectVersionsReturnType>(
    withParams(
      `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/versions`,
      listParams(options),
    ),
    requestInit({}, options.signal),
  );
}

/** Parameters for {@link getObjectVersion}. */
export type GetObjectVersionParameters = WithSignal<{
  workspaceId: UUID;
  objectId: UUID;
  version: UUID | number;
}>;

/** Return type for {@link getObjectVersion}. */
export type GetObjectVersionReturnType = ObjectVersion;

/** Gets an object version by immutable ID or monotonic version number. */
export function getObjectVersion(
  client: KivalClientBase,
  parameters: GetObjectVersionParameters,
): Promise<GetObjectVersionReturnType> {
  const { workspaceId, objectId, version, signal } = parameters;
  if (typeof version === "number" && (!Number.isSafeInteger(version) || version < 1)) {
    throw new TypeError("version must be a positive safe integer");
  }

  return client
    .requestJson<ObjectVersionResponse>(
      `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/versions/${pathId(version.toString())}`,
      requestInit({}, signal),
    )
    .then((response) => response.version);
}

/** Parameters for {@link getObjectBacklinks}. */
export type GetObjectBacklinksParameters = WithSignal<
  ObjectBacklinksParams & {
    workspaceId: UUID;
    objectId: UUID;
  }
>;

/** Return type for {@link getObjectBacklinks}. */
export type GetObjectBacklinksReturnType = ObjectBacklinksResponse;

/** Lists visible inbound explicit edges and textual references for an object. */
export function getObjectBacklinks(
  client: KivalClientBase,
  parameters: GetObjectBacklinksParameters,
): Promise<GetObjectBacklinksReturnType> {
  const { workspaceId, objectId, ...options } = parameters;
  const params = new URLSearchParams();
  setNumber(params, "limit", options.limit);
  setString(params, "edge_cursor", options.edge_cursor);
  setString(params, "reference_cursor", options.reference_cursor);
  if (options.include_archived) params.set("include_archived", "true");
  return client.requestJson<ObjectBacklinksResponse>(
    withParams(`/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/backlinks`, params),
    requestInit({}, options.signal),
  );
}
