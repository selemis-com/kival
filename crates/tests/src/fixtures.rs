//! Scenario fixture helpers built on top of the real HTTP API.

use axum::http::Method;
use kival_sdk::{
    CreateGroupMembershipRequest, CreateGroupRequest, CreateObjectEdgeRequest,
    CreateObjectGrantRequest, CreateObjectRequest, CreateWorkspaceGroupRequest,
    CreateWorkspaceMembershipRequest, CreateWorkspaceRequest, Event, GrantPrincipal, Group,
    GroupMembershipResponse, GroupResponse, ListResponse, MembershipRole, ObjectAttachment,
    ObjectAttachmentResponse, ObjectBacklinksResponse, ObjectEdge, ObjectEdgeResponse, ObjectGrant,
    ObjectGrantResponse, ObjectGraphResponse, ObjectResource, ObjectResponse, ObjectRole,
    ReuseObjectAttachmentRequest, SearchResponse, UpdateObjectRequest, Workspace,
    WorkspaceGraphResponse, WorkspaceGroup, WorkspaceGroupResponse, WorkspaceMembershipResponse,
    WorkspaceResponse,
};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    TestActor, TestKival, TestResponseExt, TestResult, db, http::actor_from_session,
    names::unique_name,
};

/// Minimal workspace fixture.
#[derive(Debug, Clone, Copy)]
pub struct TestWorkspace {
    /// Workspace ID.
    pub id: Uuid,
}

/// Minimal object-space fixture.
///
/// This represents the common setup for object scenario tests: one workspace
/// with one group linked into that workspace.
#[derive(Debug, Clone, Copy)]
pub struct TestObjectSpace {
    /// Workspace fixture.
    pub workspace: TestWorkspace,

    /// Group fixture linked into the workspace.
    pub group: TestGroup,
}

/// Minimal group fixture.
#[derive(Debug, Clone, Copy)]
pub struct TestGroup {
    /// Group ID.
    pub id: Uuid,
}

/// Minimal object fixture.
#[derive(Debug, Clone, Copy)]
pub struct TestObject {
    /// Object ID.
    pub id: Uuid,
    /// Current version ID, when present.
    pub current_version_id: Option<Uuid>,
}

/// Convenience extension methods for creating Kival test fixtures.
pub trait TestFixtureExt {
    /// Creates a workspace and one linked group for object scenario tests.
    fn object_space(
        &self,
        prefix: &str,
    ) -> impl Future<Output = TestResult<TestObjectSpace>> + Send;

    /// Creates a test user as the global admin and logs in as that user.
    fn create_user(&self, prefix: &str) -> impl Future<Output = TestResult<TestActor>> + Send;

    /// Creates a test user and adds that user to a workspace.
    fn create_workspace_actor(
        &self,
        workspace_id: Uuid,
        prefix: &str,
        role: MembershipRole,
    ) -> impl Future<Output = TestResult<TestActor>> + Send;

    /// Creates a test user with workspace membership and an explicit object grant.
    fn create_object_actor(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        prefix: &str,
        workspace_role: MembershipRole,
        object_role: ObjectRole,
    ) -> impl Future<Output = TestResult<TestActor>> + Send;

    /// Creates a workspace as the global admin.
    fn create_workspace(
        &self,
        prefix: &str,
    ) -> impl Future<Output = TestResult<TestWorkspace>> + Send;

    /// Creates a group as the global admin.
    fn create_group(&self, prefix: &str) -> impl Future<Output = TestResult<TestGroup>> + Send;

    /// Adds a user to a group.
    fn add_user_to_group(
        &self,
        group_id: Uuid,
        user_id: Uuid,
        role: MembershipRole,
    ) -> impl Future<Output = TestResult<()>> + Send;

    /// Links a group into a workspace.
    fn add_group_to_workspace(
        &self,
        workspace_id: Uuid,
        group_id: Uuid,
    ) -> impl Future<Output = TestResult<()>> + Send;

    /// Adds a user to a workspace.
    fn add_user_to_workspace(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
        role: MembershipRole,
    ) -> impl Future<Output = TestResult<()>> + Send;

