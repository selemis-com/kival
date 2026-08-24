import type { ArchiveListStatus, ArchiveStatus, FlatMetadata, Timestamp, UUID } from "./common.js";
import type { ObjectRole } from "./graph.js";
import type { ListParams } from "./pagination.js";

/** Object resource. */
export type ObjectResource = {
  /** Object ID. */
  id: UUID;
  /** Parent workspace ID. */
  workspace_id: UUID;
  /** Current version ID. */
  current_version_id: UUID | null;
  /** Title projected from the current immutable version. */
  title: string;
  /** Lifecycle status. */
  status: ArchiveStatus;
  /** User that created this object. */
  created_by: UUID | null;
  /** User that archived this object. */
  archived_by: UUID | null;
  /** Creation timestamp. */
  created_at: Timestamp;
  /** Last update timestamp. */
  updated_at: Timestamp;
  /** Archive timestamp. */
  archived_at: Timestamp | null;
};

/** Object resource enriched with workspace-list information. */
export type ObjectListItem = ObjectResource & {
  /** Username that created the current object version. */
  updated_by_username?: string;
  /** Display name of the user that created the current object version. */
  updated_by_display_name?: string;
  /** Updater's active workspace role. */
  updated_by_workspace_role?: string;
  /** Updater's effective access role for this object. */
  updated_by_object_role?: string;
  /** Number of visible active object connections. */
  connection_count: number;
  /** Number of unresolved, unexpired commentary threads. */
  unresolved_thread_count: number;
  /** Whether the authenticated user has favorited this object. */
  favorited: boolean;
  /** Whether the authenticated user has pinned this object. */
  pinned: boolean;
  /** Time at which the authenticated user pinned this object. */
  pinned_at: Timestamp | null;
};

/** Sort order for object collection queries. */
export type ObjectListOrder = "created" | "updated";

/** Object collection query parameters. */
export type ObjectListParams = ListParams & {
  /** Archive status filter. Defaults to active objects. */
  status?: ArchiveListStatus;
  /** Sort order. Defaults to creation time. */
  order?: ObjectListOrder;
  /** Restricts results by the authenticated user's favorite state. */
  favorited?: boolean;
  /** Restricts results by the authenticated user's personal pin state. */
  pinned?: boolean;
};

/** Object version summary. */
export type ObjectVersion = {
  /** Version ID. */
  id: UUID;
  /** Object ID. */
  object_id: UUID;
  /** Monotonic version number within the object. */
  version_number: number;
  /** Version title. */
  title: string;
  /** Version body. */
  body: string;
  /** Flat version metadata. */
  metadata: FlatMetadata;
  /** User that created this version. */
  created_by: UUID | null;
  /** Username of the user that created this version. */
  created_by_username?: string;
  /** Display name of the user that created this version. */
  created_by_display_name?: string;
  /** Creator's current effective workspace role. */
  created_by_workspace_role?: string;
  /** Creator's current effective access role for this object. */
  created_by_object_role?: string;
  /** Creation timestamp. */
  created_at: Timestamp;
};

/** Object response envelope. */
export type ObjectResponse = {
  /** Object resource. */
  object: ObjectResource;
  /** Current version, when present. */
  current_version?: ObjectVersion;
  /**
   * Effective role derived from the authenticated user's object authority.
   *
   * API-key scopes remain an additional restriction for API-key requests.
   */
  effective_role: ObjectRole;
};

/** Object-version response envelope. */
export type ObjectVersionResponse = { version: ObjectVersion };

/** Object attachment resource. */
export type ObjectAttachment = {
  /** Attachment ID. */
  id: UUID;
  /** Parent workspace ID. */
  workspace_id: UUID;
  /** Parent object ID. */
  object_id: UUID;
  /** Optional object-version ID. */
  version_id?: UUID;
  /** Stable SHA-256 content reference for the attachment bytes. */
  content_ref: string;
  /** Stored attachment length in bytes. */
  size_bytes: number;
  /**
   * Best-effort provenance for a reused attachment.
   *
   * Cleared if the source attachment is deleted.
   */
  source_attachment_id?: UUID;
  /** Optional attachment display name. */
  name?: string;
  /** Optional media type. */
  media_type?: string;
  /** Flat attachment metadata. */
  metadata: FlatMetadata;
  /** User that created this attachment. */
  created_by: UUID | null;
  /** Creation timestamp. */
  created_at: Timestamp;
};

/** Object-attachment response envelope. */
export type ObjectAttachmentResponse = { attachment: ObjectAttachment };

/** Request to reuse an existing object attachment. */
export type ReuseObjectAttachmentRequest = {
  /** Authorized source attachment ID. */
  source_attachment_id: UUID;
  /** Optional target object-version ID. */
  version_id?: UUID | null;
};

/** Query parameters for uploading an object attachment. */
export type UploadObjectAttachmentParams = {
  /** Optional object-version ID this attachment belongs to. */
  version_id?: UUID | null;
  /** Optional attachment display name. */
  name?: string | null;
  /** Optional media type. */
  media_type?: string | null;
  /** Optional attachment metadata encoded as a flat JSON object string. */
  metadata?: string | null;
};

/** Request body for creating an object and its initial version. */
export type CreateObjectRequest = {
  /** Object title. */
  title: string;
  /** Initial object body. Defaults to an empty string. */
  body?: string;
  /** Initial flat object metadata. Defaults to an empty object. */
  metadata?: FlatMetadata;
};

/** Request body for updating an object with optimistic concurrency control. */
export type UpdateObjectRequest = {
  /**
   * Expected current version ID for optimistic concurrency control.
   *
   * The server returns `409 Conflict` if the object has changed since this version was read.
   */
  expected_current_version_id: UUID;
  /** New title. Omitted values inherit from the current version; `null` is invalid. */
  title?: string;
  /** New body. Omitted values inherit from the current version; `null` is invalid. */
  body?: string;
  /** New flat metadata. Omitted values inherit from the current version; `null` is invalid. */
  metadata?: FlatMetadata;
};
