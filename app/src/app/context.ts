import { useOutletContext } from "react-router";
import type { User, Workspace } from "../shared/types";

export type KivalAppContext = {
  user: User;
  isGlobalAdmin: boolean;
  canManageGroups: boolean;
  workspaces: Workspace[];
  pinnedWorkspaces: Workspace[];
  workspacesNextCursor: string | null;
  workspacesLoadingMore: boolean;
  unreadInboxCount: number;
  inboxRevision: number;
  error: string | null;
  setApplicationError: (error: string | null) => void;
  replaceCurrentUser: (user: User) => void;
  loadMoreWorkspaces: () => Promise<void>;
  createWorkspace: (name: string, description?: string) => Promise<void>;
  restoreWorkspace: (workspaceId: string) => Promise<void>;
  setWorkspacePin: (workspaceId: string, pinned: boolean) => Promise<void>;
  replaceWorkspace: (workspace: Workspace) => void;
  removeWorkspace: (workspaceId: string) => void;
  refreshCurrentIdentity: () => Promise<boolean>;
  refreshInbox: (signal?: AbortSignal) => Promise<void>;
  logout: () => Promise<void>;
};

export function useKivalApp() {
  return useOutletContext<KivalAppContext>();
}