    /// Archives a workspace as the global admin.
    fn archive_workspace(
        &self,
        workspace_id: Uuid,
    ) -> impl Future<Output = TestResult<Workspace>> + Send;

    /// Unarchives a workspace as the global admin.
    fn unarchive_workspace(
        &self,
        workspace_id: Uuid,
    ) -> impl Future<Output = TestResult<Workspace>> + Send;

    /// Archives a group as the global admin.
    fn archive_group(&self, group_id: Uuid) -> impl Future<Output = TestResult<Group>> + Send;

    /// Unarchives a group as the global admin.
    fn unarchive_group(&self, group_id: Uuid) -> impl Future<Output = TestResult<Group>> + Send;

    /// Archives a workspace-group link as the global admin.
    fn archive_workspace_group(
        &self,
        workspace_id: Uuid,
        group_id: Uuid,
    ) -> impl Future<Output = TestResult<WorkspaceGroup>> + Send;

    /// Unarchives a workspace-group link as the global admin.
    fn unarchive_workspace_group(
        &self,
        workspace_id: Uuid,
        group_id: Uuid,
    ) -> impl Future<Output = TestResult<WorkspaceGroup>> + Send;

    /// Returns the active group membership ID for a user.
    fn active_group_membership_id(
        &self,
        group_id: Uuid,
        user_id: Uuid,
    ) -> impl Future<Output = TestResult<Uuid>> + Send;

    /// Returns the active workspace membership ID for a user.
    fn active_workspace_membership_id(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
    ) -> impl Future<Output = TestResult<Uuid>> + Send;

    /// Returns the active workspace-group link ID.
    fn active_workspace_group_id(
        &self,
        workspace_id: Uuid,
        group_id: Uuid,
    ) -> impl Future<Output = TestResult<Uuid>> + Send;

    /// Returns the active object grant ID for a principal.
    fn active_object_grant_id(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        principal: GrantPrincipal,
    ) -> impl Future<Output = TestResult<Uuid>> + Send;

    /// Creates an object as the global admin.
    fn create_object(
        &self,
        workspace_id: Uuid,
        title: &str,
        body: &str,
        metadata: Value,
    ) -> impl Future<Output = TestResult<TestObject>> + Send;

    /// Updates an object as the global admin.
    fn update_object(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        title: Option<&str>,
        body: Option<&str>,
        metadata: Option<Value>,
    ) -> impl Future<Output = TestResult<ObjectResponse>> + Send;

    /// Creates an explicit object edge as the global admin.
    fn create_edge(
        &self,
        workspace_id: Uuid,
        source_object_id: Uuid,
        target_object_id: Uuid,
    ) -> impl Future<Output = TestResult<ObjectEdge>> + Send;

    /// Creates an object grant as the global admin.
    fn create_object_grant(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        principal: GrantPrincipal,
        role: ObjectRole,
    ) -> impl Future<Output = TestResult<ObjectGrant>> + Send;

    /// Gets backlinks as an actor.
    fn backlinks_as(
        &self,
        actor: &TestActor,
        workspace_id: Uuid,
        object_id: Uuid,
    ) -> impl Future<Output = TestResult<ObjectBacklinksResponse>> + Send;

    /// Archives an object as the global admin.
    fn archive_object(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
    ) -> impl Future<Output = TestResult<ObjectResource>> + Send;

    /// Unarchives an object as the global admin.
    fn unarchive_object(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
    ) -> impl Future<Output = TestResult<ObjectResource>> + Send;

    /// Updates an object title as the global admin.
    fn update_object_title(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        title: &str,
    ) -> impl Future<Output = TestResult<ObjectResource>> + Send;

    /// Uploads an attachment as the global admin.
    #[expect(clippy::too_many_arguments, reason = "test harness")]
    fn upload_attachment(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        version_id: Option<Uuid>,
        name: Option<&str>,
        media_type: Option<&str>,
        metadata: Value,
        bytes: Vec<u8>,
    ) -> impl Future<Output = TestResult<ObjectAttachment>> + Send;

