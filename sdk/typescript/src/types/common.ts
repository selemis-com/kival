/** UUID serialized by the Kival API. */
export type UUID = string;

/** Actor-relative state returned after changing a personal favorite. */
export type FavoriteState = { favorited: boolean };

/** Actor-relative state returned after changing a personal pin. */
export type PinState = { pinned: boolean; pinned_at: Timestamp | null };

/** Reference to a user by exactly one stable identifier. */
export type UserReference =
  | {
      /** Member user ID. */
      user_id: UUID;
      /** Username must be omitted when `user_id` is supplied. */
      username?: never;
    }
  | {
      /** User ID must be omitted when `username` is supplied. */
      user_id?: never;
      /** Account username. */
      username: string;
    };

/** RFC 3339 timestamp serialized by the Kival API. */
export type Timestamp = string;

/** JSON-compatible value. */
export type JsonValue = null | boolean | number | string | JsonValue[] | JsonObject;

/** JSON object with JSON-compatible values. */
export type JsonObject = { [key: string]: JsonValue };

/** JSON scalar allowed as a Kival metadata value. */
export type MetadataScalar = null | boolean | number | string;

/** Value allowed for one top-level Kival metadata key. */
export type MetadataValue = MetadataScalar | MetadataScalar[];

/** Flat Kival metadata object. */
export type FlatMetadata = { [key: string]: MetadataValue };

/** Archive lifecycle status shared by archivable resources. */
export type ArchiveStatus = "active" | "archived";

/** Archive status filter for list endpoints. */
export type ArchiveListStatus = ArchiveStatus | "all";

/**
 * JSON representation of Rust's tri-state `PatchField<T>`.
 *
 * On an optional request property, omission leaves the field unchanged, `null` clears it, and a
 * concrete value replaces it.
 */
export type PatchField<T> = T | null;
