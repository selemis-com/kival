import { jsonRequest, listParams, pathId, requestInit, withParams } from "../internal/utils.js";
import type {
  CreateWorkspaceGroupRequest,
  CreateWorkspaceMembershipRequest,
  ListParams,
  ListResponse,
  PinState,
  UpdateWorkspaceMembershipRequest,
  UpdateWorkspaceRequest,
  UUID,
  Workspace,
  WorkspaceGroup,
  WorkspaceGroupListParams,
  WorkspaceGroupResponse,
  WorkspaceListItem,
  WorkspaceListParams,
  WorkspaceMembership,
  WorkspaceMembershipResponse,
  WorkspaceResponse,
} from "../types.js";
import { searchableListParams, setBoolean } from "./params.js";
import type { KivalClientBase, WithSignal } from "./types.js";

/** Parameters for {@link listWorkspaces}. */
export type ListWorkspacesParameters = WithSignal<WorkspaceListParams>;

/** Return type for {@link listWorkspaces}. */
export type ListWorkspacesReturnType = ListResponse<WorkspaceListItem>;

/** Lists workspaces visible to the authenticated user. */
export function listWorkspaces(
  client: KivalClientBase,
  parameters: ListWorkspacesParameters = {},
): Promise<ListWorkspacesReturnType> {
  const params = searchableListParams(parameters);
  params.set("status", parameters.status ?? "active");
  setBoolean(params, "pinned", parameters.pinned);
  return client.requestJson<ListWorkspacesReturnType>(
    withParams("/workspaces", params),
    requestInit({}, parameters.signal),
  );
}

/** Sets or clears a personal workspace pin. */
export function setWorkspacePin(
  client: KivalClientBase,
  parameters: WithSignal<{ workspaceId: UUID; pinned: boolean }>,
): Promise<PinState> {
  const { workspaceId, pinned, signal } = parameters;
  return client.requestJson<PinState>(
    `/workspaces/${pathId(workspaceId)}/pin`,
    requestInit({ method: pinned ? "POST" : "DELETE" }, signal),
  );
}

/** Parameters for {@link getWorkspace}. */
export type GetWorkspaceParameters = WithSignal<{
  workspaceId: UUID;
}>;

/** Return type for {@link getWorkspace}. */
export type GetWorkspaceReturnType = Workspace;

/** Gets a workspace by ID. */
export function getWorkspace(
  client: KivalClientBase,
  parameters: GetWorkspaceParameters,
): Promise<GetWorkspaceReturnType> {
  const { workspaceId, signal } = parameters;
  return client
    .requestJson<WorkspaceResponse>(`/workspaces/${pathId(workspaceId)}`, requestInit({}, signal))
    .then((response) => response.workspace);
}

/** Parameters for {@link updateWorkspace}. */
export type UpdateWorkspaceParameters = WithSignal<{
  workspaceId: UUID;
  input: UpdateWorkspaceRequest;
}>;

/** Return type for {@link updateWorkspace}. */
export type UpdateWorkspaceReturnType = Workspace;

/**
 * Updates a workspace.
 *
 * Omitting `description` leaves it unchanged, `null` clears it, and a string replaces it.
 */
export function updateWorkspace(
  client: KivalClientBase,
  parameters: UpdateWorkspaceParameters,
): Promise<UpdateWorkspaceReturnType> {
  const { workspaceId, input, signal } = parameters;
  return client
    .requestJson<WorkspaceResponse>(
      `/workspaces/${pathId(workspaceId)}`,
      jsonRequest("PATCH", input, signal),
    )
    .then((response) => response.workspace);
}

/** Parameters for {@link archiveWorkspace}. */
export type ArchiveWorkspaceParameters = WithSignal<{
  workspaceId: UUID;
}>;

/** Return type for {@link archiveWorkspace}. */
export type ArchiveWorkspaceReturnType = Workspace;