    /// Reuses an attachment as an actor.
    fn reuse_attachment_as(
        &self,
        actor: &TestActor,
        workspace_id: Uuid,
        object_id: Uuid,
        source_attachment_id: Uuid,
        version_id: Option<Uuid>,
    ) -> impl Future<Output = TestResult<ObjectAttachment>> + Send;

    /// Gets an attachment as an actor.
    fn get_attachment_as(
        &self,
        actor: &TestActor,
        workspace_id: Uuid,
        object_id: Uuid,
        attachment_id: Uuid,
    ) -> impl Future<Output = TestResult<ObjectAttachment>> + Send;

    /// Lists attachments as an actor.
    fn list_attachments_as(
        &self,
        actor: &TestActor,
        workspace_id: Uuid,
        object_id: Uuid,
    ) -> impl Future<Output = TestResult<ListResponse<ObjectAttachment>>> + Send;

    /// Searches a workspace as an actor using literal search.
    fn search_as(
        &self,
        actor: &TestActor,
        workspace_id: Uuid,
        query: &str,
    ) -> impl Future<Output = TestResult<SearchResponse>> + Send;

    /// Gets an object-centered graph as an actor.
    ///
    /// `query` should be a raw query string without the leading `?`.
    fn object_graph_as(
        &self,
        actor: &TestActor,
        workspace_id: Uuid,
        object_id: Uuid,
        query: &str,
    ) -> impl Future<Output = TestResult<ObjectGraphResponse>> + Send;

    /// Gets a workspace graph as an actor.
    ///
    /// `query` should be a raw query string without the leading `?`.
    fn workspace_graph_as(
        &self,
        actor: &TestActor,
        workspace_id: Uuid,
        query: &str,
    ) -> impl Future<Output = TestResult<WorkspaceGraphResponse>> + Send;

    /// Gets object events as an actor.
    fn object_events_as(
        &self,
        actor: &TestActor,
        workspace_id: Uuid,
        object_id: Uuid,
        query: &str,
    ) -> impl Future<Output = TestResult<ListResponse<Event>>> + Send;

    /// Gets an object as an actor.
    fn get_object_as(
        &self,
        actor: &TestActor,
        workspace_id: Uuid,
        object_id: Uuid,
    ) -> impl Future<Output = TestResult<ObjectResource>> + Send;

    /// Updates an object as an actor.
    fn update_object_as(
        &self,
        actor: &TestActor,
        workspace_id: Uuid,
        object_id: Uuid,
        title: Option<&str>,
        body: Option<&str>,
        metadata: Option<Value>,
    ) -> impl Future<Output = TestResult<ObjectResponse>> + Send;

    /// Updates an object title as an actor.
    fn update_object_title_as(
        &self,
        actor: &TestActor,
        workspace_id: Uuid,
        object_id: Uuid,
        title: &str,
    ) -> impl Future<Output = TestResult<ObjectResource>> + Send;
}

impl TestFixtureExt for TestKival {
    async fn object_space(&self, prefix: &str) -> TestResult<TestObjectSpace> {
        let workspace = self.create_workspace(prefix).await?;
        let group_prefix = format!("{prefix} editors");
        let group = self.create_group(&group_prefix).await?;

        self.add_group_to_workspace(workspace.id, group.id).await?;

        Ok(TestObjectSpace { workspace, group })
    }

    async fn create_user(&self, prefix: &str) -> TestResult<TestActor> {
        let session = db::insert_user_session(&self.pool, prefix, self.admin.id).await?;
        actor_from_session(session)
    }

    async fn create_workspace_actor(
        &self,
        workspace_id: Uuid,
        prefix: &str,
        role: MembershipRole,
    ) -> TestResult<TestActor> {
        let actor = self.create_user(prefix).await?;
        self.add_user_to_workspace(workspace_id, actor.id, role).await?;

        Ok(actor)
    }

    async fn create_object_actor(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        prefix: &str,
        workspace_role: MembershipRole,
        object_role: ObjectRole,
    ) -> TestResult<TestActor> {
        let actor = self.create_workspace_actor(workspace_id, prefix, workspace_role).await?;
        self.create_object_grant(
            workspace_id,
            object_id,
            GrantPrincipal::User(actor.id),
            object_role,
        )
        .await?;

        Ok(actor)
    }

