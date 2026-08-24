import { jsonRequest, listParams, pathId, requestInit, withParams } from "../internal/utils.js";
import type {
  CreateGroupMembershipRequest,
  CreateGroupRequest,
  Group,
  GroupListParams,
  GroupMembership,
  GroupMembershipResponse,
  GroupResponse,
  ListParams,
  ListResponse,
  UpdateGroupMembershipRequest,
  UpdateGroupRequest,
  UUID,
} from "../types.js";
import { searchableListParams } from "./params.js";
import type { KivalClientBase, WithSignal } from "./types.js";

/** Parameters for {@link createGroup}. */
export type CreateGroupParameters = WithSignal<{
  input: CreateGroupRequest;
}>;

/** Return type for {@link createGroup}. */
export type CreateGroupReturnType = Group;

/** Creates a group. */
export function createGroup(
  client: KivalClientBase,
  parameters: CreateGroupParameters,
): Promise<CreateGroupReturnType> {
  const { input, signal } = parameters;
  return client
    .requestJson<GroupResponse>("/groups", jsonRequest("POST", input, signal))
    .then((response) => response.group);
}

/** Parameters for {@link listGroups}. */
export type ListGroupsParameters = WithSignal<GroupListParams>;

/** Return type for {@link listGroups}. */
export type ListGroupsReturnType = ListResponse<Group>;

/** Lists groups. */
export function listGroups(
  client: KivalClientBase,
  parameters: ListGroupsParameters = {},
): Promise<ListGroupsReturnType> {
  const params = searchableListParams(parameters);
  params.set("status", parameters.status ?? "active");
  return client.requestJson<ListGroupsReturnType>(
    withParams("/groups", params),
    requestInit({}, parameters.signal),
  );
}

/** Parameters for {@link getGroup}. */
export type GetGroupParameters = WithSignal<{
  groupId: UUID;
}>;

/** Return type for {@link getGroup}. */
export type GetGroupReturnType = Group;

/** Gets a group by ID. */
export function getGroup(
  client: KivalClientBase,
  parameters: GetGroupParameters,
): Promise<GetGroupReturnType> {
  const { groupId, signal } = parameters;
  return client
    .requestJson<GroupResponse>(`/groups/${pathId(groupId)}`, requestInit({}, signal))
    .then((response) => response.group);
}

/** Parameters for {@link updateGroup}. */
export type UpdateGroupParameters = WithSignal<{
  groupId: UUID;
  input: UpdateGroupRequest;
}>;

/** Return type for {@link updateGroup}. */
export type UpdateGroupReturnType = Group;

/**
 * Updates a group.
 *
 * Omitting `description` leaves it unchanged, `null` clears it, and a string replaces it.
 */
export function updateGroup(
  client: KivalClientBase,
  parameters: UpdateGroupParameters,
): Promise<UpdateGroupReturnType> {
  const { groupId, input, signal } = parameters;
  return client
    .requestJson<GroupResponse>(`/groups/${pathId(groupId)}`, jsonRequest("PATCH", input, signal))
    .then((response) => response.group);
}

/** Parameters for {@link archiveGroup}. */
export type ArchiveGroupParameters = WithSignal<{
  groupId: UUID;
}>;

/** Return type for {@link archiveGroup}. */
export type ArchiveGroupReturnType = Group;

/** Archives a group. */
export function archiveGroup(
  client: KivalClientBase,
  parameters: ArchiveGroupParameters,
): Promise<ArchiveGroupReturnType> {
  const { groupId, signal } = parameters;
  return client
    .requestJson<GroupResponse>(
      `/groups/${pathId(groupId)}/archive`,
      requestInit({ method: "POST" }, signal),
    )
    .then((response) => response.group);
}

/** Parameters for {@link unarchiveGroup}. */
export type UnarchiveGroupParameters = WithSignal<{
  groupId: UUID;
}>;

/** Return type for {@link unarchiveGroup}. */
export type UnarchiveGroupReturnType = Group;

/** Unarchives a group. */
export function unarchiveGroup(
  client: KivalClientBase,
  parameters: UnarchiveGroupParameters,
): Promise<UnarchiveGroupReturnType> {
  const { groupId, signal } = parameters;
  return client
    .requestJson<GroupResponse>(
      `/groups/${pathId(groupId)}/unarchive`,
      requestInit({ method: "POST" }, signal),
    )
    .then((response) => response.group);
}

/** Parameters for {@link listGroupMemberships}. */
export type ListGroupMembershipsParameters = WithSignal<
  ListParams & {
    groupId: UUID;
  }
>;

/** Return type for {@link listGroupMemberships}. */
export type ListGroupMembershipsReturnType = ListResponse<GroupMembership>;

/** Lists active memberships in a group. */
export function listGroupMemberships(
  client: KivalClientBase,
  parameters: ListGroupMembershipsParameters,
): Promise<ListGroupMembershipsReturnType> {
  const { groupId, ...options } = parameters;
  return client.requestJson<ListGroupMembershipsReturnType>(
    withParams(`/groups/${pathId(groupId)}/memberships`, listParams(options)),
    requestInit({}, options.signal),
  );
}

/** Parameters for {@link createGroupMembership}. */
export type CreateGroupMembershipParameters = WithSignal<{
  groupId: UUID;
  input: CreateGroupMembershipRequest;
}>;

/** Return type for {@link createGroupMembership}. */
export type CreateGroupMembershipReturnType = GroupMembership;

/** Creates a group membership. */
export function createGroupMembership(
  client: KivalClientBase,
  parameters: CreateGroupMembershipParameters,
): Promise<CreateGroupMembershipReturnType> {
  const { groupId, input, signal } = parameters;
  return client
    .requestJson<GroupMembershipResponse>(
      `/groups/${pathId(groupId)}/memberships`,
      jsonRequest("POST", input, signal),
    )
    .then((response) => response.membership);
}

/** Parameters for {@link updateGroupMembership}. */
export type UpdateGroupMembershipParameters = WithSignal<{
  groupId: UUID;
  membershipId: UUID;
  input: UpdateGroupMembershipRequest;
}>;

/** Return type for {@link updateGroupMembership}. */
export type UpdateGroupMembershipReturnType = GroupMembership;

/** Updates an active group membership's role. */
export function updateGroupMembership(
  client: KivalClientBase,
  parameters: UpdateGroupMembershipParameters,
): Promise<UpdateGroupMembershipReturnType> {
  const { groupId, membershipId, input, signal } = parameters;
  return client
    .requestJson<GroupMembershipResponse>(
      `/groups/${pathId(groupId)}/memberships/${pathId(membershipId)}`,
      jsonRequest("PATCH", input, signal),
    )
    .then((response) => response.membership);
}

/** Parameters for {@link revokeGroupMembership}. */
export type RevokeGroupMembershipParameters = WithSignal<{
  groupId: UUID;
  membershipId: UUID;
}>;

/** Return type for {@link revokeGroupMembership}. */
export type RevokeGroupMembershipReturnType = GroupMembership;

/** Revokes a group membership. */
export function revokeGroupMembership(
  client: KivalClientBase,
  parameters: RevokeGroupMembershipParameters,
): Promise<RevokeGroupMembershipReturnType> {
  const { groupId, membershipId, signal } = parameters;
  return client
    .requestJson<GroupMembershipResponse>(
      `/groups/${pathId(groupId)}/memberships/${pathId(membershipId)}/revoke`,
      requestInit({ method: "POST" }, signal),
    )
    .then((response) => response.membership);
}
