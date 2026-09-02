import { jsonRequest, listParams, pathId, requestInit, withParams } from "../internal/utils.js";
import type {
  CreateObjectEdgeRequest,
  CreateObjectGrantRequest,
  ListParams,
  ListResponse,
  ObjectEdge,
  ObjectEdgeResponse,
  ObjectGrant,
  ObjectGrantResponse,
  ObjectGraphParams,
  ObjectGraphResponse,
  UpdateObjectGrantRequest,
  UUID,
  WorkspaceGraphParams,
  WorkspaceGraphResponse,
} from "../types.js";
import { setNumber } from "./params.js";
import type { KivalClientBase, WithSignal } from "./types.js";

/** Parameters for {@link getWorkspaceGraph}. */
export type GetWorkspaceGraphParameters = WithSignal<
  WorkspaceGraphParams & {
    workspaceId: UUID;
  }
>;

/** Return type for {@link getWorkspaceGraph}. */
export type GetWorkspaceGraphReturnType = WorkspaceGraphResponse;

/** Gets a bounded authorized workspace graph projection. */
export function getWorkspaceGraph(
  client: KivalClientBase,
  parameters: GetWorkspaceGraphParameters,
): Promise<GetWorkspaceGraphReturnType> {
  const { workspaceId, ...options } = parameters;
  const params = new URLSearchParams();
  setNumber(params, "limit_nodes", options.limit_nodes);
  setNumber(params, "limit_edges", options.limit_edges);
  if (options.exclude_isolated) params.set("exclude_isolated", "true");
  return client.requestJson<GetWorkspaceGraphReturnType>(
    withParams(`/workspaces/${pathId(workspaceId)}/graph`, params),
    requestInit({}, options.signal),
  );
}

/** Parameters for {@link getObjectGraph}. */
export type GetObjectGraphParameters = WithSignal<
  ObjectGraphParams & {
    workspaceId: UUID;
    objectId: UUID;
  }
>;

/** Return type for {@link getObjectGraph}. */
export type GetObjectGraphReturnType = ObjectGraphResponse;

/** Gets a bounded authorized graph neighborhood around an object. */
export function getObjectGraph(
  client: KivalClientBase,
  parameters: GetObjectGraphParameters,
): Promise<GetObjectGraphReturnType> {
  const { workspaceId, objectId, ...options } = parameters;
  const params = new URLSearchParams();
  setNumber(params, "depth", options.depth);
  params.set("direction", options.direction ?? "both");
  setNumber(params, "max_nodes", options.max_nodes);
  setNumber(params, "max_edges", options.max_edges);
  if (options.include_root === false) params.set("include_root", "false");
  return client.requestJson<GetObjectGraphReturnType>(
    withParams(`/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/graph`, params),
    requestInit({}, options.signal),
  );
}

/** Parameters for {@link listObjectEdges}. */
export type ListObjectEdgesParameters = WithSignal<
  ListParams & {
    workspaceId: UUID;
    objectId: UUID;
  }
>;

/** Return type for {@link listObjectEdges}. */
export type ListObjectEdgesReturnType = ListResponse<ObjectEdge>;

/** Lists active edges attached to an object. */
export function listObjectEdges(
  client: KivalClientBase,
  parameters: ListObjectEdgesParameters,
): Promise<ListObjectEdgesReturnType> {
  const { workspaceId, objectId, ...options } = parameters;
  return client.requestJson<ListObjectEdgesReturnType>(
    withParams(
      `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/edges`,
      listParams(options),
    ),
    requestInit({}, options.signal),
  );
}

/** Parameters for {@link createObjectEdge}. */
export type CreateObjectEdgeParameters = WithSignal<{
  workspaceId: UUID;
  input: CreateObjectEdgeRequest;
}>;

/** Return type for {@link createObjectEdge}. */
export type CreateObjectEdgeReturnType = ObjectEdge;

/** Creates an object edge. */
export function createObjectEdge(
  client: KivalClientBase,
  parameters: CreateObjectEdgeParameters,
): Promise<CreateObjectEdgeReturnType> {
  const { workspaceId, input, signal } = parameters;
  return client
    .requestJson<ObjectEdgeResponse>(
      `/workspaces/${pathId(workspaceId)}/edges`,
      jsonRequest("POST", input, signal),
    )
    .then((response) => response.edge);
}

/** Parameters for {@link getObjectEdge}. */
export type GetObjectEdgeParameters = WithSignal<{
  workspaceId: UUID;
  edgeId: UUID;
}>;

/** Return type for {@link getObjectEdge}. */
export type GetObjectEdgeReturnType = ObjectEdge;