    async fn create_workspace(&self, prefix: &str) -> TestResult<TestWorkspace> {
        let response: WorkspaceResponse = self
            .request_json_as(
                &self.admin,
                Method::POST,
                "/workspaces",
                &CreateWorkspaceRequest {
                    name: unique_name(prefix),
                    description: Some(format!("Test workspace for {prefix}")),
                },
            )
            .await?
            .into_success()?;

        Ok(TestWorkspace { id: response.workspace.id })
    }

    async fn create_group(&self, prefix: &str) -> TestResult<TestGroup> {
        let response: GroupResponse = self
            .request_json_as(
                &self.admin,
                Method::POST,
                "/groups",
                &CreateGroupRequest {
                    name: unique_name(prefix),
                    description: Some(format!("Test group for {prefix}")),
                },
            )
            .await?
            .into_success()?;

        Ok(TestGroup { id: response.group.id })
    }

    async fn add_user_to_group(
        &self,
        group_id: Uuid,
        user_id: Uuid,
        role: MembershipRole,
    ) -> TestResult<()> {
        let _: GroupMembershipResponse = self
            .request_json_as(
                &self.admin,
                Method::POST,
                &format!("/groups/{group_id}/memberships"),
                &CreateGroupMembershipRequest {
                    user_id: Some(user_id),
                    username: None,
                    group_role: role,
                },
            )
            .await?
            .into_success()?;

        Ok(())
    }

    async fn add_group_to_workspace(&self, workspace_id: Uuid, group_id: Uuid) -> TestResult<()> {
        let _: WorkspaceGroupResponse = self
            .request_json_as(
                &self.admin,
                Method::POST,
                &format!("/workspaces/{workspace_id}/groups"),
                &CreateWorkspaceGroupRequest { group_id },
            )
            .await?
            .into_success()?;

        Ok(())
    }

    async fn add_user_to_workspace(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
        role: MembershipRole,
    ) -> TestResult<()> {
        let _: WorkspaceMembershipResponse = self
            .request_json_as(
                &self.admin,
                Method::POST,
                &format!("/workspaces/{workspace_id}/memberships"),
                &CreateWorkspaceMembershipRequest {
                    user_id: Some(user_id),
                    username: None,
                    workspace_role: role,
                },
            )
            .await?
            .into_success()?;

        Ok(())
    }

    async fn archive_workspace(&self, workspace_id: Uuid) -> TestResult<Workspace> {
        let response: WorkspaceResponse = self
            .empty_json_as(
                &self.admin,
                Method::POST,
                &format!("/workspaces/{workspace_id}/archive"),
            )
            .await?
            .into_success()?;

        Ok(response.workspace)
    }

    async fn unarchive_workspace(&self, workspace_id: Uuid) -> TestResult<Workspace> {
        let response: WorkspaceResponse = self
            .empty_json_as(
                &self.admin,
                Method::POST,
                &format!("/workspaces/{workspace_id}/unarchive"),
            )
            .await?
            .into_success()?;

        Ok(response.workspace)
    }

    async fn archive_group(&self, group_id: Uuid) -> TestResult<Group> {
        let response: GroupResponse = self
            .empty_json_as(&self.admin, Method::POST, &format!("/groups/{group_id}/archive"))
            .await?
            .into_success()?;

        Ok(response.group)
    }

    async fn unarchive_group(&self, group_id: Uuid) -> TestResult<Group> {
        let response: GroupResponse = self
            .empty_json_as(&self.admin, Method::POST, &format!("/groups/{group_id}/unarchive"))
            .await?
            .into_success()?;

        Ok(response.group)
    }

    async fn archive_workspace_group(
        &self,
        workspace_id: Uuid,
        group_id: Uuid,
    ) -> TestResult<WorkspaceGroup> {
        let response: WorkspaceGroupResponse = self
            .empty_json_as(
                &self.admin,
                Method::POST,
                &format!("/workspaces/{workspace_id}/groups/{group_id}/archive"),
            )
            .await?
            .into_success()?;

        Ok(response.workspace_group)
    }

