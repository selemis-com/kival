import type { Timestamp, UUID } from "./common.js";
import type { ListParams } from "./pagination.js";

/** User lifecycle status. */
export type UserStatus = "active" | "disabled";

/** User status filter for administrative list endpoints. */
export type UserListStatus = UserStatus | "all";

/** User resource. */
export type User = {
  /** User ID. */
  id: UUID;
  /** Username account identifier. */
  username: string;
  /** Display name. */
  display_name: string;
  /** Lifecycle status. */
  status: UserStatus;
  /** Creation timestamp. */
  created_at: Timestamp;
  /** Last update timestamp. */
  updated_at: Timestamp;
  /** Disable timestamp. */
  disabled_at: Timestamp | null;
  /** User that disabled this user. */
  disabled_by: UUID | null;
};

/** User response envelope used by administrative user routes. */
export type UserResponse = {
  /** User resource. */
  user: User;
  /** Whether the authenticated user is a global administrator. */
  is_global_admin?: boolean;
  /** Whether the authenticated user may manage any groups. */
  can_manage_groups?: boolean;
};

/** User collection query parameters. */
export type UserListParams = ListParams & {
  /** User status filter. Defaults to active users. */
  status?: UserListStatus;
  /** Case-insensitive username or display-name search. */
  q?: string | null;
};

/** Request body for updating a user. */
export type UpdateUserRequest = {
  /** New display name. */
  display_name?: string | null;
};