/** Gets an object edge by ID. */
export function getObjectEdge(
  client: KivalClientBase,
  parameters: GetObjectEdgeParameters,
): Promise<GetObjectEdgeReturnType> {
  const { workspaceId, edgeId, signal } = parameters;
  return client
    .requestJson<ObjectEdgeResponse>(
      `/workspaces/${pathId(workspaceId)}/edges/${pathId(edgeId)}`,
      requestInit({}, signal),
    )
    .then((response) => response.edge);
}

/** Parameters for {@link revokeObjectEdge}. */
export type RevokeObjectEdgeParameters = WithSignal<{
  workspaceId: UUID;
  edgeId: UUID;
}>;

/** Return type for {@link revokeObjectEdge}. */
export type RevokeObjectEdgeReturnType = ObjectEdge;

/** Revokes an object edge. */
export function revokeObjectEdge(
  client: KivalClientBase,
  parameters: RevokeObjectEdgeParameters,
): Promise<RevokeObjectEdgeReturnType> {
  const { workspaceId, edgeId, signal } = parameters;
  return client
    .requestJson<ObjectEdgeResponse>(
      `/workspaces/${pathId(workspaceId)}/edges/${pathId(edgeId)}/revoke`,
      requestInit({ method: "POST" }, signal),
    )
    .then((response) => response.edge);
}

/** Parameters for {@link listObjectGrants}. */
export type ListObjectGrantsParameters = WithSignal<
  ListParams & {
    workspaceId: UUID;
    objectId: UUID;
  }
>;

/** Return type for {@link listObjectGrants}. */
export type ListObjectGrantsReturnType = ListResponse<ObjectGrant>;

/** Lists active grants on an object. */
export function listObjectGrants(
  client: KivalClientBase,
  parameters: ListObjectGrantsParameters,
): Promise<ListObjectGrantsReturnType> {
  const { workspaceId, objectId, ...options } = parameters;
  return client.requestJson<ListObjectGrantsReturnType>(
    withParams(
      `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/grants`,
      listParams(options),
    ),
    requestInit({}, options.signal),
  );
}

/** Parameters for {@link createObjectGrant}. */
export type CreateObjectGrantParameters = WithSignal<{
  workspaceId: UUID;
  objectId: UUID;
  input: CreateObjectGrantRequest;
}>;

/** Return type for {@link createObjectGrant}. */
export type CreateObjectGrantReturnType = ObjectGrant;

/** Creates an object grant. */
export function createObjectGrant(
  client: KivalClientBase,
  parameters: CreateObjectGrantParameters,
): Promise<CreateObjectGrantReturnType> {
  const { workspaceId, objectId, input, signal } = parameters;
  return client
    .requestJson<ObjectGrantResponse>(
      `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/grants`,
      jsonRequest("POST", input, signal),
    )
    .then((response) => response.grant);
}

/** Parameters for {@link updateObjectGrant}. */
export type UpdateObjectGrantParameters = WithSignal<{
  workspaceId: UUID;
  objectId: UUID;
  grantId: UUID;
  input: UpdateObjectGrantRequest;
}>;

/** Return type for {@link updateObjectGrant}. */
export type UpdateObjectGrantReturnType = ObjectGrant;

/** Updates an active object grant's role. */
export function updateObjectGrant(
  client: KivalClientBase,
  parameters: UpdateObjectGrantParameters,
): Promise<UpdateObjectGrantReturnType> {
  const { workspaceId, objectId, grantId, input, signal } = parameters;
  return client
    .requestJson<ObjectGrantResponse>(
      `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/grants/${pathId(grantId)}`,
      jsonRequest("PATCH", input, signal),
    )
    .then((response) => response.grant);
}

/** Parameters for {@link revokeObjectGrant}. */
export type RevokeObjectGrantParameters = WithSignal<{
  workspaceId: UUID;
  objectId: UUID;
  grantId: UUID;
}>;

/** Return type for {@link revokeObjectGrant}. */
export type RevokeObjectGrantReturnType = ObjectGrant;

/** Revokes an object grant. */
export function revokeObjectGrant(
  client: KivalClientBase,
  parameters: RevokeObjectGrantParameters,
): Promise<RevokeObjectGrantReturnType> {
  const { workspaceId, objectId, grantId, signal } = parameters;
  return client
    .requestJson<ObjectGrantResponse>(
      `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}` +
        `/grants/${pathId(grantId)}/revoke`,
      requestInit({ method: "POST" }, signal),
    )
    .then((response) => response.grant);
}
