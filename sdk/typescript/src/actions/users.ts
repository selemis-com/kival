import { jsonRequest, pathId, requestInit, withParams } from "../internal/utils.js";
import type {
  ListResponse,
  UpdateUserRequest,
  User,
  UserListParams,
  UserResponse,
  UUID,
} from "../types.js";
import { searchableListParams } from "./params.js";
import type { KivalClientBase, WithSignal } from "./types.js";

/** Parameters for {@link listUsers}. */
export type ListUsersParameters = WithSignal<UserListParams>;

/** Return type for {@link listUsers}. */
export type ListUsersReturnType = ListResponse<User>;

/** Lists users. */
export function listUsers(
  client: KivalClientBase,
  parameters: ListUsersParameters = {},
): Promise<ListUsersReturnType> {
  const params = searchableListParams(parameters);
  params.set("status", parameters.status ?? "active");
  return client.requestJson<ListUsersReturnType>(
    withParams("/users", params),
    requestInit({}, parameters.signal),
  );
}

/** Parameters for {@link getUser}. */
export type GetUserParameters = WithSignal<{
  userId: UUID;
}>;

/** Return type for {@link getUser}. */
export type GetUserReturnType = User;

/** Gets a user by ID. */
export function getUser(
  client: KivalClientBase,
  parameters: GetUserParameters,
): Promise<GetUserReturnType> {
  const { userId, signal } = parameters;
  return client
    .requestJson<UserResponse>(`/users/${pathId(userId)}`, requestInit({}, signal))
    .then((response) => response.user);
}

/** Parameters for {@link updateUser}. */
export type UpdateUserParameters = WithSignal<{
  userId: UUID;
  input: UpdateUserRequest;
}>;

/** Return type for {@link updateUser}. */
export type UpdateUserReturnType = User;

/** Updates a user. */
export function updateUser(
  client: KivalClientBase,
  parameters: UpdateUserParameters,
): Promise<UpdateUserReturnType> {
  const { userId, input, signal } = parameters;
  return client
    .requestJson<UserResponse>(`/users/${pathId(userId)}`, jsonRequest("PATCH", input, signal))
    .then((response) => response.user);
}

/** Parameters for {@link disableUser}. */
export type DisableUserParameters = WithSignal<{
  userId: UUID;
}>;

/** Return type for {@link disableUser}. */
export type DisableUserReturnType = User;

/** Disables a user. */
export function disableUser(
  client: KivalClientBase,
  parameters: DisableUserParameters,
): Promise<DisableUserReturnType> {
  const { userId, signal } = parameters;
  return client
    .requestJson<UserResponse>(
      `/users/${pathId(userId)}/disable`,
      requestInit({ method: "POST" }, signal),
    )
    .then((response) => response.user);
}

/** Parameters for {@link enableUser}. */
export type EnableUserParameters = WithSignal<{
  userId: UUID;
}>;

/** Return type for {@link enableUser}. */
export type EnableUserReturnType = User;

/** Enables a disabled user without changing their credentials or access. */
export function enableUser(
  client: KivalClientBase,
  parameters: EnableUserParameters,
): Promise<EnableUserReturnType> {
  const { userId, signal } = parameters;
  return client
    .requestJson<UserResponse>(
      `/users/${pathId(userId)}/enable`,
      requestInit({ method: "POST" }, signal),
    )
    .then((response) => response.user);
}