/** Archives a workspace. */
export function archiveWorkspace(
  client: KivalClientBase,
  parameters: ArchiveWorkspaceParameters,
): Promise<ArchiveWorkspaceReturnType> {
  const { workspaceId, signal } = parameters;
  return client
    .requestJson<WorkspaceResponse>(
      `/workspaces/${pathId(workspaceId)}/archive`,
      requestInit({ method: "POST" }, signal),
    )
    .then((response) => response.workspace);
}

/** Parameters for {@link unarchiveWorkspace}. */
export type UnarchiveWorkspaceParameters = WithSignal<{
  workspaceId: UUID;
}>;

/** Return type for {@link unarchiveWorkspace}. */
export type UnarchiveWorkspaceReturnType = Workspace;

/** Unarchives a workspace. */
export function unarchiveWorkspace(
  client: KivalClientBase,
  parameters: UnarchiveWorkspaceParameters,
): Promise<UnarchiveWorkspaceReturnType> {
  const { workspaceId, signal } = parameters;
  return client
    .requestJson<WorkspaceResponse>(
      `/workspaces/${pathId(workspaceId)}/unarchive`,
      requestInit({ method: "POST" }, signal),
    )
    .then((response) => response.workspace);
}

/** Parameters for {@link listWorkspaceMemberships}. */
export type ListWorkspaceMembershipsParameters = WithSignal<
  ListParams & {
    workspaceId: UUID;
  }
>;

/** Return type for {@link listWorkspaceMemberships}. */
export type ListWorkspaceMembershipsReturnType = ListResponse<WorkspaceMembership>;

/** Lists active workspace memberships. */
export function listWorkspaceMemberships(
  client: KivalClientBase,
  parameters: ListWorkspaceMembershipsParameters,
): Promise<ListWorkspaceMembershipsReturnType> {
  const { workspaceId, ...options } = parameters;
  return client.requestJson<ListWorkspaceMembershipsReturnType>(
    withParams(`/workspaces/${pathId(workspaceId)}/memberships`, listParams(options)),
    requestInit({}, options.signal),
  );
}

/** Parameters for {@link createWorkspaceMembership}. */
export type CreateWorkspaceMembershipParameters = WithSignal<{
  workspaceId: UUID;
  input: CreateWorkspaceMembershipRequest;
}>;

/** Return type for {@link createWorkspaceMembership}. */
export type CreateWorkspaceMembershipReturnType = WorkspaceMembership;

/** Creates a workspace membership. */
export function createWorkspaceMembership(
  client: KivalClientBase,
  parameters: CreateWorkspaceMembershipParameters,
): Promise<CreateWorkspaceMembershipReturnType> {
  const { workspaceId, input, signal } = parameters;
  return client
    .requestJson<WorkspaceMembershipResponse>(
      `/workspaces/${pathId(workspaceId)}/memberships`,
      jsonRequest("POST", input, signal),
    )
    .then((response) => response.membership);
}

/** Parameters for {@link updateWorkspaceMembership}. */
export type UpdateWorkspaceMembershipParameters = WithSignal<{
  workspaceId: UUID;
  membershipId: UUID;
  input: UpdateWorkspaceMembershipRequest;
}>;

/** Return type for {@link updateWorkspaceMembership}. */
export type UpdateWorkspaceMembershipReturnType = WorkspaceMembership;

/** Updates an active workspace membership's role. */
export function updateWorkspaceMembership(
  client: KivalClientBase,
  parameters: UpdateWorkspaceMembershipParameters,
): Promise<UpdateWorkspaceMembershipReturnType> {
  const { workspaceId, membershipId, input, signal } = parameters;
  return client
    .requestJson<WorkspaceMembershipResponse>(
      `/workspaces/${pathId(workspaceId)}/memberships/${pathId(membershipId)}`,
      jsonRequest("PATCH", input, signal),
    )
    .then((response) => response.membership);
}

/** Parameters for {@link revokeWorkspaceMembership}. */
export type RevokeWorkspaceMembershipParameters = WithSignal<{
  workspaceId: UUID;
  membershipId: UUID;
}>;

/** Return type for {@link revokeWorkspaceMembership}. */
export type RevokeWorkspaceMembershipReturnType = WorkspaceMembership;