    async fn unarchive_workspace_group(
        &self,
        workspace_id: Uuid,
        group_id: Uuid,
    ) -> TestResult<WorkspaceGroup> {
        let response: WorkspaceGroupResponse = self
            .empty_json_as(
                &self.admin,
                Method::POST,
                &format!("/workspaces/{workspace_id}/groups/{group_id}/unarchive"),
            )
            .await?
            .into_success()?;

        Ok(response.workspace_group)
    }

    async fn active_group_membership_id(&self, group_id: Uuid, user_id: Uuid) -> TestResult<Uuid> {
        sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM kival.group_memberships WHERE group_id = $1 AND user_id = $2 AND revoked_at IS NULL",
        )
        .bind(group_id)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    async fn active_workspace_membership_id(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
    ) -> TestResult<Uuid> {
        sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT id
            FROM kival.workspace_memberships
            WHERE workspace_id = $1
                AND user_id = $2
                AND revoked_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    async fn active_workspace_group_id(
        &self,
        workspace_id: Uuid,
        group_id: Uuid,
    ) -> TestResult<Uuid> {
        sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT id
            FROM kival.workspace_groups
            WHERE workspace_id = $1
                AND group_id = $2
                AND archived_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .bind(group_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    async fn active_object_grant_id(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        principal: GrantPrincipal,
    ) -> TestResult<Uuid> {
        let (principal_user_id, principal_group_id) = match principal {
            GrantPrincipal::User(user_id) => (Some(user_id), None),
            GrantPrincipal::Group(group_id) => (None, Some(group_id)),
        };

        sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT id
            FROM kival.object_grants
            WHERE workspace_id = $1
                AND object_id = $2
                AND principal_user_id IS NOT DISTINCT FROM $3
                AND principal_group_id IS NOT DISTINCT FROM $4
                AND revoked_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .bind(object_id)
        .bind(principal_user_id)
        .bind(principal_group_id)
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    async fn create_object(
        &self,
        workspace_id: Uuid,
        title: &str,
        body: &str,
        metadata: Value,
    ) -> TestResult<TestObject> {
        let response: ObjectResponse = self
            .request_json_as(
                &self.admin,
                Method::POST,
                &format!("/workspaces/{workspace_id}/objects"),
                &CreateObjectRequest { title: title.to_owned(), body: body.to_owned(), metadata },
            )
            .await?
            .into_success()?;

        Ok(TestObject {
            id: response.object.id,
            current_version_id: response.object.current_version_id,
        })
    }

    async fn update_object(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        title: Option<&str>,
        body: Option<&str>,
        metadata: Option<Value>,
    ) -> TestResult<ObjectResponse> {
        self.update_object_as(&self.admin, workspace_id, object_id, title, body, metadata).await
    }

    async fn create_edge(
        &self,
        workspace_id: Uuid,
        source_object_id: Uuid,
        target_object_id: Uuid,
    ) -> TestResult<ObjectEdge> {
        let response: ObjectEdgeResponse = self
            .request_json_as(
                &self.admin,
                Method::POST,
                &format!("/workspaces/{workspace_id}/edges"),
                &CreateObjectEdgeRequest { source_object_id, target_object_id },
            )
            .await?
            .into_success()?;

        Ok(response.edge)
    }

    async fn create_object_grant(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        principal: GrantPrincipal,
        role: ObjectRole,
    ) -> TestResult<ObjectGrant> {
        let response: ObjectGrantResponse = self
            .request_json_as(
                &self.admin,
                Method::POST,
                &format!("/workspaces/{workspace_id}/objects/{object_id}/grants"),
                &CreateObjectGrantRequest { principal, object_role: role },
            )
            .await?
            .into_success()?;

        Ok(response.grant)
    }

    async fn backlinks_as(
        &self,
        actor: &TestActor,
        workspace_id: Uuid,
        object_id: Uuid,
    ) -> TestResult<ObjectBacklinksResponse> {
        self.get_json_as(
            actor,
            &format!("/workspaces/{workspace_id}/objects/{object_id}/backlinks"),
        )
        .await?
        .into_success()
    }

    async fn archive_object(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
    ) -> TestResult<ObjectResource> {
        let response: ObjectResponse = self
            .empty_json_as(
                &self.admin,
                Method::POST,
                &format!("/workspaces/{workspace_id}/objects/{object_id}/archive"),
            )
            .await?
            .into_success()?;

        Ok(response.object)
    }

    async fn unarchive_object(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
    ) -> TestResult<ObjectResource> {
        let response: ObjectResponse = self
            .empty_json_as(
                &self.admin,
                Method::POST,
                &format!("/workspaces/{workspace_id}/objects/{object_id}/unarchive"),
            )
            .await?
            .into_success()?;

        Ok(response.object)
    }

    async fn update_object_title(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        title: &str,
    ) -> TestResult<ObjectResource> {
        self.update_object_title_as(&self.admin, workspace_id, object_id, title).await
    }

    async fn upload_attachment(
        &self,
        workspace_id: Uuid,
        object_id: Uuid,
        version_id: Option<Uuid>,
        name: Option<&str>,
        media_type: Option<&str>,
        metadata: Value,
        bytes: Vec<u8>,
    ) -> TestResult<ObjectAttachment> {
        let path = attachment_upload_path(
            workspace_id,
            object_id,
            version_id,
            name,
            media_type,
            &metadata,
        );

        let response: ObjectAttachmentResponse = self
            .request_bytes_as(
                &self.admin,
                Method::POST,
                &path,
                bytes,
                Some("application/octet-stream"),
            )
            .await?
            .into_success()?;

        Ok(response.attachment)
    }

    async fn reuse_attachment_as(
        &self,
        actor: &TestActor,
        workspace_id: Uuid,
        object_id: Uuid,
        source_attachment_id: Uuid,
        version_id: Option<Uuid>,
    ) -> TestResult<ObjectAttachment> {
        let response: ObjectAttachmentResponse = self
            .request_json_as(
                actor,
                Method::POST,
                &format!("/workspaces/{workspace_id}/objects/{object_id}/attachments/reuse"),
                &ReuseObjectAttachmentRequest { source_attachment_id, version_id },
            )
            .await?
            .into_success()?;

        Ok(response.attachment)
    }

    async fn get_attachment_as(
        &self,
        actor: &TestActor,
        workspace_id: Uuid,
        object_id: Uuid,
        attachment_id: Uuid,
    ) -> TestResult<ObjectAttachment> {
        let response: ObjectAttachmentResponse = self
            .get_json_as(
                actor,
                &format!(
                    "/workspaces/{workspace_id}/objects/{object_id}/attachments/{attachment_id}"
                ),
            )
            .await?
            .into_success()?;

        Ok(response.attachment)
    }

    async fn list_attachments_as(
        &self,
        actor: &TestActor,
        workspace_id: Uuid,
        object_id: Uuid,
    ) -> TestResult<ListResponse<ObjectAttachment>> {
        self.get_json_as(
            actor,
            &format!("/workspaces/{workspace_id}/objects/{object_id}/attachments"),
        )
        .await?
        .into_success()
    }

    async fn search_as(
        &self,
        actor: &TestActor,
        workspace_id: Uuid,
        query: &str,
    ) -> TestResult<SearchResponse> {
        self.get_json_as(
            actor,
            &format!(
                "/workspaces/{workspace_id}/search?q={}&mode=literal",
                percent_encode_query_value(query),
            ),
        )
        .await?
        .into_success()
    }

    async fn object_graph_as(
        &self,
        actor: &TestActor,
        workspace_id: Uuid,
        object_id: Uuid,
        query: &str,
    ) -> TestResult<ObjectGraphResponse> {
        let query = optional_query_suffix(query);

        self.get_json_as(
            actor,
            &format!("/workspaces/{workspace_id}/objects/{object_id}/graph{query}"),
        )
        .await?
        .into_success()
    }

    async fn workspace_graph_as(
        &self,
        actor: &TestActor,
        workspace_id: Uuid,
        query: &str,
    ) -> TestResult<WorkspaceGraphResponse> {
        let query = optional_query_suffix(query);

        self.get_json_as(actor, &format!("/workspaces/{workspace_id}/graph{query}"))
            .await?
            .into_success()
    }

    async fn object_events_as(
        &self,
        actor: &TestActor,
        workspace_id: Uuid,
        object_id: Uuid,
        query: &str,
    ) -> TestResult<ListResponse<Event>> {
        let query = optional_query_suffix(query);

        self.get_json_as(
            actor,
            &format!("/workspaces/{workspace_id}/objects/{object_id}/events{query}"),
        )
        .await?
        .into_success()
    }

    async fn get_object_as(
        &self,
        actor: &TestActor,
        workspace_id: Uuid,
        object_id: Uuid,
    ) -> TestResult<ObjectResource> {
        let response: ObjectResponse = self
            .get_json_as(actor, &format!("/workspaces/{workspace_id}/objects/{object_id}"))
            .await?
            .into_success()?;

        Ok(response.object)
    }

    async fn update_object_as(
        &self,
        actor: &TestActor,
        workspace_id: Uuid,
        object_id: Uuid,
        title: Option<&str>,
        body: Option<&str>,
        metadata: Option<Value>,
    ) -> TestResult<ObjectResponse> {
        let expected_current_version_id = self
            .get_object_as(actor, workspace_id, object_id)
            .await?
            .current_version_id
            .expect("test object should have a current version");
        self.request_json_as(
            actor,
            Method::PATCH,
            &format!("/workspaces/{workspace_id}/objects/{object_id}"),
            &UpdateObjectRequest {
                expected_current_version_id,
                title: title.map(ToOwned::to_owned),
                body: body.map(ToOwned::to_owned),
                metadata,
            },
        )
        .await?
        .into_success()
    }

    async fn update_object_title_as(
        &self,
        actor: &TestActor,
        workspace_id: Uuid,
        object_id: Uuid,
        title: &str,
    ) -> TestResult<ObjectResource> {
        Ok(self
            .update_object_as(actor, workspace_id, object_id, Some(title), None, None)
            .await?
            .object)
    }
}

/// Creates a simple metadata object.
#[must_use]
pub fn object_metadata(kind: &str) -> Value {
    json!({ "kind": kind })
}

/// Creates a simple Markdown body.
#[must_use]
pub fn test_body(title: &str, body: &str) -> String {
    format!("# {title}\n\n{body}")
}

/// Builds an attachment upload path with query parameters.
fn attachment_upload_path(
    workspace_id: Uuid,
    object_id: Uuid,
    version_id: Option<Uuid>,
    name: Option<&str>,
    media_type: Option<&str>,
    metadata: &Value,
) -> String {
    let mut path = format!("/workspaces/{workspace_id}/objects/{object_id}/attachments/upload");
    let mut query = Vec::new();

    if let Some(version_id) = version_id {
        query.push(("version_id", version_id.to_string()));
    }
    if let Some(name) = name {
        query.push(("name", name.to_owned()));
    }
    if let Some(media_type) = media_type {
        query.push(("media_type", media_type.to_owned()));
    }

    query.push(("metadata", metadata.to_string()));

    if !query.is_empty() {
        path.push('?');

        for (index, (key, value)) in query.into_iter().enumerate() {
            if index > 0 {
                path.push('&');
            }

            path.push_str(key);
            path.push('=');
            path.push_str(&percent_encode_query_value(&value));
        }
    }

    path
}

/// Percent-encodes a query parameter value.
fn percent_encode_query_value(value: &str) -> String {
    use std::fmt::Write as _;

    let mut encoded = String::new();

    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => {
                write!(&mut encoded, "%{byte:02X}").expect("writing to String cannot fail");
            }
        }
    }

    encoded
}

/// Returns a query suffix for an optional raw query string.
fn optional_query_suffix(query: &str) -> String {
    if query.is_empty() { String::new() } else { format!("?{query}") }
}
