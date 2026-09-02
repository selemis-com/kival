import { pathId, requestInit, withParams } from "../internal/utils.js";
import type { Event, EventListParams, ListResponse, UUID } from "../types.js";
import { setNumber, setString } from "./params.js";
import type { KivalClientBase, WithSignal } from "./types.js";

/** Parameters for {@link listObjectEvents}. */
export type ListObjectEventsParameters = WithSignal<
  EventListParams & {
    workspaceId: UUID;
    objectId: UUID;
  }
>;

/** Return type for {@link listObjectEvents}. */
export type ListObjectEventsReturnType = ListResponse<Event>;

/** Lists events for an object. */
export function listObjectEvents(
  client: KivalClientBase,
  parameters: ListObjectEventsParameters,
): Promise<ListObjectEventsReturnType> {
  const { workspaceId, objectId, ...options } = parameters;
  return client.requestJson<ListObjectEventsReturnType>(
    withParams(
      `/workspaces/${pathId(workspaceId)}/objects/${pathId(objectId)}/events`,
      eventParams(options),
    ),
    requestInit({}, options.signal),
  );
}

/** Parameters for {@link listWorkspaceEvents}. */
export type ListWorkspaceEventsParameters = WithSignal<
  EventListParams & {
    workspaceId: UUID;
  }
>;

/** Return type for {@link listWorkspaceEvents}. */
export type ListWorkspaceEventsReturnType = ListResponse<Event>;

/** Lists events in a workspace. */
export function listWorkspaceEvents(
  client: KivalClientBase,
  parameters: ListWorkspaceEventsParameters,
): Promise<ListWorkspaceEventsReturnType> {
  const { workspaceId, ...options } = parameters;
  return client.requestJson<ListWorkspaceEventsReturnType>(
    withParams(`/workspaces/${pathId(workspaceId)}/events`, eventParams(options)),
    requestInit({}, options.signal),
  );
}

/** Parameters for {@link listEvents}. */
export type ListEventsParameters = WithSignal<EventListParams>;

/** Return type for {@link listEvents}. */
export type ListEventsReturnType = ListResponse<Event>;

/** Lists global events. */
export function listEvents(
  client: KivalClientBase,
  parameters: ListEventsParameters = {},
): Promise<ListEventsReturnType> {
  return client.requestJson<ListEventsReturnType>(
    withParams("/events", eventParams(parameters)),
    requestInit({}, parameters.signal),
  );
}

function eventParams(options: WithSignal<EventListParams>) {
  const params = new URLSearchParams();
  setNumber(params, "limit", options.limit);
  setNumber(params, "after_sequence", options.after_sequence);
  setNumber(params, "before_sequence", options.before_sequence);
  if (options.order === "desc") params.set("order", "desc");
  setString(params, "event_kind", options.event_kind);
  setString(params, "actor_user_id", options.actor_user_id);
  setString(params, "target_user_id", options.target_user_id);
  setString(params, "object_id", options.object_id);
  setString(params, "group_id", options.group_id);
  return params;
}
