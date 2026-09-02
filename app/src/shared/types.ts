import type {
  ListResponse,
  ObjectBacklinksResponse,
  ObjectEdge,
  ObjectGraphResponse,
  ObjectListItem,
  ObjectResource,
  ObjectResponse,
  ObjectVersion,
} from "kival-sdk";

export type * from "kival-sdk";
export type * from "./auth/types";

export type AuthState = "checking" | "authenticated" | "anonymous";

/** Object response after the UI has established that a current version is present. */
export type CurrentObjectVersion = ObjectVersion;

export type CurrentObjectResponse = Omit<ObjectResponse, "current_version"> & {
  current_version: CurrentObjectVersion;
};

/** Compact view model used by the workspace directory. */
export type ObjectSummary = Pick<ObjectResource, "id" | "title" | "status" | "updated_at"> &
  Partial<
    Pick<
      ObjectListItem,
      | "updated_by_username"
      | "updated_by_display_name"
      | "updated_by_workspace_role"
      | "updated_by_object_role"
      | "connection_count"
      | "unresolved_thread_count"
      | "favorited"
      | "pinned"
      | "pinned_at"
    >
  >;

export type RecentObject = ObjectSummary;

export type ObjectContext = {
  backlinks: ObjectBacklinksResponse;
  edges: ListResponse<ObjectEdge>;
  graph: ObjectGraphResponse;
};
