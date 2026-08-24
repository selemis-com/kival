import * as commentaryActions from "../actions/commentary.js";
import {
  archiveGroup,
  archiveObject,
  archiveWorkspace,
  archiveWorkspaceGroup,
  createGroup,
  createGroupMembership,
  createObject,
  createObjectEdge,
  createObjectGrant,
  createWorkspaceGroup,
  createWorkspaceMembership,
  disableUser,
  enableUser,
  getGroup,
  getInboxUnreadCount,
  getObject,
  getObjectAttachment,
  getObjectAttachmentContent,
  getObjectAttachmentContentResponse,
  getObjectBacklinks,
  getObjectEdge,
  getObjectGraph,
  getObjectNotificationPreference,
  getObjectVersion,
  getObjectVersionWikilinks,
  getUser,
  getWorkspace,
  getWorkspaceGraph,
  health,
  listEvents,
  listGroupMemberships,
  listGroups,
  listInbox,
  listObjectAttachments,
  listObjectEdges,
  listObjectEvents,
  listObjectGrants,
  listObjects,
  listObjectVersions,
  listUsers,
  listWorkspaceEvents,
  listWorkspaceGroups,
  listWorkspaceMemberships,
  listWorkspaces,
  markInboxRead,
  ready,
  reuseObjectAttachment,
  revokeGroupMembership,
  revokeObjectEdge,
  revokeObjectGrant,
  revokeWorkspaceMembership,
  searchWorkspace,
  setObjectFavorite,
  setObjectPin,
  setWorkspacePin,
  unarchiveGroup,
  unarchiveObject,
  unarchiveWorkspace,
  unarchiveWorkspaceGroup,
  updateGroup,
  updateGroupMembership,
  updateInboxEntry,
  updateObject,
  updateObjectGrant,
  updateObjectNotificationPreference,
  updateUser,
  updateWorkspace,
  updateWorkspaceMembership,
  uploadObjectAttachment,
  whoami,
} from "../actions/index.js";
import {
  type HttpTransportOptions,
  http,
  type KivalRequestInit,
  type KivalTransport,
} from "../transports/index.js";

/**
 * Kival client configuration.
 *
 * Supply either a complete custom `transport`, or the options needed to create the default HTTP
 * transport. The two forms cannot be mixed.
 */
export type KivalClientConfig =
  | {
      transport: KivalTransport;
      apiKey?: never;
      baseUrl?: never;
      apiPrefix?: never;
      fetch?: never;
      timeout?: never;
    }
  | (HttpTransportOptions & { transport?: never });

const kivalActions = {
  ...commentaryActions,
  archiveGroup,
  archiveObject,
  archiveWorkspace,
  archiveWorkspaceGroup,
  createGroup,
  createGroupMembership,
  createObject,
  createObjectEdge,
  createObjectGrant,
  createWorkspaceGroup,
  createWorkspaceMembership,
  disableUser,
  enableUser,
  getGroup,
  getInboxUnreadCount,
  getObject,
  getObjectAttachment,
  getObjectAttachmentContent,
  getObjectAttachmentContentResponse,
  getObjectBacklinks,
  getObjectEdge,
  getObjectGraph,
  getObjectNotificationPreference,
  getObjectVersion,
  getObjectVersionWikilinks,
  getUser,
  getWorkspace,
  getWorkspaceGraph,
  health,
  listEvents,
  listGroupMemberships,
  listGroups,
  listInbox,
  listObjectAttachments,
  listObjectEdges,
  listObjectEvents,
  listObjectGrants,
  listObjectVersions,
  listObjects,
  listUsers,
  listWorkspaceEvents,
  listWorkspaceGroups,
  listWorkspaceMemberships,
  listWorkspaces,
  markInboxRead,
  ready,
  reuseObjectAttachment,
  revokeGroupMembership,
  revokeObjectEdge,
  revokeObjectGrant,
  revokeWorkspaceMembership,
  searchWorkspace,
  setObjectFavorite,
  setObjectPin,
  setWorkspacePin,
  unarchiveGroup,
  unarchiveObject,
  unarchiveWorkspace,
  unarchiveWorkspaceGroup,
  updateGroup,
  updateGroupMembership,
  updateInboxEntry,
  updateObject,
  updateObjectGrant,
  updateObjectNotificationPreference,
  updateUser,
  updateWorkspace,
  updateWorkspaceMembership,
  uploadObjectAttachment,
  whoami,
} as const;

type BoundAction<Action> = Action extends (
  client: infer _Client,
  ...args: infer Arguments
) => infer Result
  ? (...args: Arguments) => Result
  : never;

type ActionModule = typeof kivalActions;

/** Complete set of standalone SDK actions bound to a client transport. */
export type KivalActions = {
  [Name in keyof ActionModule]: BoundAction<ActionModule[Name]>;
};

/** Fully configured Kival client with all SDK actions. */
export type KivalClient = KivalTransport & KivalActions;

function bindActions(client: KivalTransport): KivalActions {
  return Object.fromEntries(
    Object.entries(kivalActions).map(([name, action]) => [
      name,
      (parameters?: unknown) => action(client, parameters as never),
    ]),
  ) as KivalActions;
}

/** Creates a configured Kival client with the complete action set. */
export function createKivalClient(config: KivalClientConfig): KivalClient {
  if (!config) {
    throw new TypeError("client configuration is required");
  }

  const transport = "transport" in config ? config.transport : http(config);
  const client: KivalTransport = {
    baseUrl: transport.baseUrl,
    apiPrefix: transport.apiPrefix,
    requestJson<T>(path: string, init?: KivalRequestInit) {
      return transport.requestJson<T>(path, init);
    },
    requestBytes(path: string, init?: KivalRequestInit) {
      return transport.requestBytes(path, init);
    },
    requestVoid(path: string, init?: KivalRequestInit) {
      return transport.requestVoid(path, init);
    },
    requestResponse(path: string, init?: KivalRequestInit) {
      return transport.requestResponse(path, init);
    },
    url(path: string) {
      return transport.url(path);
    },
  };

  return { ...client, ...bindActions(client) };
}