/** Revokes a workspace membership. */
export function revokeWorkspaceMembership(
  client: KivalClientBase,
  parameters: RevokeWorkspaceMembershipParameters,
): Promise<RevokeWorkspaceMembershipReturnType> {
  const { workspaceId, membershipId, signal } = parameters;
  return client
    .requestJson<WorkspaceMembershipResponse>(
      `/workspaces/${pathId(workspaceId)}/memberships/${pathId(membershipId)}/revoke`,
      requestInit({ method: "POST" }, signal),
    )
    .then((response) => response.membership);
}

/** Parameters for {@link listWorkspaceGroups}. */
export type ListWorkspaceGroupsParameters = WithSignal<
  WorkspaceGroupListParams & {
    workspaceId: UUID;
  }
>;

/** Return type for {@link listWorkspaceGroups}. */
export type ListWorkspaceGroupsReturnType = ListResponse<WorkspaceGroup>;

/** Lists workspace-group links. */
export function listWorkspaceGroups(
  client: KivalClientBase,
  parameters: ListWorkspaceGroupsParameters,
): Promise<ListWorkspaceGroupsReturnType> {
  const { workspaceId, ...options } = parameters;
  const params = listParams(options);
  params.set("status", options.status ?? "active");
  return client.requestJson<ListWorkspaceGroupsReturnType>(
    withParams(`/workspaces/${pathId(workspaceId)}/groups`, params),
    requestInit({}, options.signal),
  );
}

/** Parameters for {@link createWorkspaceGroup}. */
export type CreateWorkspaceGroupParameters = WithSignal<{
  workspaceId: UUID;
  input: CreateWorkspaceGroupRequest;
}>;

/** Return type for {@link createWorkspaceGroup}. */
export type CreateWorkspaceGroupReturnType = WorkspaceGroup;

/** Links a group to a workspace. */
export function createWorkspaceGroup(
  client: KivalClientBase,
  parameters: CreateWorkspaceGroupParameters,
): Promise<CreateWorkspaceGroupReturnType> {
  const { workspaceId, input, signal } = parameters;
  return client
    .requestJson<WorkspaceGroupResponse>(
      `/workspaces/${pathId(workspaceId)}/groups`,
      jsonRequest("POST", input, signal),
    )
    .then((response) => response.workspace_group);
}

/** Parameters for {@link archiveWorkspaceGroup}. */
export type ArchiveWorkspaceGroupParameters = WithSignal<{
  workspaceId: UUID;
  groupId: UUID;
}>;

/** Return type for {@link archiveWorkspaceGroup}. */
export type ArchiveWorkspaceGroupReturnType = WorkspaceGroup;

/** Archives a workspace-group link. */
export function archiveWorkspaceGroup(
  client: KivalClientBase,
  parameters: ArchiveWorkspaceGroupParameters,
): Promise<ArchiveWorkspaceGroupReturnType> {
  const { workspaceId, groupId, signal } = parameters;
  return client
    .requestJson<WorkspaceGroupResponse>(
      `/workspaces/${pathId(workspaceId)}/groups/${pathId(groupId)}/archive`,
      requestInit({ method: "POST" }, signal),
    )
    .then((response) => response.workspace_group);
}

/** Parameters for {@link unarchiveWorkspaceGroup}. */
export type UnarchiveWorkspaceGroupParameters = WithSignal<{
  workspaceId: UUID;
  groupId: UUID;
}>;

/** Return type for {@link unarchiveWorkspaceGroup}. */
export type UnarchiveWorkspaceGroupReturnType = WorkspaceGroup;

/** Unarchives a workspace-group link. */
export function unarchiveWorkspaceGroup(
  client: KivalClientBase,
  parameters: UnarchiveWorkspaceGroupParameters,
): Promise<UnarchiveWorkspaceGroupReturnType> {
  const { workspaceId, groupId, signal } = parameters;
  return client
    .requestJson<WorkspaceGroupResponse>(
      `/workspaces/${pathId(workspaceId)}/groups/${pathId(groupId)}/unarchive`,
      requestInit({ method: "POST" }, signal),
    )
    .then((response) => response.workspace_group);
}
