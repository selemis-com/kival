import { useCallback } from "react";
import { kival } from "../../../shared/api";
import { usePaginatedResource } from "../../../shared/hooks/usePaginatedResource";
import type {
  CreateWorkspaceMembershipRequest,
  MembershipRole,
  WorkspaceMembership,
} from "../../../shared/types";

export function useWorkspaceDirectory(workspaceId: string) {
  const loadMembershipPage = useCallback(
    async (cursor: string | null, signal: AbortSignal) => {
      const response = await kival.listWorkspaceMemberships({ workspaceId, cursor, signal });
      return { items: response.items, nextCursor: response.next_cursor ?? null };
    },
    [workspaceId],
  );
  const {
    items: memberships,
    setItems: setMemberships,
    nextCursor: membershipsNextCursor,
    loading,
    loadingMore,
    error,
    loadMore,
  } = usePaginatedResource({
    queryKey: workspaceId,
    loadPage: loadMembershipPage,
    errorMessage: "Could not load workspace members.",
    itemKey: (membership: WorkspaceMembership) => membership.id,
  });

  async function addMember(input: CreateWorkspaceMembershipRequest) {
    const membership = await kival.createWorkspaceMembership({ workspaceId, input });
    setMemberships((current) => [
      membership,
      ...current.filter((candidate) => candidate.id !== membership.id),
    ]);
    return membership;
  }

  async function removeMember(membershipId: string) {
    const membership = await kival.revokeWorkspaceMembership({ workspaceId, membershipId });
    setMemberships((current) => current.filter((candidate) => candidate.id !== membership.id));
    return membership;
  }

  async function updateMemberRole(membershipId: string, workspaceRole: MembershipRole) {
    const membership = await kival.updateWorkspaceMembership({
      workspaceId,
      membershipId,
      input: { workspace_role: workspaceRole },
    });
    setMemberships((current) =>
      current.map((candidate) => (candidate.id === membershipId ? membership : candidate)),
    );
    return membership;
  }

  return {
    memberships,
    membershipsNextCursor,
    loading,
    loadingMore,
    error,
    loadMore,
    addMember,
    updateMemberRole,
    removeMember,
  };
}
