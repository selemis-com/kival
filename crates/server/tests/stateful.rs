//! Proptest state-machine scenarios against the real HTTP server.

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        collections::{BTreeMap, BTreeSet},
        net::{Ipv4Addr, SocketAddr},
    };

    use kival_sdk::{
        ApiKeyResponse, ApiKeyScope, ArchiveStatus, CommentMentionCandidate, CommentResponse,
        CommentStatus, CommentThreadListResponse, CommentThreadResponse, CreateApiKeyRequest,
        CreateApiKeyResponse, CreateCommentRequest, CreateGroupMembershipRequest,
        CreateGroupRequest, CreateObjectEdgeRequest, CreateObjectGrantRequest, CreateObjectRequest,
        CreateWorkspaceGroupRequest, CreateWorkspaceMembershipRequest, CreateWorkspaceRequest,
        Event, FavoriteState, GrantPrincipal, Group, GroupMembership, GroupMembershipResponse,
        GroupResponse, InboxEntry, InboxUnreadCountResponse, InboxUpdatedResponse, ListResponse,
        MarkInboxReadRequest, ObjectAttachment, ObjectAttachmentResponse, ObjectBacklinksResponse,
        ObjectEdge, ObjectEdgeResponse, ObjectGrant, ObjectGrantResponse, ObjectGraphResponse,
        ObjectNotificationPreference, ObjectResource, ObjectResponse, ObjectRole, ObjectVersion,
        ObjectVersionResponse, PinState, ReuseObjectAttachmentRequest, SearchResponse,
        SessionListResponse, SessionOnlyResponse, UpdateApiKeyRequest, UpdateCommentRequest,
        UpdateGroupMembershipRequest, UpdateGroupRequest, UpdateInboxEntryRequest,
        UpdateObjectGrantRequest, UpdateObjectNotificationPreferenceRequest, UpdateObjectRequest,
        UpdateWorkspaceMembershipRequest, UpdateWorkspaceRequest, UserResponse, UserStatus,
        WhoamiResponse, Workspace, WorkspaceGraphResponse, WorkspaceGroup, WorkspaceGroupResponse,
        WorkspaceMembership, WorkspaceMembershipResponse, WorkspaceResponse,
    };
    use kival_tests::{
        Actor, ActorClient, ApiKeyClient, Fixture, HttpResponse, KivalStateMachine, Lifecycle,
        Model, Operation, Principal, ResourceMap, TEST_RP_ID, TestKival,
    };
    use proptest_state_machine::{ReferenceStateMachine, StateMachineTest};
    use sqlx::PgPool;
    use tokio::{net::TcpListener, runtime::Handle, sync::oneshot, task::JoinHandle};

    /// Real server state corresponding to one generated Proptest case.
    struct ServerUnderTest {
        /// Campaign runtime used to bridge the synchronous state-machine runner to async HTTP.
        runtime: Handle,
        /// Test application retained for its database, blob directory, and pool.
        kival: TestKival,
        /// Authenticated browser clients used by generated operations.
        fixture: Fixture,
        /// Symbolic-to-real resource identifiers observed from API responses.
        resources: ResourceMap,
        /// Real bearer clients for API keys retained by the reference model.
        api_key_clients: BTreeMap<kival_tests::Handle, ApiKeyClient>,
        /// Per-case prefix preventing resource-name collisions during shrinking.
        namespace: String,
        /// Number of transitions executed in this generated case.
        step: usize,
        /// Last global event sequence observed before generated operations begin.
        event_baseline: i64,
        /// Graceful-shutdown trigger for the HTTP server.
        shutdown: Option<oneshot::Sender<()>>,
        /// Running HTTP server task.
        server: Option<JoinHandle<std::io::Result<()>>>,
    }

    impl ServerUnderTest {
        /// Stops and joins the per-case HTTP server exactly once.
        fn stop_server(&mut self) -> Result<(), String> {
            if let Some(shutdown) = self.shutdown.take() {
                let _ = shutdown.send(());
            }

            let Some(server) = self.server.take() else {
                return Ok(());
            };

            self.runtime
                .block_on(server)
                .map_err(|error| format!("join stateful HTTP server: {error}"))?
                .map_err(|error| format!("stop stateful HTTP server: {error}"))
        }
    }

    impl Drop for ServerUnderTest {
        fn drop(&mut self) {
            if let Err(error) = self.stop_server() {
                // Cleanup can run while propagating the case's real failure, so it must
                // not replace that panic with a second one.
                eprintln!("[stateful {} cleanup failed] {error}", self.namespace);
            }
        }
    }

    /// Concrete state-machine adapter for the Kival HTTP API.
    struct ServerStateMachine;

    std::thread_local! {
        /// SQLx-managed pool available to synchronous state-machine cases.
        static STATEFUL_POOL: RefCell<Option<PgPool>> = const { RefCell::new(None) };
        /// Campaign-wide Tokio runtime kept alive across generated cases and shrinking.
        static STATEFUL_RUNTIME: RefCell<Option<Handle>> = const { RefCell::new(None) };
    }

    /// Clears thread-local campaign resources when the stateful test finishes or unwinds.
    struct StatefulContextGuard;

    impl StatefulContextGuard {
        /// Installs the SQLx-managed pool and runtime for synchronous state-machine cases.
        fn install(pool: PgPool, runtime: Handle) -> Self {
            STATEFUL_POOL.with(|slot| {
                assert!(
                    slot.borrow_mut().replace(pool).is_none(),
                    "stateful pool already installed"
                );
            });
            STATEFUL_RUNTIME.with(|slot| {
                assert!(
                    slot.borrow_mut().replace(runtime).is_none(),
                    "stateful runtime already installed"
                );
            });
            Self
        }
    }

    impl Drop for StatefulContextGuard {
        fn drop(&mut self) {
            STATEFUL_RUNTIME.with(|slot| {
                let _runtime = slot.borrow_mut().take();
            });
            STATEFUL_POOL.with(|slot| {
                let _pool = slot.borrow_mut().take();
            });
        }
    }

    /// Clones the SQLx-managed pool for a generated state-machine case.
    fn stateful_pool() -> PgPool {
        STATEFUL_POOL.with(|slot| {
            slot.borrow().as_ref().expect("stateful test pool must be installed").clone()
        })
    }

    /// Clones the campaign-wide runtime handle for a generated state-machine case.
    fn stateful_runtime() -> Handle {
        STATEFUL_RUNTIME.with(|slot| {
            slot.borrow().as_ref().expect("stateful runtime must be installed").clone()
        })
    }

    /// Event kinds whose stable fields are represented by the reference model.
    const STABLE_EVENT_KINDS: &[&str] = &[
        "group.archived",
        "group.created",
        "group.membership_created",
        "group.membership_revoked",
        "group.membership_updated",
        "group.unarchived",
        "group.updated",
        "comment.created",
        "comment.deleted",
        "comment.edited",
        "comment.replied",
        "comment_thread.reopened",
        "comment_thread.resolved",
        "object.archived",
        "object.attachment_created",
        "object.created",
        "object.unarchived",
        "object.updated",
        "object.version_appended",
        "object_edge.created",
        "object_edge.revoked",
        "object_grant.created",
        "object_grant.revoked",
        "object_grant.updated",
        "user.disabled",
        "user.enabled",
        "workspace.archived",
        "workspace.created",
        "workspace.group_archived",
        "workspace.group_linked",
        "workspace.group_unarchived",
        "workspace.membership_created",
        "workspace.membership_revoked",
        "workspace.membership_updated",
        "workspace.unarchived",
        "workspace.updated",
    ];

    /// Exact HTTP outcome required by a generated operation.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ExpectedOutcome {
        /// The operation completes and returns its JSON or byte response.
        Success,
        /// The resource exists but the actor lacks authority.
        Forbidden,
        /// The resource is absent from the lifecycle accepted by the endpoint.
        NotFound,
        /// The request conflicts with existing state.
        Conflict,
    }

    impl ExpectedOutcome {
        /// Returns the one accepted HTTP status for this outcome.
        const fn status(self) -> u16 {
            match self {
                Self::Success => 200,
                Self::Forbidden => 403,
                Self::NotFound => 404,
                Self::Conflict => 409,
            }
        }
    }

    /// Response body retained after exact status validation.
    struct AssertedResponse {
        /// Buffered response bytes.
        body: Vec<u8>,
        /// Response content type, when supplied.
        content_type: Option<String>,
    }

    /// Stable event fields compared between the reference model and HTTP API.
    #[derive(Debug, PartialEq, Eq)]
    struct EventProjection {
        /// Event kind emitted by the mutation.
        kind: String,
        /// Concrete user ID of the actor.
        actor_user_id: Option<uuid::Uuid>,
        /// Concrete workspace ID associated with the event.
        workspace_id: Option<uuid::Uuid>,
        /// Concrete object ID associated with the event.
        object_id: Option<uuid::Uuid>,
        /// Concrete object-edge ID associated with the event.
        object_edge_id: Option<uuid::Uuid>,
        /// Concrete object-grant ID associated with the event.
        object_grant_id: Option<uuid::Uuid>,
        /// Concrete group ID associated with the event.
        group_id: Option<uuid::Uuid>,
        /// Concrete user ID targeted by the mutation.
        target_user_id: Option<uuid::Uuid>,
    }

    impl AssertedResponse {
        /// Decodes the retained response as JSON.
        fn json<T: serde::de::DeserializeOwned>(&self, context: &str) -> T {
            serde_json::from_slice(&self.body).unwrap_or_else(|error| {
                panic!("{context}: {error}; response body={}", String::from_utf8_lossy(&self.body))
            })
        }

        /// Returns the retained raw response bytes.
        fn body(&self) -> &[u8] {
            &self.body
        }

        /// Returns the buffered response content type.
        fn content_type(&self) -> Option<&str> {
            self.content_type.as_deref()
        }
    }

    impl StateMachineTest for ServerStateMachine {
        type Reference = KivalStateMachine;
        type SystemUnderTest = ServerUnderTest;

        fn init_test(_reference: &Model) -> Self::SystemUnderTest {
            let runtime = stateful_runtime();
            let (kival, fixture, namespace, event_baseline, shutdown, server) =
                runtime.block_on(async {
                    let kival = TestKival::new(stateful_pool()).await.expect("create test Kival");
                    let users =
                        kival.provision_fixture_users().await.expect("provision fixture users");
                    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
                        .await
                        .expect("bind test server");
                    let address = listener.local_addr().expect("read test server address");
                    let base_url = format!("http://{address}");
                    let origin = format!("http://{TEST_RP_ID}:5173");
                    let (shutdown_tx, shutdown_rx) = oneshot::channel();
                    let app = kival.app.clone();
                    let server = tokio::spawn(async move {
                        axum::serve(
                            listener,
                            app.into_make_service_with_connect_info::<SocketAddr>(),
                        )
                        .with_graceful_shutdown(async {
                            let _ = shutdown_rx.await;
                        })
                        .await
                    });
                    let fixture =
                        Fixture::install_for_users(&kival.pool, base_url, &origin, &users)
                            .await
                            .expect("authenticate fixture users");
                    let namespace = format!("prop-{}", uuid::Uuid::now_v7().simple());
                    let events = fixture
                        .actors
                        .admin()
                        .browser
                        .get(format!("{}/api/v1/events?order=desc&limit=1", fixture.base_url))
                        .send()
                        .await
                        .expect("send initial event-baseline request")
                        .error_for_status()
                        .expect("initial event-baseline request succeeds")
                        .json::<ListResponse<Event>>()
                        .await
                        .expect("decode initial event baseline");
                    let event_baseline =
                        events.items.first().map_or(0, |event| event.sequence_number);
                    (kival, fixture, namespace, event_baseline, shutdown_tx, server)
                });

            let seed = std::env::var("PROPTEST_RNG_SEED").unwrap_or_else(|_| "random".to_owned());
            eprintln!("[stateful {namespace} case start] campaign_seed={seed}");
            ServerUnderTest {
                runtime,
                kival,
                fixture,
                resources: ResourceMap::default(),
                api_key_clients: BTreeMap::new(),
                namespace,
                step: 0,
                event_baseline,
                shutdown: Some(shutdown),
                server: Some(server),
            }
        }

        fn apply(
            mut state: Self::SystemUnderTest,
            reference: &Model,
            transition: Operation,
        ) -> Self::SystemUnderTest {
            state.step += 1;
            let action = serde_json::to_string(&transition).expect("serialize stateful action");
            eprintln!("[stateful {} step {}] {action}", state.namespace, state.step);
            let runtime = &state.runtime;
            let fixture = &state.fixture;
            let resources = &mut state.resources;
            let api_key_clients = &mut state.api_key_clients;
            let namespace = &state.namespace;
            runtime.block_on(execute(
                fixture,
                &state.kival.pool,
                resources,
                api_key_clients,
                namespace,
                reference,
                &transition,
            ));
            state
        }

        fn check_invariants(state: &Self::SystemUnderTest, reference: &Model) {
            for workspace in reference.visible_workspaces(Actor::Admin) {
                state
                    .resources
                    .resolve(workspace)
                    .expect("every modeled workspace has a concrete server ID");
            }
            for group in reference.visible_groups(Actor::Admin) {
                state
                    .resources
                    .resolve(group)
                    .expect("every modeled group has a concrete server ID");
            }
            for (object, workspace) in reference.objects() {
                state
                    .resources
                    .resolve(workspace)
                    .expect("every modeled object workspace has a concrete server ID");
                state
                    .resources
                    .resolve(object)
                    .expect("every modeled object has a concrete server ID");
            }
        }

        fn teardown(mut state: Self::SystemUnderTest, reference: Model) {
            eprintln!("[stateful {} final audit start]", state.namespace);
            state.runtime.block_on(audit_final_model(
                &state.fixture,
                &state.resources,
                &state.namespace,
                state.event_baseline,
                &reference,
            ));
            eprintln!("[stateful {} final audit complete]", state.namespace);
            eprintln!("[stateful {} case complete] {} actions", state.namespace, state.step);
            state.stop_server().expect("cleanly stop stateful HTTP server");
        }
    }

    /// Requires one exact HTTP outcome and retains the response for decoding.
    async fn assert_http_outcome(
        response: HttpResponse,
        expected: ExpectedOutcome,
        operation: &Operation,
    ) -> AssertedResponse {
        let operation = serde_json::to_string(operation).expect("serialize operation context");
        assert_http_outcome_with_context(response, expected, &operation).await
    }

    /// Requires one exact HTTP outcome with caller-provided audit context.
    async fn assert_http_outcome_with_context(
        response: HttpResponse,
        expected: ExpectedOutcome,
        context: &str,
    ) -> AssertedResponse {
        let actual = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = response.bytes().await.expect("buffer stateful HTTP response").to_vec();
        assert_eq!(
            actual,
            expected.status(),
            "stateful HTTP outcome mismatch\noperation={}\nexpected={expected:?} ({})\
             \nactual={actual}\nresponse body={}",
            context,
            expected.status(),
            String::from_utf8_lossy(&body),
        );
        AssertedResponse { body, content_type }
    }

    /// Verifies hydrated mention identities match the persisted fixture users.
    fn assert_comment_mentions(
        fixture: &Fixture,
        comment: &kival_sdk::Comment,
        expected: &[Actor],
    ) {
        assert_eq!(comment.mentions.len(), expected.len());
        for (mention, expected_actor) in comment.mentions.iter().zip(expected) {
            let actor = fixture.actors.get(*expected_actor);
            let identity = fixture.identities.get(*expected_actor);
            assert_eq!(mention.user_id, actor.user_id);
            assert_eq!(mention.username, identity.username);
            assert!(!mention.display_name.is_empty());
        }
    }

    /// Appends one object version with explicitly supplied title and body.
    async fn update_object_content(
        actor: &ActorClient,
        api: &str,
        workspace_id: uuid::Uuid,
        object_id: uuid::Uuid,
        title: &str,
        body: &str,
    ) {
        let current = actor
            .browser
            .get(format!("{api}/workspaces/{workspace_id}/objects/{object_id}"))
            .send()
            .await
            .expect("send stateful current-object read")
            .error_for_status()
            .expect("stateful current-object read succeeds")
            .json::<ObjectResponse>()
            .await
            .expect("decode stateful current object");
        let expected_current_version_id =
            current.current_version.expect("stateful object has current version").id;

        actor
            .browser
            .patch(format!("{api}/workspaces/{workspace_id}/objects/{object_id}"))
            .json(&UpdateObjectRequest {
                expected_current_version_id,
                title: Some(title.to_owned()),
                body: Some(body.to_owned()),
                metadata: Some(serde_json::json!({ "stateful": true })),
            })
            .send()
            .await
            .expect("send stateful object-content update")
            .error_for_status()
            .expect("stateful object-content update succeeds");
    }

    /// Verifies whether one source currently appears as a textual backlink.
    async fn assert_textual_backlink(
        actor: &ActorClient,
        api: &str,
        workspace_id: uuid::Uuid,
        target_id: uuid::Uuid,
        source_id: uuid::Uuid,
        raw_target: Option<&str>,
    ) {
        let response = actor
            .browser
            .get(format!("{api}/workspaces/{workspace_id}/objects/{target_id}/backlinks?limit=100"))
            .send()
            .await
            .expect("send stateful backlinks probe")
            .error_for_status()
            .expect("stateful backlinks probe succeeds")
            .json::<ObjectBacklinksResponse>()
            .await
            .expect("decode stateful backlinks probe");
        let matches = response
            .incoming_references
            .iter()
            .filter(|reference| reference.source_object.id == source_id)
            .collect::<Vec<_>>();
        match raw_target {
            Some(raw_target) => {
                assert_eq!(matches.len(), 1, "expected one textual backlink from source");
                assert_eq!(matches[0].raw_target, raw_target);
                assert_eq!(matches[0].target_object_id, target_id);
            }
            None => assert!(matches.is_empty(), "source textual backlink must be absent"),
        }
    }

    /// Executes one generated transition and checks its immediate postconditions.
    async fn execute(
        fixture: &Fixture,
        pool: &PgPool,
        resources: &mut ResourceMap,
        api_key_clients: &mut BTreeMap<kival_tests::Handle, ApiKeyClient>,
        namespace: &str,
        reference: &Model,
        operation: &Operation,
    ) {
        let actor = fixture.actors.get(operation.actor());
        let api = format!("{}/api/v1", fixture.base_url);

        match operation {
            Operation::CheckWhoAmI { .. } => {
                let response = actor
                    .browser
                    .get(format!("{api}/auth/whoami"))
                    .send()
                    .await
                    .expect("send whoami request");
                let whoami = response
                    .error_for_status()
                    .expect("whoami succeeds")
                    .json::<WhoamiResponse>()
                    .await
                    .expect("decode whoami response");
                assert_eq!(whoami.user.id, actor.user_id);
            }
            Operation::CreateWorkspace { actor: creator, output, membership_output, name } => {
                let response = actor
                    .browser
                    .post(format!("{api}/workspaces"))
                    .json(&CreateWorkspaceRequest {
                        name: format!("{namespace}-{name}"),
                        description: None,
                    })
                    .send()
                    .await
                    .expect("send create-workspace request");
                let workspace = response
                    .error_for_status()
                    .expect("workspace creation succeeds")
                    .json::<WorkspaceResponse>()
                    .await
                    .expect("decode create-workspace response")
                    .workspace;
                assert_eq!(workspace.status, ArchiveStatus::Active);
                resources.bind(*output, workspace.id).expect("bind workspace handle");

                let memberships = actor
                    .browser
                    .get(format!("{api}/workspaces/{}/memberships?limit=200", workspace.id))
                    .send()
                    .await
                    .expect("send initial workspace membership request")
                    .error_for_status()
                    .expect("initial workspace membership request succeeds")
                    .json::<ListResponse<WorkspaceMembership>>()
                    .await
                    .expect("decode initial workspace membership response");
                let creator_id = fixture.actors.get(*creator).user_id;
                let creator_membership = memberships
                    .items
                    .iter()
                    .find(|membership| membership.user_id == creator_id)
                    .expect("workspace creator has initial administrator membership");
                assert_eq!(creator_membership.workspace_role, kival_sdk::MembershipRole::Admin);
                resources
                    .bind(*membership_output, creator_membership.id)
                    .expect("bind initial workspace membership handle");
            }
            Operation::ListWorkspaces { .. } => {
                let context =
                    serde_json::to_string(operation).expect("serialize operation context");
                let response = fetch_list::<Workspace>(
                    actor,
                    &format!("{api}/workspaces?status=all&limit=200"),
                    ExpectedOutcome::Success,
                    &context,
                )
                .await
                .expect("workspace collection is always readable");
                let actual = response.iter().map(|workspace| workspace.id).collect::<BTreeSet<_>>();
                let expected =
                    resolve_handles(resources, reference.visible_workspaces(operation.actor()));
                assert!(
                    expected.is_subset(&actual),
                    "workspace collection omits modeled visible workspaces"
                );
                if operation.actor() != Actor::Admin {
                    assert_eq!(actual, expected, "regular-user workspace collection visibility");
                }
            }
            Operation::CreateGroup { output, name, .. } => {
                let response = actor
                    .browser
                    .post(format!("{api}/groups"))
                    .json(&CreateGroupRequest {
                        name: format!("{namespace}-{name}"),
                        description: Some("stateful group".to_owned()),
                    })
                    .send()
                    .await
                    .expect("send create-group request")
                    .error_for_status()
                    .expect("group creation succeeds")
                    .json::<GroupResponse>()
                    .await
                    .expect("decode create-group response")
                    .group;
                assert_eq!(response.status, ArchiveStatus::Active);
                resources.bind(*output, response.id).expect("bind group handle");
            }
            Operation::ListGroups { .. } => {
                let context =
                    serde_json::to_string(operation).expect("serialize operation context");
                let response = fetch_list::<Group>(
                    actor,
                    &format!("{api}/groups?status=all&limit=200"),
                    ExpectedOutcome::Success,
                    &context,
                )
                .await
                .expect("group collection is always readable");
                let actual = response.iter().map(|group| group.id).collect::<BTreeSet<_>>();
                let expected =
                    resolve_handles(resources, reference.visible_groups(operation.actor()));
                assert!(expected.is_subset(&actual), "group collection omits modeled groups");
                if operation.actor() != Actor::Admin {
                    assert_eq!(actual, expected, "regular-user group collection visibility");
                }
            }
            Operation::GetGroup { group, .. } => {
                let group_id = resources.resolve(*group).expect("resolve group handle");
                let response = actor
                    .browser
                    .get(format!("{api}/groups/{group_id}"))
                    .send()
                    .await
                    .expect("send get-group request");
                let expected = if reference.can_read_group(*group, operation.actor()) {
                    ExpectedOutcome::Success
                } else {
                    ExpectedOutcome::Forbidden
                };
                let response = assert_http_outcome(response, expected, operation).await;
                if expected != ExpectedOutcome::Success {
                    return;
                }
                let response = response.json::<GroupResponse>("decode get-group response").group;
                assert_eq!(response.id, group_id);
                assert_eq!(
                    response.name,
                    format!(
                        "{namespace}-{}",
                        reference.group_name(*group).expect("modeled group name")
                    )
                );
                assert_eq!(response.status, expected_group_status(reference, *group));
            }
            Operation::UpdateGroup { group, name, .. } => {
                let group_id = resources.resolve(*group).expect("resolve group handle");
                let response = actor
                    .browser
                    .patch(format!("{api}/groups/{group_id}"))
                    .json(&UpdateGroupRequest {
                        name: Some(format!("{namespace}-{name}")),
                        description: Default::default(),
                    })
                    .send()
                    .await
                    .expect("send update-group request")
                    .error_for_status()
                    .expect("group update succeeds")
                    .json::<GroupResponse>()
                    .await
                    .expect("decode updated group")
                    .group;
                assert_eq!(response.id, group_id);
                assert_eq!(response.name, format!("{namespace}-{name}"));
                assert_eq!(response.status, ArchiveStatus::Active);
            }
            Operation::ArchiveGroup { group, .. } => {
                let group_id = resources.resolve(*group).expect("resolve group handle");
                let response = actor
                    .browser
                    .post(format!("{api}/groups/{group_id}/archive"))
                    .send()
                    .await
                    .expect("send archive-group request")
                    .error_for_status()
                    .expect("group archive succeeds")
                    .json::<GroupResponse>()
                    .await
                    .expect("decode archived group")
                    .group;
                assert_eq!(response.id, group_id);
                assert_eq!(response.status, ArchiveStatus::Archived);
                resources.archive(*group).expect("archive group handle");
                assert_group_visibility(
                    fixture, resources, &api, namespace, reference, *group, operation,
                )
                .await;
                assert_group_principal_access(
                    fixture, resources, &api, reference, *group, operation,
                )
                .await;
            }
            Operation::UnarchiveGroup { group, .. } => {
                let group_id = resources.resolve(*group).expect("resolve group handle");
                let response = actor
                    .browser
                    .post(format!("{api}/groups/{group_id}/unarchive"))
                    .send()
                    .await
                    .expect("send unarchive-group request")
                    .error_for_status()
                    .expect("group restore succeeds")
                    .json::<GroupResponse>()
                    .await
                    .expect("decode restored group")
                    .group;
                assert_eq!(response.id, group_id);
                assert_eq!(response.status, ArchiveStatus::Active);
                resources.unarchive(*group).expect("unarchive group handle");
            }
            Operation::ListGroupMemberships { group, .. } => {
                let group_id = resources.resolve(*group).expect("resolve group handle");
                let expected = if reference.group(*group) == Some(Lifecycle::Archived) {
                    ExpectedOutcome::NotFound
                } else if reference.can_admin_group(*group, operation.actor()) {
                    ExpectedOutcome::Success
                } else {
                    ExpectedOutcome::Forbidden
                };
                let context =
                    serde_json::to_string(operation).expect("serialize operation context");
                let response = fetch_list::<GroupMembership>(
                    actor,
                    &format!("{api}/groups/{group_id}/memberships?limit=200"),
                    expected,
                    &context,
                )
                .await;
                let Some(response) = response else {
                    return;
                };
                let actual =
                    response.iter().map(|membership| membership.id).collect::<BTreeSet<_>>();
                let expected_handles = reference.active_group_memberships(*group);
                let expected = resolve_handles(resources, expected_handles.clone());
                assert_eq!(actual, expected, "active group-membership projection");
                for handle in expected_handles {
                    let membership_id =
                        resources.resolve(handle).expect("resolve listed group membership");
                    let membership = response
                        .iter()
                        .find(|membership| membership.id == membership_id)
                        .expect("modeled group membership appears in the response");
                    let (_, member, role, active) =
                        reference.group_membership(handle).expect("modeled group membership");
                    assert!(active);
                    assert_eq!(membership.user_id, fixture.actors.get(member).user_id);
                    assert_eq!(membership.group_role, role);
                }
            }
            Operation::CreateGroupMembership { group, member, role, output, .. } => {
                let group_id = resources.resolve(*group).expect("resolve group handle");
                let member_client = fixture.actors.get(*member);
                let response = actor
                    .browser
                    .post(format!("{api}/groups/{group_id}/memberships"))
                    .json(&CreateGroupMembershipRequest {
                        user_id: Some(member_client.user_id),
                        username: None,
                        group_role: *role,
                    })
                    .send()
                    .await
                    .expect("send create-group-membership request")
                    .error_for_status()
                    .expect("group membership creation succeeds")
                    .json::<GroupMembershipResponse>()
                    .await
                    .expect("decode group-membership response")
                    .membership;
                assert_eq!(response.group_id, group_id);
                assert_eq!(response.user_id, member_client.user_id);
                assert_eq!(response.group_role, *role);
                assert!(response.revoked_at.is_none());
                resources.bind(*output, response.id).expect("bind group-membership handle");
                assert_group_visibility(
                    fixture, resources, &api, namespace, reference, *group, operation,
                )
                .await;
                assert_group_principal_access(
                    fixture, resources, &api, reference, *group, operation,
                )
                .await;
            }
            Operation::RevokeGroupMembership { group, membership, .. } => {
                let group_id = resources.resolve(*group).expect("resolve group handle");
                let membership_id =
                    resources.resolve(*membership).expect("resolve group-membership handle");
                let response = actor
                    .browser
                    .post(format!("{api}/groups/{group_id}/memberships/{membership_id}/revoke"))
                    .send()
                    .await
                    .expect("send revoke-group-membership request")
                    .error_for_status()
                    .expect("group membership revocation succeeds")
                    .json::<GroupMembershipResponse>()
                    .await
                    .expect("decode revoked group-membership response")
                    .membership;
                assert_eq!(response.id, membership_id);
                assert!(response.revoked_at.is_some());
                resources.archive(*membership).expect("retire group-membership handle");
                assert_group_principal_access(
                    fixture, resources, &api, reference, *group, operation,
                )
                .await;
            }
            Operation::UpdateGroupMembership { group, membership, role, output, .. } => {
                let group_id = resources.resolve(*group).expect("resolve group handle");
                let membership_id =
                    resources.resolve(*membership).expect("resolve group-membership handle");
                let (_, member, _, _) =
                    reference.group_membership(*membership).expect("modeled group membership");
                let response = actor
                    .browser
                    .patch(format!("{api}/groups/{group_id}/memberships/{membership_id}"))
                    .json(&UpdateGroupMembershipRequest { group_role: *role })
                    .send()
                    .await
                    .expect("send update-group-membership request")
                    .error_for_status()
                    .expect("group membership update succeeds")
                    .json::<GroupMembershipResponse>()
                    .await
                    .expect("decode updated group-membership response")
                    .membership;
                assert_ne!(response.id, membership_id);
                assert_eq!(response.group_id, group_id);
                assert_eq!(response.user_id, fixture.actors.get(member).user_id);
                assert_eq!(response.group_role, *role);
                assert!(response.revoked_at.is_none());
                resources.archive(*membership).expect("retire replaced group membership");
                resources
                    .bind(*output, response.id)
                    .expect("bind replacement group-membership handle");
                assert_group_principal_access(
                    fixture, resources, &api, reference, *group, operation,
                )
                .await;
            }
            Operation::ProbeArchivedGroupMembershipWrites { group, membership, member, .. } => {
                let group_id = resources.resolve(*group).expect("resolve group handle");
                let membership_id =
                    resources.resolve(*membership).expect("resolve group-membership handle");
                let member = fixture.actors.get(*member);

                let create = actor
                    .browser
                    .post(format!("{api}/groups/{group_id}/memberships"))
                    .json(&CreateGroupMembershipRequest {
                        user_id: Some(member.user_id),
                        username: None,
                        group_role: kival_sdk::MembershipRole::Member,
                    })
                    .send()
                    .await
                    .expect("send archived-group membership creation");
                assert_http_outcome(create, ExpectedOutcome::NotFound, operation).await;

                let revoke = actor
                    .browser
                    .post(format!("{api}/groups/{group_id}/memberships/{membership_id}/revoke"))
                    .send()
                    .await
                    .expect("send archived-group membership revocation");
                assert_http_outcome(revoke, ExpectedOutcome::NotFound, operation).await;

                let update = actor
                    .browser
                    .patch(format!("{api}/groups/{group_id}/memberships/{membership_id}"))
                    .json(&UpdateGroupMembershipRequest {
                        group_role: kival_sdk::MembershipRole::Admin,
                    })
                    .send()
                    .await
                    .expect("send archived-group membership replacement");
                assert_http_outcome(update, ExpectedOutcome::NotFound, operation).await;
            }
            Operation::LinkWorkspaceGroup { workspace, group, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let group_id = resources.resolve(*group).expect("resolve group handle");
                let response = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/groups"))
                    .json(&CreateWorkspaceGroupRequest { group_id })
                    .send()
                    .await
                    .expect("send link-workspace-group request")
                    .error_for_status()
                    .expect("workspace group link succeeds")
                    .json::<WorkspaceGroupResponse>()
                    .await
                    .expect("decode workspace-group response")
                    .workspace_group;
                assert_eq!(response.workspace_id, workspace_id);
                assert_eq!(response.group_id, group_id);
                assert_eq!(response.status, ArchiveStatus::Active);
                assert_workspace_group_listed(actor, &api, workspace_id, group_id, true).await;
                assert_group_principal_access(
                    fixture, resources, &api, reference, *group, operation,
                )
                .await;
            }
            Operation::ArchiveWorkspaceGroup { workspace, group, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let group_id = resources.resolve(*group).expect("resolve group handle");
                let response = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/groups/{group_id}/archive"))
                    .send()
                    .await
                    .expect("send archive-workspace-group request")
                    .error_for_status()
                    .expect("workspace group archive succeeds")
                    .json::<WorkspaceGroupResponse>()
                    .await
                    .expect("decode archived workspace-group response")
                    .workspace_group;
                assert_eq!(response.status, ArchiveStatus::Archived);
                assert_workspace_group_listed(actor, &api, workspace_id, group_id, false).await;
                assert_group_principal_access(
                    fixture, resources, &api, reference, *group, operation,
                )
                .await;
            }
            Operation::UnarchiveWorkspaceGroup { workspace, group, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let group_id = resources.resolve(*group).expect("resolve group handle");
                let response = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/groups/{group_id}/unarchive"))
                    .send()
                    .await
                    .expect("send unarchive-workspace-group request")
                    .error_for_status()
                    .expect("workspace group restore succeeds")
                    .json::<WorkspaceGroupResponse>()
                    .await
                    .expect("decode restored workspace-group response")
                    .workspace_group;
                assert_eq!(response.status, ArchiveStatus::Active);
                assert_workspace_group_listed(actor, &api, workspace_id, group_id, true).await;
                assert_group_principal_access(
                    fixture, resources, &api, reference, *group, operation,
                )
                .await;
            }
            Operation::GetWorkspace { workspace, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let response = actor
                    .browser
                    .get(format!("{api}/workspaces/{workspace_id}"))
                    .send()
                    .await
                    .expect("send get-workspace request");
                let expected = if reference.can_read_workspace(*workspace, operation.actor()) {
                    ExpectedOutcome::Success
                } else {
                    ExpectedOutcome::Forbidden
                };
                let response = assert_http_outcome(response, expected, operation).await;
                if expected != ExpectedOutcome::Success {
                    return;
                }
                let actual =
                    response.json::<WorkspaceResponse>("decode get-workspace response").workspace;
                assert_eq!(actual.id, workspace_id);
                assert_eq!(actual.status, expected_status(reference, *workspace));
            }
            Operation::GetWorkspaceGraph { workspace, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let response = actor
                    .browser
                    .get(format!(
                        "{api}/workspaces/{workspace_id}/graph?limit_nodes=1000&limit_edges=3000"
                    ))
                    .send()
                    .await
                    .expect("send workspace-graph request");
                let expected = active_workspace_outcome(
                    reference,
                    *workspace,
                    reference.can_use_workspace(*workspace, operation.actor()),
                );
                let response = assert_http_outcome(response, expected, operation).await;
                if expected != ExpectedOutcome::Success {
                    return;
                }
                let response =
                    response.json::<WorkspaceGraphResponse>("decode workspace-graph response");
                assert_eq!(response.workspace_id, workspace_id);
                let expected_nodes = resolve_handles(
                    resources,
                    reference.visible_active_objects(*workspace, operation.actor()),
                );
                let actual_nodes =
                    response.nodes.iter().map(|node| node.id).collect::<BTreeSet<_>>();
                assert_eq!(actual_nodes, expected_nodes, "workspace graph node projection");
                let expected_edges = resolve_handles(
                    resources,
                    reference.visible_active_edges(*workspace, operation.actor()),
                );
                let actual_edges =
                    response.edges.iter().map(|edge| edge.id).collect::<BTreeSet<_>>();
                assert_eq!(actual_edges, expected_edges, "workspace graph edge projection");
                assert!(!response.limits.has_more_nodes);
                assert!(!response.limits.has_more_edges);
            }
            Operation::SearchWorkspace { workspace, object, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let title = reference.object_title(*object).expect("modeled object title");
                let response = actor
                    .browser
                    .get(format!(
                        "{api}/workspaces/{workspace_id}/search?q={title}&categories=title&mode=exact&limit=100"
                    ))
                    .send()
                    .await
                    .expect("send workspace-search request");
                let expected = active_workspace_outcome(
                    reference,
                    *workspace,
                    reference.can_use_workspace(*workspace, operation.actor()),
                );
                let response = assert_http_outcome(response, expected, operation).await;
                if expected != ExpectedOutcome::Success {
                    return;
                }
                let response = response.json::<SearchResponse>("decode workspace search");
                let object_is_active =
                    reference.object(*object).expect("modeled object").1 == Lifecycle::Active;
                let expected_hit =
                    object_is_active && reference.can_read_object(*object, operation.actor());
                assert_eq!(
                    response.items.iter().any(|hit| hit.object_id == object_id),
                    expected_hit,
                    "exact-title search visibility disagrees with the model"
                );
            }
            Operation::GetWorkspaceEvents { workspace, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let response = actor
                    .browser
                    .get(format!("{api}/workspaces/{workspace_id}/events?limit=100&order=asc"))
                    .send()
                    .await
                    .expect("send workspace-events request");
                let expected = active_workspace_outcome(
                    reference,
                    *workspace,
                    reference.can_admin_workspace(*workspace, operation.actor()),
                );
                let response = assert_http_outcome(response, expected, operation).await;
                if expected != ExpectedOutcome::Success {
                    return;
                }
                let response = response.json::<ListResponse<Event>>("decode workspace events");
                assert!(!response.items.is_empty(), "workspace has a creation event");
                assert!(
                    response.items.iter().all(|event| event.workspace_id == Some(workspace_id))
                );
                assert!(
                    response
                        .items
                        .windows(2)
                        .all(|events| events[0].sequence_number < events[1].sequence_number)
                );
            }
            Operation::ListWorkspaceMemberships { workspace, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let expected = active_workspace_outcome(
                    reference,
                    *workspace,
                    reference.can_read_workspace(*workspace, operation.actor()),
                );
                let context =
                    serde_json::to_string(operation).expect("serialize operation context");
                let response = fetch_list::<WorkspaceMembership>(
                    actor,
                    &format!("{api}/workspaces/{workspace_id}/memberships?limit=200"),
                    expected,
                    &context,
                )
                .await;
                let Some(response) = response else {
                    return;
                };
                let actual =
                    response.iter().map(|membership| membership.id).collect::<BTreeSet<_>>();
                let expected =
                    resolve_handles(resources, reference.active_workspace_memberships(*workspace));
                assert_eq!(actual, expected, "workspace membership collection projection");
            }
            Operation::ListWorkspaceGroups { workspace, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let expected = active_workspace_outcome(
                    reference,
                    *workspace,
                    reference.can_read_workspace(*workspace, operation.actor()),
                );
                let context =
                    serde_json::to_string(operation).expect("serialize operation context");
                let response = fetch_list::<WorkspaceGroup>(
                    actor,
                    &format!("{api}/workspaces/{workspace_id}/groups?status=all&limit=200"),
                    expected,
                    &context,
                )
                .await;
                let Some(response) = response else {
                    return;
                };
                let mut actual =
                    response.iter().map(|link| (link.group_id, link.status)).collect::<Vec<_>>();
                actual.sort_by_key(|(group_id, _)| *group_id);
                let mut expected = reference
                    .workspace_group_lifecycles(*workspace)
                    .into_iter()
                    .map(|(group, lifecycle)| {
                        (
                            resources.resolve(group).expect("resolve linked group"),
                            match lifecycle {
                                Lifecycle::Active => ArchiveStatus::Active,
                                Lifecycle::Archived => ArchiveStatus::Archived,
                            },
                        )
                    })
                    .collect::<Vec<_>>();
                expected.sort_by_key(|(group_id, _)| *group_id);
                assert_eq!(actual, expected, "workspace group collection projection");
            }
            Operation::UpdateWorkspace { workspace, name, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let response = actor
                    .browser
                    .patch(format!("{api}/workspaces/{workspace_id}"))
                    .json(&UpdateWorkspaceRequest {
                        name: Some(format!("{namespace}-{name}")),
                        description: Default::default(),
                    })
                    .send()
                    .await
                    .expect("send update-workspace request")
                    .error_for_status()
                    .expect("workspace update succeeds")
                    .json::<WorkspaceResponse>()
                    .await
                    .expect("decode updated workspace")
                    .workspace;
                assert_eq!(response.id, workspace_id);
                assert_eq!(response.name, format!("{namespace}-{name}"));
            }
            Operation::ArchiveWorkspace { workspace, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let response = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/archive"))
                    .send()
                    .await
                    .expect("send archive-workspace request");
                let actual = response
                    .error_for_status()
                    .expect("workspace archive succeeds")
                    .json::<WorkspaceResponse>()
                    .await
                    .expect("decode archive-workspace response")
                    .workspace;
                assert_eq!(actual.status, ArchiveStatus::Archived);
                resources.archive(*workspace).expect("archive workspace handle");
                assert_workspace_visibility_and_access(
                    fixture, resources, &api, namespace, reference, *workspace, operation,
                )
                .await;
            }
            Operation::UnarchiveWorkspace { workspace, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let response = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/unarchive"))
                    .send()
                    .await
                    .expect("send unarchive-workspace request");
                let actual = response
                    .error_for_status()
                    .expect("workspace restore succeeds")
                    .json::<WorkspaceResponse>()
                    .await
                    .expect("decode unarchive-workspace response")
                    .workspace;
                assert_eq!(actual.status, ArchiveStatus::Active);
                resources.unarchive(*workspace).expect("unarchive workspace handle");
                assert_workspace_visibility_and_access(
                    fixture, resources, &api, namespace, reference, *workspace, operation,
                )
                .await;
            }
            Operation::CreateWorkspaceMembership { workspace, member, role, output, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let member_client = fixture.actors.get(*member);
                let response = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/memberships"))
                    .json(&CreateWorkspaceMembershipRequest {
                        user_id: Some(member_client.user_id),
                        username: None,
                        workspace_role: *role,
                    })
                    .send()
                    .await
                    .expect("send create-workspace-membership request")
                    .error_for_status()
                    .expect("workspace membership creation succeeds")
                    .json::<WorkspaceMembershipResponse>()
                    .await
                    .expect("decode workspace-membership response")
                    .membership;
                assert_eq!(response.workspace_id, workspace_id);
                assert_eq!(response.user_id, member_client.user_id);
                assert_eq!(response.workspace_role, *role);
                assert!(response.revoked_at.is_none());
                resources.bind(*output, response.id).expect("bind membership handle");
                assert_membership_listed(
                    fixture.actors.get(Actor::Admin),
                    &api,
                    workspace_id,
                    response.id,
                    true,
                )
                .await;

                let member_workspace = member_client
                    .browser
                    .get(format!("{api}/workspaces/{workspace_id}"))
                    .send()
                    .await
                    .expect("send member workspace read");
                assert_http_outcome(member_workspace, ExpectedOutcome::Success, operation).await;
                assert_workspace_visibility_and_access(
                    fixture, resources, &api, namespace, reference, *workspace, operation,
                )
                .await;
            }
            Operation::RevokeWorkspaceMembership { workspace, membership, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let membership_id =
                    resources.resolve(*membership).expect("resolve membership handle");
                let response = actor
                    .browser
                    .post(format!(
                        "{api}/workspaces/{workspace_id}/memberships/{membership_id}/revoke"
                    ))
                    .send()
                    .await
                    .expect("send revoke-workspace-membership request")
                    .error_for_status()
                    .expect("workspace membership revocation succeeds")
                    .json::<WorkspaceMembershipResponse>()
                    .await
                    .expect("decode revoked workspace-membership response")
                    .membership;
                assert_eq!(response.id, membership_id);
                assert_eq!(response.revoked_by, Some(actor.user_id));
                assert!(response.revoked_at.is_some());
                resources.archive(*membership).expect("retire membership handle");
                assert_membership_listed(
                    fixture.actors.get(Actor::Admin),
                    &api,
                    workspace_id,
                    membership_id,
                    false,
                )
                .await;

                let (_, revoked_actor, _, active) =
                    reference.membership(*membership).expect("modeled membership");
                assert!(!active);
                let revoked_client = fixture.actors.get(revoked_actor);
                let revoked_access = revoked_client
                    .browser
                    .get(format!("{api}/workspaces/{workspace_id}"))
                    .send()
                    .await
                    .expect("send revoked-member workspace read");
                let expected = if reference.can_read_workspace(*workspace, revoked_actor) {
                    ExpectedOutcome::Success
                } else {
                    ExpectedOutcome::Forbidden
                };
                assert_http_outcome(revoked_access, expected, operation).await;
                assert_workspace_visibility_and_access(
                    fixture, resources, &api, namespace, reference, *workspace, operation,
                )
                .await;
            }
            Operation::UpdateWorkspaceMembership {
                workspace, membership, role, output, ..
            } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let membership_id =
                    resources.resolve(*membership).expect("resolve membership handle");
                let (_, member, _, _) =
                    reference.membership(*membership).expect("modeled workspace membership");
                let response = actor
                    .browser
                    .patch(format!("{api}/workspaces/{workspace_id}/memberships/{membership_id}"))
                    .json(&UpdateWorkspaceMembershipRequest { workspace_role: *role })
                    .send()
                    .await
                    .expect("send update-workspace-membership request")
                    .error_for_status()
                    .expect("workspace membership update succeeds")
                    .json::<WorkspaceMembershipResponse>()
                    .await
                    .expect("decode updated workspace-membership response")
                    .membership;
                assert_ne!(response.id, membership_id);
                assert_eq!(response.workspace_id, workspace_id);
                assert_eq!(response.user_id, fixture.actors.get(member).user_id);
                assert_eq!(response.workspace_role, *role);
                assert!(response.revoked_at.is_none());
                resources.archive(*membership).expect("retire replaced membership");
                resources.bind(*output, response.id).expect("bind replacement membership handle");
                assert_membership_listed(
                    fixture.actors.get(Actor::Admin),
                    &api,
                    workspace_id,
                    membership_id,
                    false,
                )
                .await;
                assert_membership_listed(
                    fixture.actors.get(Actor::Admin),
                    &api,
                    workspace_id,
                    response.id,
                    true,
                )
                .await;
                assert_workspace_visibility_and_access(
                    fixture, resources, &api, namespace, reference, *workspace, operation,
                )
                .await;
            }
            Operation::CreateObjectGrant { workspace, object, principal, role, output, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let principal_client = fixture.actors.get(*principal);
                let response = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/objects/{object_id}/grants"))
                    .json(&CreateObjectGrantRequest {
                        principal: GrantPrincipal::User(principal_client.user_id),
                        object_role: *role,
                    })
                    .send()
                    .await
                    .expect("send create-object-grant request")
                    .error_for_status()
                    .expect("object grant creation succeeds")
                    .json::<ObjectGrantResponse>()
                    .await
                    .expect("decode object-grant response")
                    .grant;
                assert_eq!(response.workspace_id, workspace_id);
                assert_eq!(response.object_id, object_id);
                assert_eq!(response.principal_user_id, Some(principal_client.user_id));
                assert_eq!(response.object_role, *role);
                assert_eq!(response.created_by, Some(actor.user_id));
                assert!(response.revoked_at.is_none());
                resources.bind(*output, response.id).expect("bind grant handle");
                assert_grant_listed(
                    fixture.actors.admin(),
                    &api,
                    workspace_id,
                    object_id,
                    response.id,
                    true,
                )
                .await;
                assert_actor_object_access(
                    principal_client,
                    &api,
                    workspace_id,
                    object_id,
                    reference.object_role(*object, *principal),
                    ExpectedOutcome::Success,
                    operation,
                )
                .await;
            }
            Operation::CreateGroupObjectGrant {
                workspace,
                object,
                principal,
                role,
                output,
                ..
            } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let group_id = resources.resolve(*principal).expect("resolve group handle");
                let response = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/objects/{object_id}/grants"))
                    .json(&CreateObjectGrantRequest {
                        principal: GrantPrincipal::Group(group_id),
                        object_role: *role,
                    })
                    .send()
                    .await
                    .expect("send create-group-object-grant request")
                    .error_for_status()
                    .expect("group object grant creation succeeds")
                    .json::<ObjectGrantResponse>()
                    .await
                    .expect("decode group object-grant response")
                    .grant;
                assert_eq!(response.workspace_id, workspace_id);
                assert_eq!(response.object_id, object_id);
                assert_eq!(response.principal_group_id, Some(group_id));
                assert_eq!(response.object_role, *role);
                resources.bind(*output, response.id).expect("bind group grant handle");
                assert_grant_listed(
                    fixture.actors.admin(),
                    &api,
                    workspace_id,
                    object_id,
                    response.id,
                    true,
                )
                .await;
                assert_group_principal_access(
                    fixture, resources, &api, reference, *principal, operation,
                )
                .await;
            }
            Operation::RevokeObjectGrant { workspace, object, grant, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let grant_id = resources.resolve(*grant).expect("resolve grant handle");
                let response = actor
                    .browser
                    .post(format!(
                        "{api}/workspaces/{workspace_id}/objects/{object_id}/grants/{grant_id}/revoke"
                    ))
                    .send()
                    .await
                    .expect("send revoke-object-grant request");
                let (_, _, principal, _, active) =
                    reference.grant(*grant).expect("modeled object grant");
                let expected =
                    if active { ExpectedOutcome::Conflict } else { ExpectedOutcome::Success };
                let object_is_active =
                    reference.object(*object).expect("modeled object").1 == Lifecycle::Active;
                let response = assert_http_outcome(response, expected, operation).await;
                if expected != ExpectedOutcome::Success {
                    if object_is_active {
                        assert_grant_listed(
                            fixture.actors.admin(),
                            &api,
                            workspace_id,
                            object_id,
                            grant_id,
                            true,
                        )
                        .await;
                    }
                    return;
                }
                let response = response
                    .json::<ObjectGrantResponse>("decode revoked object-grant response")
                    .grant;
                assert_eq!(response.id, grant_id);
                assert_eq!(response.revoked_by, Some(actor.user_id));
                assert!(response.revoked_at.is_some());
                resources.archive(*grant).expect("retire grant handle");
                if object_is_active {
                    assert_grant_listed(
                        fixture.actors.admin(),
                        &api,
                        workspace_id,
                        object_id,
                        grant_id,
                        false,
                    )
                    .await;
                }

                assert!(!active);
                match principal {
                    Principal::User(principal) => {
                        assert_actor_object_access(
                            fixture.actors.get(principal),
                            &api,
                            workspace_id,
                            object_id,
                            reference.object_role(*object, principal),
                            readable_object_outcome(reference, *object, principal),
                            operation,
                        )
                        .await;
                    }
                    Principal::Group(group) => {
                        assert_group_principal_access(
                            fixture, resources, &api, reference, group, operation,
                        )
                        .await;
                    }
                }
            }
            Operation::UpdateObjectGrant { workspace, object, grant, role, output, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let grant_id = resources.resolve(*grant).expect("resolve grant handle");
                let response = actor
                    .browser
                    .patch(format!(
                        "{api}/workspaces/{workspace_id}/objects/{object_id}/grants/{grant_id}"
                    ))
                    .json(&UpdateObjectGrantRequest { object_role: *role })
                    .send()
                    .await
                    .expect("send update-object-grant request");
                let (_, _, principal, _, active) =
                    reference.grant(*grant).expect("modeled previous object grant");
                let expected =
                    if active { ExpectedOutcome::Conflict } else { ExpectedOutcome::Success };
                let object_is_active =
                    reference.object(*object).expect("modeled object").1 == Lifecycle::Active;
                let response = assert_http_outcome(response, expected, operation).await;
                if expected != ExpectedOutcome::Success {
                    if object_is_active {
                        assert_grant_listed(
                            fixture.actors.admin(),
                            &api,
                            workspace_id,
                            object_id,
                            grant_id,
                            true,
                        )
                        .await;
                    }
                    return;
                }
                let response = response
                    .json::<ObjectGrantResponse>("decode updated object-grant response")
                    .grant;
                assert_ne!(response.id, grant_id);
                assert_eq!(response.object_role, *role);
                match principal {
                    Principal::User(principal) => {
                        assert_eq!(
                            response.principal_user_id,
                            Some(fixture.actors.get(principal).user_id)
                        );
                    }
                    Principal::Group(group) => {
                        assert_eq!(
                            response.principal_group_id,
                            Some(resources.resolve(group).expect("resolve group principal"))
                        );
                    }
                }
                resources.archive(*grant).expect("retire replaced grant handle");
                resources.bind(*output, response.id).expect("bind replacement grant handle");
                if object_is_active {
                    assert_grant_listed(
                        fixture.actors.admin(),
                        &api,
                        workspace_id,
                        object_id,
                        grant_id,
                        false,
                    )
                    .await;
                    assert_grant_listed(
                        fixture.actors.admin(),
                        &api,
                        workspace_id,
                        object_id,
                        response.id,
                        true,
                    )
                    .await;
                }
                match principal {
                    Principal::User(principal) => {
                        assert_actor_object_access(
                            fixture.actors.get(principal),
                            &api,
                            workspace_id,
                            object_id,
                            reference.object_role(*object, principal),
                            readable_object_outcome(reference, *object, principal),
                            operation,
                        )
                        .await;
                    }
                    Principal::Group(group) => {
                        assert_group_principal_access(
                            fixture, resources, &api, reference, group, operation,
                        )
                        .await;
                    }
                }
            }
            Operation::CreateApiKey { workspace, output, scope, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                actor.browser.fresh_authenticate().await.expect("fresh-authenticate API-key owner");
                let response = actor
                    .browser
                    .post(format!("{api}/auth/api-keys"))
                    .json(&CreateApiKeyRequest {
                        label: format!("{namespace}-key-{}", output.index),
                        scopes: vec![*scope],
                        workspace_ids: vec![workspace_id],
                        expires_at: None,
                    })
                    .send()
                    .await
                    .expect("send create-api-key request")
                    .error_for_status()
                    .expect("API key creation succeeds")
                    .json::<CreateApiKeyResponse>()
                    .await
                    .expect("decode created API key");
                assert_eq!(response.api_key.user_id, actor.user_id);
                assert_eq!(response.api_key.authorization_revision, 0);
                assert_eq!(response.api_key.scopes, vec![*scope]);
                assert_eq!(response.api_key.workspace_ids, vec![workspace_id]);
                resources.bind(*output, response.api_key.id).expect("bind API-key handle");
                api_key_clients.insert(
                    *output,
                    ApiKeyClient::new(fixture.base_url.clone(), response.token)
                        .expect("construct modeled API-key client"),
                );
            }
            Operation::UpdateApiKey { key, scope, .. } => {
                let key_id = resources.resolve(*key).expect("resolve API-key handle");
                let modeled = reference.api_key(*key).expect("modeled API key");
                let workspace_id =
                    resources.resolve(modeled.workspace).expect("resolve delegated workspace");
                actor.browser.fresh_authenticate().await.expect("fresh-authenticate API-key owner");
                let response = actor
                    .browser
                    .patch(format!("{api}/auth/api-keys/{key_id}"))
                    .json(&UpdateApiKeyRequest {
                        authorization_revision: modeled.revision - 1,
                        scopes: vec![*scope],
                        workspace_ids: vec![workspace_id],
                    })
                    .send()
                    .await
                    .expect("send update-api-key request")
                    .error_for_status()
                    .expect("API key update succeeds")
                    .json::<ApiKeyResponse>()
                    .await
                    .expect("decode updated API key")
                    .api_key;
                assert_eq!(response.authorization_revision, modeled.revision);
                assert_eq!(response.scopes, vec![*scope]);
                assert_eq!(response.workspace_ids, vec![workspace_id]);

                let stale = actor
                    .browser
                    .patch(format!("{api}/auth/api-keys/{key_id}"))
                    .json(&UpdateApiKeyRequest {
                        authorization_revision: modeled.revision - 1,
                        scopes: vec![*scope],
                        workspace_ids: vec![workspace_id],
                    })
                    .send()
                    .await
                    .expect("send stale API-key update request");
                assert_eq!(
                    stale.status().as_u16(),
                    409,
                    "stale API-key authorization revision must conflict"
                );
            }
            Operation::RevokeApiKey { key, .. } => {
                let key_id = resources.resolve(*key).expect("resolve API-key handle");
                actor.browser.fresh_authenticate().await.expect("fresh-authenticate API-key owner");
                let response = actor
                    .browser
                    .post(format!("{api}/auth/api-keys/{key_id}/revoke"))
                    .send()
                    .await
                    .expect("send revoke-api-key request")
                    .error_for_status()
                    .expect("API key revocation succeeds")
                    .json::<ApiKeyResponse>()
                    .await
                    .expect("decode revoked API key")
                    .api_key;
                assert_eq!(response.id, key_id);
                assert!(response.revoked_at.is_some());

                let revoked = api_key_clients
                    .get(key)
                    .expect("modeled API-key client")
                    .get("/auth/whoami")
                    .send()
                    .await
                    .expect("send revoked API-key request");
                assert_eq!(revoked.status().as_u16(), 401, "revoked API key must not authenticate");
            }
            Operation::ProbeApiKeyAccess { key, workspace, object, .. } => {
                let modeled = reference.api_key(*key).expect("modeled API key");
                let client = api_key_clients.get(key).expect("modeled API-key client");
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");

                let workspace_response = client
                    .get(format!("{api}/workspaces/{workspace_id}"))
                    .send()
                    .await
                    .expect("send API-key workspace read");
                let workspace_status = if !modeled.active {
                    401
                } else if !modeled.scope.permits(ApiKeyScope::WorkspaceRead)
                    || !reference.can_read_workspace(*workspace, modeled.owner)
                {
                    403
                } else {
                    200
                };
                assert_eq!(
                    workspace_response.status().as_u16(),
                    workspace_status,
                    "API-key workspace authorization"
                );

                let object_response = client
                    .get(format!("{api}/workspaces/{workspace_id}/objects/{object_id}"))
                    .send()
                    .await
                    .expect("send API-key object read");
                let object_status = if !modeled.active {
                    401
                } else if !modeled.scope.permits(ApiKeyScope::ObjectRead) {
                    403
                } else {
                    readable_object_outcome(reference, *object, modeled.owner).status()
                };
                assert_eq!(
                    object_response.status().as_u16(),
                    object_status,
                    "API-key object authorization"
                );
            }
            Operation::CreateObject { workspace, output, creator_grant, title, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let response = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/objects"))
                    .json(&CreateObjectRequest {
                        title: title.clone(),
                        body: object_body(title),
                        metadata: serde_json::json!({ "stateful": true }),
                    })
                    .send()
                    .await
                    .expect("send create-object request")
                    .error_for_status()
                    .expect("object creation succeeds")
                    .json::<ObjectResponse>()
                    .await
                    .expect("decode create-object response");
                assert_eq!(response.object.workspace_id, workspace_id);
                assert_eq!(response.object.title, *title);
                assert_eq!(response.object.status, ArchiveStatus::Active);
                assert_eq!(response.object.created_by, Some(actor.user_id));
                assert_eq!(response.effective_role, ObjectRole::Admin);
                let object_id = response.object.id;
                let version = response.current_version.expect("created object has a version");
                assert_eq!(version.version_number, 1);
                assert_eq!(version.title, *title);
                assert_eq!(version.body, object_body(title));
                resources.bind(*output, object_id).expect("bind object handle");
                let context =
                    serde_json::to_string(operation).expect("serialize operation context");
                let grants = fetch_list::<ObjectGrant>(
                    actor,
                    &format!(
                        "{api}/workspaces/{workspace_id}/objects/{object_id}/grants?limit=200"
                    ),
                    ExpectedOutcome::Success,
                    &context,
                )
                .await
                .expect("creator-grant collection is readable");
                let creator = grants
                    .iter()
                    .find(|grant| {
                        grant.principal_user_id == Some(actor.user_id)
                            && grant.object_role == ObjectRole::Admin
                    })
                    .expect("automatic creator-admin grant");
                resources.bind(*creator_grant, creator.id).expect("bind creator-grant handle");
                assert_object_listed(actor, &api, workspace_id, object_id, *output, reference)
                    .await;
            }
            Operation::GetObject { workspace, object, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let response = actor
                    .browser
                    .get(format!("{api}/workspaces/{workspace_id}/objects/{object_id}"))
                    .send()
                    .await
                    .expect("send get-object request");
                let expected = readable_object_outcome(reference, *object, operation.actor());
                let response = assert_http_outcome(response, expected, operation).await;
                if expected != ExpectedOutcome::Success {
                    return;
                }
                let response = response.json::<ObjectResponse>("decode get-object response");
                assert_eq!(response.object.id, object_id);
                assert_eq!(response.object.workspace_id, workspace_id);
                assert_eq!(response.object.status, expected_object_status(reference, *object));
                let expected_title = reference.object_title(*object).expect("modeled object title");
                assert_eq!(response.object.title, expected_title);
                let version = response.current_version.expect("object has a current version");
                assert_eq!(
                    version.version_number,
                    reference.object_version(*object).expect("modeled object version")
                );
                assert_eq!(version.title, expected_title);
                assert_eq!(version.body, object_body(expected_title));
                assert_eq!(
                    response.effective_role,
                    reference.object_role(*object, operation.actor()).expect("modeled object role")
                );
            }
            Operation::GetObjectGraph { workspace, object, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let response = actor
                    .browser
                    .get(format!(
                        "{api}/workspaces/{workspace_id}/objects/{object_id}/graph?depth=1&direction=both&max_nodes=1000&max_edges=3000"
                    ))
                    .send()
                    .await
                    .expect("send object-graph request");
                let expected = active_object_outcome(
                    reference,
                    *object,
                    reference.can_read_object(*object, operation.actor()),
                );
                let response = assert_http_outcome(response, expected, operation).await;
                if expected != ExpectedOutcome::Success {
                    return;
                }
                let response = response.json::<ObjectGraphResponse>("decode object-graph response");
                assert_eq!(response.workspace_id, workspace_id);
                assert_eq!(response.root_object_id, object_id);
                assert!(!response.truncated);

                let visible_edges = reference.visible_active_edges(*workspace, operation.actor());
                let expected_node_handles =
                    immediate_graph_nodes(reference, *object, &visible_edges);
                let expected_nodes = resolve_handles(resources, expected_node_handles.clone());
                let actual_nodes =
                    response.nodes.iter().map(|node| node.id).collect::<BTreeSet<_>>();
                assert_eq!(actual_nodes, expected_nodes, "object graph node projection");

                let expected_edges: Vec<_> = visible_edges
                    .into_iter()
                    .filter(|edge| {
                        let (_, source, target, _) = reference.edge(*edge).expect("modeled edge");
                        expected_node_handles.contains(&source)
                            && expected_node_handles.contains(&target)
                    })
                    .collect();
                let expected_edges = resolve_handles(resources, expected_edges);
                let actual_edges =
                    response.edges.iter().map(|edge| edge.id).collect::<BTreeSet<_>>();
                assert_eq!(actual_edges, expected_edges, "object graph edge projection");
            }
            Operation::GetObjectBacklinks { workspace, object, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let response = actor
                    .browser
                    .get(format!(
                        "{api}/workspaces/{workspace_id}/objects/{object_id}/backlinks?limit=100"
                    ))
                    .send()
                    .await
                    .expect("send object-backlinks request");
                let expected = readable_object_outcome(reference, *object, operation.actor());
                let response = assert_http_outcome(response, expected, operation).await;
                if expected != ExpectedOutcome::Success {
                    return;
                }
                let response =
                    response.json::<ObjectBacklinksResponse>("decode object-backlinks response");
                assert_eq!(response.object_id, object_id);
                let expected_edges = resolve_handles(
                    resources,
                    reference.visible_incoming_edges(*object, operation.actor()),
                );
                let actual_edges: BTreeSet<_> =
                    response.incoming_edges.iter().map(|edge| edge.edge_id).collect();
                assert_eq!(actual_edges, expected_edges, "backlink edge projection");
                assert!(
                    response.incoming_references.is_empty(),
                    "generated object bodies contain no textual references"
                );
            }
            Operation::GetObjectEvents { workspace, object, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let response = actor
                    .browser
                    .get(format!(
                        "{api}/workspaces/{workspace_id}/objects/{object_id}/events?limit=100&order=asc"
                    ))
                    .send()
                    .await
                    .expect("send object-events request");
                let expected = readable_object_outcome(reference, *object, operation.actor());
                let response = assert_http_outcome(response, expected, operation).await;
                if expected != ExpectedOutcome::Success {
                    return;
                }
                let response = response.json::<ListResponse<Event>>("decode object events");
                assert!(!response.items.is_empty(), "object has a creation event");
                assert!(response.items.iter().all(|event| {
                    event.workspace_id == Some(workspace_id) && event.object_id == Some(object_id)
                }));
                assert!(
                    response
                        .items
                        .windows(2)
                        .all(|events| events[0].sequence_number < events[1].sequence_number)
                );
            }
            Operation::GetObjectVersion { workspace, object, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let expected = readable_object_outcome(reference, *object, operation.actor());
                let context =
                    serde_json::to_string(operation).expect("serialize operation context");
                let versions = fetch_list::<ObjectVersion>(
                    actor,
                    &format!(
                        "{api}/workspaces/{workspace_id}/objects/{object_id}/versions?limit=200"
                    ),
                    expected,
                    &context,
                )
                .await;
                let Some(versions) = versions else {
                    return;
                };
                for version in &versions {
                    let author = reference
                        .object_version_author(*object, version.version_number)
                        .expect("modeled version author");
                    let author_identity = fixture.identities.get(author);
                    assert_eq!(version.created_by, Some(author_identity.user_id));
                    assert_eq!(
                        version.created_by_username.as_deref(),
                        Some(author_identity.username.as_str())
                    );
                    assert!(version.created_by_display_name.is_some());
                    assert_eq!(
                        version.created_by_workspace_role,
                        reference.workspace_role(*workspace, author)
                    );
                    assert_eq!(
                        version.created_by_object_role,
                        reference.object_role(*object, author)
                    );
                }

                let version_number =
                    reference.object_version(*object).expect("modeled object version");
                let version = versions
                    .iter()
                    .find(|version| version.version_number == version_number)
                    .expect("current version appears in version list");
                let response = actor
                    .browser
                    .get(format!(
                        "{api}/workspaces/{workspace_id}/objects/{object_id}/versions/{}",
                        version.id
                    ))
                    .send()
                    .await
                    .expect("send get-object-version request");
                let response =
                    assert_http_outcome(response, ExpectedOutcome::Success, operation).await;
                let response = response
                    .json::<ObjectVersionResponse>("decode object-version response")
                    .version;
                assert_eq!(response.id, version.id);
                assert_eq!(response.object_id, object_id);
                assert_eq!(response.version_number, version_number);
                let author = reference
                    .object_version_author(*object, version_number)
                    .expect("modeled version author");
                let author_identity = fixture.identities.get(author);
                assert_eq!(response.created_by, Some(author_identity.user_id));
                assert_eq!(
                    response.created_by_username.as_deref(),
                    Some(author_identity.username.as_str())
                );
                assert!(response.created_by_display_name.is_some());
                assert_eq!(
                    response.created_by_workspace_role,
                    reference.workspace_role(*workspace, author)
                );
                assert_eq!(response.created_by_object_role, reference.object_role(*object, author));
                let title = reference.object_title(*object).expect("modeled object title");
                assert_eq!(response.title, title);
                assert_eq!(response.body, object_body(title));
            }
            Operation::ListObjectGrants { workspace, object, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let expected = active_object_outcome(
                    reference,
                    *object,
                    reference.can_admin_object(*object, operation.actor()),
                );
                let context =
                    serde_json::to_string(operation).expect("serialize operation context");
                let response = fetch_list::<ObjectGrant>(
                    actor,
                    &format!(
                        "{api}/workspaces/{workspace_id}/objects/{object_id}/grants?limit=200"
                    ),
                    expected,
                    &context,
                )
                .await;
                let Some(response) = response else {
                    return;
                };
                let actual = response.iter().map(|grant| grant.id).collect::<BTreeSet<_>>();
                let expected = resolve_handles(resources, reference.active_object_grants(*object));
                assert_eq!(actual, expected, "object grant collection projection");
            }
            Operation::ListObjectEdges { workspace, object, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let expected = active_object_outcome(
                    reference,
                    *object,
                    reference.can_read_object(*object, operation.actor()),
                );
                let context =
                    serde_json::to_string(operation).expect("serialize operation context");
                let response = fetch_list::<ObjectEdge>(
                    actor,
                    &format!("{api}/workspaces/{workspace_id}/objects/{object_id}/edges?limit=200"),
                    expected,
                    &context,
                )
                .await;
                let Some(response) = response else {
                    return;
                };
                let actual = response.iter().map(|edge| edge.id).collect::<BTreeSet<_>>();
                let expected = resolve_handles(
                    resources,
                    reference.visible_incident_edges(*object, operation.actor()),
                );
                assert_eq!(actual, expected, "object edge collection projection");
            }
            Operation::PinWorkspace { workspace, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let response = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/pin"))
                    .send()
                    .await
                    .expect("send pin-workspace request")
                    .error_for_status()
                    .expect("workspace pin succeeds")
                    .json::<PinState>()
                    .await
                    .expect("decode workspace pin");
                assert!(response.pinned);
                assert!(response.pinned_at.is_some());
            }
            Operation::UnpinWorkspace { workspace, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let response = actor
                    .browser
                    .delete(format!("{api}/workspaces/{workspace_id}/pin"))
                    .send()
                    .await
                    .expect("send unpin-workspace request")
                    .error_for_status()
                    .expect("workspace unpin succeeds")
                    .json::<PinState>()
                    .await
                    .expect("decode workspace unpin");
                assert!(!response.pinned);
                assert!(response.pinned_at.is_none());
            }
            Operation::PinObject { workspace, object, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let response = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/objects/{object_id}/pin"))
                    .send()
                    .await
                    .expect("send pin-object request")
                    .error_for_status()
                    .expect("object pin succeeds")
                    .json::<PinState>()
                    .await
                    .expect("decode object pin");
                assert!(response.pinned);
                assert!(response.pinned_at.is_some());
            }
            Operation::UnpinObject { workspace, object, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let response = actor
                    .browser
                    .delete(format!("{api}/workspaces/{workspace_id}/objects/{object_id}/pin"))
                    .send()
                    .await
                    .expect("send unpin-object request")
                    .error_for_status()
                    .expect("object unpin succeeds")
                    .json::<PinState>()
                    .await
                    .expect("decode object unpin");
                assert!(!response.pinned);
                assert!(response.pinned_at.is_none());
            }
            Operation::FavoriteObject { workspace, object, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let response = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/objects/{object_id}/favorite"))
                    .send()
                    .await
                    .expect("send favorite-object request")
                    .error_for_status()
                    .expect("object favorite succeeds")
                    .json::<FavoriteState>()
                    .await
                    .expect("decode object favorite");
                assert!(response.favorited);
            }
            Operation::UnfavoriteObject { workspace, object, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let response = actor
                    .browser
                    .delete(format!("{api}/workspaces/{workspace_id}/objects/{object_id}/favorite"))
                    .send()
                    .await
                    .expect("send unfavorite-object request")
                    .error_for_status()
                    .expect("object unfavorite succeeds")
                    .json::<FavoriteState>()
                    .await
                    .expect("decode object unfavorite");
                assert!(!response.favorited);
            }
            Operation::CreateCommentThread {
                workspace,
                object,
                thread_output,
                comment_output,
                body,
                ..
            } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let response = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/objects/{object_id}/commentary"))
                    .json(&CreateCommentRequest {
                        body: body.clone(),
                        mentioned_user_ids: Vec::new(),
                    })
                    .send()
                    .await
                    .expect("send create-comment-thread request")
                    .error_for_status()
                    .expect("comment-thread creation succeeds")
                    .json::<CommentThreadResponse>()
                    .await
                    .expect("decode created comment thread")
                    .thread;
                assert_eq!(response.workspace_id, workspace_id);
                assert_eq!(response.object_id, object_id);
                assert!(response.resolved_at.is_none());
                let root = response.comments.first().expect("created thread has root comment");
                assert_eq!(root.body.as_deref(), Some(body.as_str()));
                assert_eq!(root.status, CommentStatus::Active);
                assert!(root.parent_comment_id.is_none());
                resources.bind(*thread_output, response.id).expect("bind comment-thread handle");
                resources.bind(*comment_output, root.id).expect("bind root-comment handle");
            }
            Operation::ReplyComment { workspace, object, thread, output, body, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let thread_id = resources.resolve(*thread).expect("resolve comment-thread handle");
                let root = reference.comment_thread(*thread).expect("modeled comment thread").root;
                let root_id = resources.resolve(root).expect("resolve root-comment handle");
                let response = actor
                    .browser
                    .post(format!(
                        "{api}/workspaces/{workspace_id}/objects/{object_id}/commentary/{thread_id}/replies"
                    ))
                    .json(&CreateCommentRequest {
                        body: body.clone(),
                        mentioned_user_ids: Vec::new(),
                    })
                    .send()
                    .await
                    .expect("send comment-reply request")
                    .error_for_status()
                    .expect("comment reply succeeds")
                    .json::<CommentResponse>()
                    .await
                    .expect("decode comment reply")
                    .comment;
                assert_eq!(response.thread_id, thread_id);
                assert_eq!(response.parent_comment_id, Some(root_id));
                assert_eq!(response.body.as_deref(), Some(body.as_str()));
                assert_eq!(response.status, CommentStatus::Active);
                resources.bind(*output, response.id).expect("bind reply-comment handle");
            }
            Operation::EditComment { workspace, object, comment, body, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let comment_id = resources.resolve(*comment).expect("resolve comment handle");
                let response = actor
                    .browser
                    .patch(format!(
                        "{api}/workspaces/{workspace_id}/objects/{object_id}/commentary/comments/{comment_id}"
                    ))
                    .json(&UpdateCommentRequest {
                        body: body.clone(),
                        mentioned_user_ids: Vec::new(),
                    })
                    .send()
                    .await
                    .expect("send edit-comment request")
                    .error_for_status()
                    .expect("comment edit succeeds")
                    .json::<CommentResponse>()
                    .await
                    .expect("decode edited comment")
                    .comment;
                assert_eq!(response.id, comment_id);
                assert_eq!(response.body.as_deref(), Some(body.as_str()));
                assert_eq!(response.status, CommentStatus::Active);
                assert!(response.edited_at.is_some());
            }
            Operation::DeleteComment { workspace, object, comment, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let comment_id = resources.resolve(*comment).expect("resolve comment handle");
                let response = actor
                    .browser
                    .delete(format!(
                        "{api}/workspaces/{workspace_id}/objects/{object_id}/commentary/comments/{comment_id}"
                    ))
                    .send()
                    .await
                    .expect("send delete-comment request")
                    .error_for_status()
                    .expect("comment deletion succeeds")
                    .json::<CommentResponse>()
                    .await
                    .expect("decode deleted comment")
                    .comment;
                assert_eq!(response.id, comment_id);
                assert_eq!(response.status, CommentStatus::Deleted);
                assert!(response.body.is_none());
                assert_eq!(response.deleted_by, Some(actor.user_id));
            }
            Operation::ResolveCommentThread { workspace, object, thread, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let thread_id = resources.resolve(*thread).expect("resolve comment-thread handle");
                let path = format!(
                    "{api}/workspaces/{workspace_id}/objects/{object_id}/commentary/{thread_id}/resolve"
                );
                let response = actor
                    .browser
                    .post(&path)
                    .send()
                    .await
                    .expect("send resolve-comment-thread request")
                    .error_for_status()
                    .expect("comment-thread resolution succeeds")
                    .json::<CommentThreadResponse>()
                    .await
                    .expect("decode resolved comment thread")
                    .thread;
                assert_eq!(response.id, thread_id);
                assert!(response.resolved_at.is_some());
                assert_eq!(response.resolved_by, Some(actor.user_id));

                let repeated = actor
                    .browser
                    .post(path)
                    .send()
                    .await
                    .expect("send repeated resolve-comment-thread request");
                assert_http_outcome(repeated, ExpectedOutcome::Conflict, operation).await;
            }
            Operation::ReopenCommentThread { workspace, object, thread, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let thread_id = resources.resolve(*thread).expect("resolve comment-thread handle");
                let path = format!(
                    "{api}/workspaces/{workspace_id}/objects/{object_id}/commentary/{thread_id}/reopen"
                );
                let response = actor
                    .browser
                    .post(&path)
                    .send()
                    .await
                    .expect("send reopen-comment-thread request")
                    .error_for_status()
                    .expect("comment-thread reopen succeeds")
                    .json::<CommentThreadResponse>()
                    .await
                    .expect("decode reopened comment thread")
                    .thread;
                assert_eq!(response.id, thread_id);
                assert!(response.resolved_at.is_none());
                assert!(response.resolved_by.is_none());

                let repeated = actor
                    .browser
                    .post(path)
                    .send()
                    .await
                    .expect("send repeated reopen-comment-thread request");
                assert_http_outcome(repeated, ExpectedOutcome::Conflict, operation).await;
            }
            Operation::ListThreadComments { workspace, object, thread, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let thread_id = resources.resolve(*thread).expect("resolve comment-thread handle");
                let expected = readable_object_outcome(reference, *object, operation.actor());
                let context =
                    serde_json::to_string(operation).expect("serialize operation context");
                let response = fetch_list::<kival_sdk::Comment>(
                    actor,
                    &format!(
                        "{api}/workspaces/{workspace_id}/objects/{object_id}/commentary/{thread_id}/comments?limit=200"
                    ),
                    expected,
                    &context,
                )
                .await;
                let Some(response) = response else {
                    return;
                };
                let actual = response.iter().map(|comment| comment.id).collect::<Vec<_>>();
                let expected = reference
                    .thread_comments(*thread)
                    .into_iter()
                    .map(|comment| resources.resolve(comment).expect("resolve modeled comment"))
                    .collect::<Vec<_>>();
                assert_eq!(actual, expected, "thread comment collection projection");
                for (wire, handle) in response.iter().zip(reference.thread_comments(*thread)) {
                    let modeled = reference.comment(handle).expect("modeled comment");
                    assert_eq!(wire.author.id, fixture.actors.get(modeled.author).user_id);
                    assert_eq!(wire.body, modeled.body);
                    assert_eq!(
                        wire.status,
                        if modeled.body.is_some() {
                            CommentStatus::Active
                        } else {
                            CommentStatus::Deleted
                        }
                    );
                }
            }
            Operation::ListMentionCandidates { workspace, object, candidate, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let expected = active_object_outcome(
                    reference,
                    *object,
                    reference.can_read_object(*object, operation.actor()),
                );
                let context =
                    serde_json::to_string(operation).expect("serialize operation context");
                let candidate = fixture.actors.get(*candidate);
                let response = fetch_list::<CommentMentionCandidate>(
                    actor,
                    &format!(
                        "{api}/workspaces/{workspace_id}/objects/{object_id}/commentary/mention-candidates?q={}&limit=20",
                        fixture.identities.get(candidate.actor).username.as_str()
                    ),
                    expected,
                    &context,
                )
                .await;
                let Some(response) = response else {
                    return;
                };
                let actual =
                    response.iter().map(|candidate| candidate.user_id).collect::<BTreeSet<_>>();
                let expected = if reference.can_read_object(*object, candidate.actor) {
                    BTreeSet::from([candidate.user_id])
                } else {
                    BTreeSet::new()
                };
                assert_eq!(actual, expected, "comment mention-candidate projection");
            }
            Operation::ProbeCommentMentions {
                workspace,
                object,
                first_mention,
                second_mention,
                ..
            } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let first = fixture.actors.get(*first_mention);
                let second = fixture.actors.get(*second_mention);
                let commentary_path =
                    format!("{api}/workspaces/{workspace_id}/objects/{object_id}/commentary");

                let created = actor
                    .browser
                    .post(&commentary_path)
                    .json(&CreateCommentRequest {
                        body: format!("stateful mention {}", first.actor.username()),
                        mentioned_user_ids: vec![first.user_id],
                    })
                    .send()
                    .await
                    .expect("send create-comment request")
                    .error_for_status()
                    .expect("comment creation succeeds")
                    .json::<CommentThreadResponse>()
                    .await
                    .expect("decode created comment thread");
                let root =
                    created.thread.comments.first().expect("created thread has root comment");
                assert_comment_mentions(fixture, root, &[*first_mention]);

                let edited = actor
                    .browser
                    .patch(format!("{commentary_path}/comments/{}", root.id))
                    .json(&UpdateCommentRequest {
                        body: format!("stateful replacement mention {}", second.actor.username()),
                        mentioned_user_ids: vec![second.user_id],
                    })
                    .send()
                    .await
                    .expect("send update-comment request")
                    .error_for_status()
                    .expect("comment update succeeds")
                    .json::<CommentResponse>()
                    .await
                    .expect("decode updated comment");
                assert_comment_mentions(fixture, &edited.comment, &[*second_mention]);

                let listed = actor
                    .browser
                    .get(&commentary_path)
                    .send()
                    .await
                    .expect("send list-commentary request")
                    .error_for_status()
                    .expect("commentary list succeeds")
                    .json::<CommentThreadListResponse>()
                    .await
                    .expect("decode commentary list");
                let thread = listed
                    .items
                    .iter()
                    .find(|thread| thread.id == created.thread.id)
                    .expect("created thread appears in commentary list");
                let root = thread
                    .comments
                    .iter()
                    .find(|comment| comment.id == edited.comment.id)
                    .expect("edited root appears in commentary list");
                assert_comment_mentions(fixture, root, &[*second_mention]);
            }
            Operation::ProbeNotificationInbox { workspace, object, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                loop {
                    let (processed, _, _): (i32, i32, i64) = sqlx::query_as(
                        "SELECT * FROM kival.process_notification_candidate_batch(100)",
                    )
                    .fetch_one(pool)
                    .await
                    .expect("drain notification projection");
                    if processed < 100 {
                        break;
                    }
                }
                sqlx::query("DELETE FROM kival.inbox_notifications WHERE recipient_user_id = $1")
                    .bind(actor.user_id)
                    .execute(pool)
                    .await
                    .expect("clear recipient inbox before stateful notification probe");

                let preference_path = format!(
                    "{api}/workspaces/{workspace_id}/objects/{object_id}/notification-preference"
                );
                let current = actor
                    .browser
                    .get(&preference_path)
                    .send()
                    .await
                    .expect("send get-notification-preference request")
                    .error_for_status()
                    .expect("notification preference read succeeds")
                    .json::<ObjectNotificationPreference>()
                    .await
                    .expect("decode notification preference");
                assert_eq!(current.workspace_id, workspace_id);
                assert_eq!(current.object_id, object_id);

                let muted = actor
                    .browser
                    .patch(&preference_path)
                    .json(&UpdateObjectNotificationPreferenceRequest {
                        ordinary_notifications_enabled: false,
                    })
                    .send()
                    .await
                    .expect("send mute-notification request")
                    .error_for_status()
                    .expect("notification preference update succeeds")
                    .json::<ObjectNotificationPreference>()
                    .await
                    .expect("decode muted notification preference");
                assert!(!muted.ordinary_notifications_enabled);
                assert!(muted.explicit);

                let admin = fixture.actors.admin();
                let created = admin
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/objects/{object_id}/commentary"))
                    .json(&CreateCommentRequest {
                        body: format!(
                            "stateful directed notification for {}",
                            actor.actor.username()
                        ),
                        mentioned_user_ids: vec![actor.user_id],
                    })
                    .send()
                    .await
                    .expect("send directed-comment request")
                    .error_for_status()
                    .expect("directed comment creation succeeds")
                    .json::<CommentThreadResponse>()
                    .await
                    .expect("decode directed comment thread");
                let root = created.thread.comments.first().expect("directed thread has root");

                loop {
                    let (processed, _, _): (i32, i32, i64) = sqlx::query_as(
                        "SELECT * FROM kival.process_notification_candidate_batch(100)",
                    )
                    .fetch_one(pool)
                    .await
                    .expect("project notification candidates");
                    if processed < 100 {
                        break;
                    }
                }

                let inbox = actor
                    .browser
                    .get(format!("{api}/inbox?limit=100"))
                    .send()
                    .await
                    .expect("send inbox-list request")
                    .error_for_status()
                    .expect("inbox list succeeds")
                    .json::<ListResponse<InboxEntry>>()
                    .await
                    .expect("decode inbox list");
                let entry = inbox
                    .items
                    .iter()
                    .find(|entry| {
                        entry.reason == "mention"
                            && entry.object_id == Some(object_id)
                            && entry.comment_id == Some(root.id)
                    })
                    .expect("directed mention appears in inbox");
                assert!(entry.read_at.is_none());

                let count = actor
                    .browser
                    .get(format!("{api}/inbox/unread-count"))
                    .send()
                    .await
                    .expect("send unread-count request")
                    .error_for_status()
                    .expect("unread count succeeds")
                    .json::<InboxUnreadCountResponse>()
                    .await
                    .expect("decode unread count");
                assert!(count.unread_count >= 1);

                let read = actor
                    .browser
                    .patch(format!("{api}/inbox/{}", entry.id))
                    .json(&UpdateInboxEntryRequest { read: true })
                    .send()
                    .await
                    .expect("send inbox-entry read request")
                    .error_for_status()
                    .expect("inbox-entry read succeeds")
                    .json::<InboxEntry>()
                    .await
                    .expect("decode read inbox entry");
                assert!(read.read_at.is_some());

                let unread = actor
                    .browser
                    .patch(format!("{api}/inbox/{}", entry.id))
                    .json(&UpdateInboxEntryRequest { read: false })
                    .send()
                    .await
                    .expect("send inbox-entry unread request")
                    .error_for_status()
                    .expect("inbox-entry unread succeeds")
                    .json::<InboxEntry>()
                    .await
                    .expect("decode unread inbox entry");
                assert!(unread.read_at.is_none());

                let marked = actor
                    .browser
                    .post(format!("{api}/inbox/read"))
                    .json(&MarkInboxReadRequest {
                        workspace_id: Some(workspace_id),
                        through_sequence: None,
                    })
                    .send()
                    .await
                    .expect("send bulk inbox-read request")
                    .error_for_status()
                    .expect("bulk inbox-read succeeds")
                    .json::<InboxUpdatedResponse>()
                    .await
                    .expect("decode bulk inbox-read response");
                assert!(marked.updated >= 1);

                let count = actor
                    .browser
                    .get(format!("{api}/inbox/unread-count"))
                    .send()
                    .await
                    .expect("send final unread-count request")
                    .error_for_status()
                    .expect("final unread count succeeds")
                    .json::<InboxUnreadCountResponse>()
                    .await
                    .expect("decode final unread count");
                assert_eq!(count.unread_count, 0);

                let restored = actor
                    .browser
                    .patch(preference_path)
                    .json(&UpdateObjectNotificationPreferenceRequest {
                        ordinary_notifications_enabled: true,
                    })
                    .send()
                    .await
                    .expect("send restore-notification request")
                    .error_for_status()
                    .expect("notification preference restore succeeds")
                    .json::<ObjectNotificationPreference>()
                    .await
                    .expect("decode restored notification preference");
                assert!(restored.ordinary_notifications_enabled);
                assert!(restored.explicit);
            }
            Operation::ProbeUserDisableEnable { target, workspace, object, .. } => {
                let target_client = fixture.actors.get(*target);
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");

                let disabled = actor
                    .browser
                    .post(format!("{api}/users/{}/disable", target_client.user_id))
                    .send()
                    .await
                    .expect("send disable-user request")
                    .error_for_status()
                    .expect("user disable succeeds")
                    .json::<UserResponse>()
                    .await
                    .expect("decode disabled user");
                assert_eq!(disabled.user.id, target_client.user_id);
                assert_eq!(disabled.user.status, UserStatus::Disabled);
                assert!(disabled.user.disabled_at.is_some());
                assert_eq!(disabled.user.disabled_by, Some(actor.user_id));

                let whoami = target_client
                    .browser
                    .get(format!("{api}/auth/whoami"))
                    .send()
                    .await
                    .expect("send disabled-user whoami request");
                assert_eq!(whoami.status().as_u16(), 401, "disabled session must be rejected");

                let workspace_read = target_client
                    .browser
                    .get(format!("{api}/workspaces/{workspace_id}"))
                    .send()
                    .await
                    .expect("send disabled-user workspace request");
                assert_eq!(
                    workspace_read.status().as_u16(),
                    401,
                    "disabled user must be rejected before workspace authorization"
                );

                let enabled = actor
                    .browser
                    .post(format!("{api}/users/{}/enable", target_client.user_id))
                    .send()
                    .await
                    .expect("send enable-user request")
                    .error_for_status()
                    .expect("user enable succeeds")
                    .json::<UserResponse>()
                    .await
                    .expect("decode enabled user");
                assert_eq!(enabled.user.id, target_client.user_id);
                assert_eq!(enabled.user.status, UserStatus::Active);
                assert!(enabled.user.disabled_at.is_none());
                assert!(enabled.user.disabled_by.is_none());

                let whoami = target_client
                    .browser
                    .get(format!("{api}/auth/whoami"))
                    .send()
                    .await
                    .expect("send re-enabled whoami request")
                    .error_for_status()
                    .expect("re-enabled session becomes usable")
                    .json::<WhoamiResponse>()
                    .await
                    .expect("decode re-enabled whoami");
                assert_eq!(whoami.user.id, target_client.user_id);

                let object_read = target_client
                    .browser
                    .get(format!("{api}/workspaces/{workspace_id}/objects/{object_id}"))
                    .send()
                    .await
                    .expect("send re-enabled object request");
                assert_http_outcome(object_read, ExpectedOutcome::Success, operation).await;
            }
            Operation::ProbeAuthLifecycle { .. } => {
                let extra =
                    actor.browser.new_session().await.expect("create additional browser session");
                let extra_sessions = extra
                    .get("/auth/sessions")
                    .send()
                    .await
                    .expect("send additional-session list request")
                    .error_for_status()
                    .expect("additional-session list succeeds")
                    .json::<SessionListResponse>()
                    .await
                    .expect("decode additional-session list");
                let extra_session_id = extra_sessions
                    .items
                    .iter()
                    .find(|session| session.is_current)
                    .expect("additional browser identifies its current session")
                    .id;

                let sessions = actor
                    .browser
                    .get("/auth/sessions")
                    .send()
                    .await
                    .expect("send session-list request")
                    .error_for_status()
                    .expect("session list succeeds")
                    .json::<SessionListResponse>()
                    .await
                    .expect("decode session list");
                assert_eq!(sessions.items.iter().filter(|session| session.is_current).count(), 1);
                assert!(sessions.items.iter().any(|session| session.id == extra_session_id));

                let revoked = actor
                    .browser
                    .post(format!("{api}/auth/sessions/{extra_session_id}/revoke"))
                    .send()
                    .await
                    .expect("send session-revoke request")
                    .error_for_status()
                    .expect("session revoke succeeds")
                    .json::<SessionOnlyResponse>()
                    .await
                    .expect("decode revoked session");
                assert_eq!(revoked.session.id, extra_session_id);
                assert!(revoked.session.revoked_at.is_some());
                assert_eq!(revoked.session.revoked_by, Some(actor.user_id));

                let extra_whoami = extra
                    .get("/auth/whoami")
                    .send()
                    .await
                    .expect("send revoked additional-session request");
                assert_eq!(
                    extra_whoami.status().as_u16(),
                    401,
                    "revoked browser session must stop authenticating"
                );

                let sessions = actor
                    .browser
                    .get("/auth/sessions")
                    .send()
                    .await
                    .expect("send post-revoke session-list request")
                    .error_for_status()
                    .expect("post-revoke session list succeeds")
                    .json::<SessionListResponse>()
                    .await
                    .expect("decode post-revoke session list");
                assert!(!sessions.items.iter().any(|session| session.id == extra_session_id));

                let disposable =
                    actor.browser.new_session().await.expect("create disposable logout session");
                let logout =
                    disposable.post("/auth/logout").send().await.expect("send logout request");
                assert_eq!(logout.status().as_u16(), 204, "logout succeeds");
                let logged_out = disposable
                    .get("/auth/whoami")
                    .send()
                    .await
                    .expect("send logged-out whoami request");
                assert_eq!(
                    logged_out.status().as_u16(),
                    401,
                    "logged-out browser session must stop authenticating"
                );

                let passkeys = actor
                    .browser
                    .get("/auth/passkeys")
                    .send()
                    .await
                    .expect("send passkey-list request")
                    .error_for_status()
                    .expect("passkey list succeeds")
                    .json::<serde_json::Value>()
                    .await
                    .expect("decode passkey list");
                let items = passkeys["items"].as_array().expect("passkey items array");
                assert_eq!(items.len(), 1, "fixture actor must retain exactly one passkey");
                let passkey_id = items[0]["id"].as_str().expect("passkey ID string");

                actor.browser.fresh_authenticate().await.expect("fresh-authenticate passkey owner");
                let revoke_last = actor
                    .browser
                    .post(format!("{api}/auth/passkeys/{passkey_id}/revoke"))
                    .send()
                    .await
                    .expect("send last-passkey revoke request");
                assert_eq!(
                    revoke_last.status().as_u16(),
                    409,
                    "last passkey revocation must be rejected"
                );

                let passkeys = actor
                    .browser
                    .get("/auth/passkeys")
                    .send()
                    .await
                    .expect("send post-conflict passkey-list request")
                    .error_for_status()
                    .expect("post-conflict passkey list succeeds")
                    .json::<serde_json::Value>()
                    .await
                    .expect("decode post-conflict passkey list");
                assert_eq!(passkeys["items"].as_array().expect("passkey items array").len(), 1);
            }
            Operation::ProbeUnauthorizedGroupMutations { group, member, .. } => {
                let group_id = resources.resolve(*group).expect("resolve group handle");
                let member = fixture.actors.get(*member);

                let update = actor
                    .browser
                    .patch(format!("{api}/groups/{group_id}"))
                    .json(&UpdateGroupRequest {
                        name: Some(format!("{namespace}-rejected-group-update")),
                        description: Default::default(),
                    })
                    .send()
                    .await
                    .expect("send unauthorized group update");
                assert_http_outcome(update, ExpectedOutcome::Forbidden, operation).await;

                let archive = actor
                    .browser
                    .post(format!("{api}/groups/{group_id}/archive"))
                    .send()
                    .await
                    .expect("send unauthorized group archive");
                assert_http_outcome(archive, ExpectedOutcome::Forbidden, operation).await;

                let membership = actor
                    .browser
                    .post(format!("{api}/groups/{group_id}/memberships"))
                    .json(&CreateGroupMembershipRequest {
                        user_id: Some(member.user_id),
                        username: None,
                        group_role: kival_sdk::MembershipRole::Member,
                    })
                    .send()
                    .await
                    .expect("send unauthorized group membership creation");
                assert_http_outcome(membership, ExpectedOutcome::Forbidden, operation).await;

                let create = actor
                    .browser
                    .post(format!("{api}/groups"))
                    .json(&CreateGroupRequest {
                        name: format!("{namespace}-rejected-group-create"),
                        description: None,
                    })
                    .send()
                    .await
                    .expect("send unauthorized group creation");
                assert_http_outcome(create, ExpectedOutcome::Forbidden, operation).await;
            }
            Operation::ProbeUnauthorizedWorkspaceMutations { workspace, member, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let member = fixture.actors.get(*member);

                let update = actor
                    .browser
                    .patch(format!("{api}/workspaces/{workspace_id}"))
                    .json(&UpdateWorkspaceRequest {
                        name: Some(format!("{namespace}-rejected-workspace-update")),
                        description: Default::default(),
                    })
                    .send()
                    .await
                    .expect("send unauthorized workspace update");
                assert_http_outcome(update, ExpectedOutcome::Forbidden, operation).await;

                let archive = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/archive"))
                    .send()
                    .await
                    .expect("send unauthorized workspace archive");
                assert_http_outcome(archive, ExpectedOutcome::Forbidden, operation).await;

                let membership = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/memberships"))
                    .json(&CreateWorkspaceMembershipRequest {
                        user_id: Some(member.user_id),
                        username: None,
                        workspace_role: kival_sdk::MembershipRole::Member,
                    })
                    .send()
                    .await
                    .expect("send unauthorized workspace membership creation");
                assert_http_outcome(membership, ExpectedOutcome::Forbidden, operation).await;
            }
            Operation::ProbeUnauthorizedObjectMutations { workspace, object, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");

                let update = actor
                    .browser
                    .patch(format!("{api}/workspaces/{workspace_id}/objects/{object_id}"))
                    .json(&UpdateObjectRequest {
                        expected_current_version_id: uuid::Uuid::nil(),
                        title: Some("rejected unauthorized object update".to_owned()),
                        body: None,
                        metadata: None,
                    })
                    .send()
                    .await
                    .expect("send unauthorized object update");
                assert_http_outcome(update, ExpectedOutcome::Forbidden, operation).await;

                let archive = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/objects/{object_id}/archive"))
                    .send()
                    .await
                    .expect("send unauthorized object archive");
                assert_http_outcome(archive, ExpectedOutcome::Forbidden, operation).await;

                let attachment = actor
                    .browser
                    .post(format!(
                        "{api}/workspaces/{workspace_id}/objects/{object_id}/attachments/upload?name=unauthorized-probe.txt&media_type=text%2Fplain&metadata=%7B%7D"
                    ))
                    .body("rejected unauthorized attachment")
                    .send()
                    .await
                    .expect("send unauthorized attachment upload");
                assert_http_outcome(attachment, ExpectedOutcome::Forbidden, operation).await;
            }
            Operation::ProbeUnauthorizedObjectCreate { workspace, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let response = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/objects"))
                    .json(&CreateObjectRequest {
                        title: "rejected unauthorized object creation".to_owned(),
                        body: "rejected unauthorized object creation".to_owned(),
                        metadata: serde_json::json!({ "stateful": true }),
                    })
                    .send()
                    .await
                    .expect("send unauthorized object creation");
                assert_http_outcome(response, ExpectedOutcome::Forbidden, operation).await;
            }
            Operation::ProbeWikilinkReresolution { workspace, source, target, suffix, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let source_id = resources.resolve(*source).expect("resolve source-object handle");
                let target_id = resources.resolve(*target).expect("resolve target-object handle");
                let source_title = reference.object_title(*source).expect("modeled source title");
                let target_title = reference.object_title(*target).expect("modeled target title");
                let temporary_target_title =
                    format!("stateful-wikilink-target-{}-{suffix}", target.index);

                update_object_content(
                    actor,
                    &api,
                    workspace_id,
                    source_id,
                    source_title,
                    &format!("[[{target_title}]]"),
                )
                .await;
                assert_textual_backlink(
                    actor,
                    &api,
                    workspace_id,
                    target_id,
                    source_id,
                    Some(target_title),
                )
                .await;

                update_object_content(
                    actor,
                    &api,
                    workspace_id,
                    target_id,
                    &temporary_target_title,
                    &object_body(&temporary_target_title),
                )
                .await;
                assert_textual_backlink(actor, &api, workspace_id, target_id, source_id, None)
                    .await;

                update_object_content(
                    actor,
                    &api,
                    workspace_id,
                    source_id,
                    source_title,
                    &format!("[[{temporary_target_title}]]"),
                )
                .await;
                assert_textual_backlink(
                    actor,
                    &api,
                    workspace_id,
                    target_id,
                    source_id,
                    Some(&temporary_target_title),
                )
                .await;

                update_object_content(
                    actor,
                    &api,
                    workspace_id,
                    source_id,
                    source_title,
                    &object_body(source_title),
                )
                .await;
                assert_textual_backlink(actor, &api, workspace_id, target_id, source_id, None)
                    .await;

                update_object_content(
                    actor,
                    &api,
                    workspace_id,
                    target_id,
                    target_title,
                    &object_body(target_title),
                )
                .await;
            }
            Operation::ProbeArchivedWorkspaceObjectWrites {
                workspace, object, principal, ..
            } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");

                let update = actor
                    .browser
                    .patch(format!("{api}/workspaces/{workspace_id}/objects/{object_id}"))
                    .json(&UpdateObjectRequest {
                        expected_current_version_id: uuid::Uuid::nil(),
                        title: Some("rejected archived-workspace update".to_owned()),
                        body: None,
                        metadata: None,
                    })
                    .send()
                    .await
                    .expect("send archived-workspace object update");
                assert_http_outcome(update, ExpectedOutcome::NotFound, operation).await;

                let archive = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/objects/{object_id}/archive"))
                    .send()
                    .await
                    .expect("send archived-workspace object archive");
                assert_http_outcome(archive, ExpectedOutcome::NotFound, operation).await;

                let comment = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/objects/{object_id}/commentary"))
                    .json(&CreateCommentRequest {
                        body: "rejected archived-workspace comment".to_owned(),
                        mentioned_user_ids: Vec::new(),
                    })
                    .send()
                    .await
                    .expect("send archived-workspace comment creation");
                assert_http_outcome(comment, ExpectedOutcome::NotFound, operation).await;

                let attachment = actor
                    .browser
                    .post(format!(
                        "{api}/workspaces/{workspace_id}/objects/{object_id}/attachments/upload\
                         ?name=archived-probe.txt&media_type=text%2Fplain&metadata=%7B%7D"
                    ))
                    .body("rejected archived-workspace attachment")
                    .send()
                    .await
                    .expect("send archived-workspace attachment upload");
                assert_http_outcome(attachment, ExpectedOutcome::NotFound, operation).await;

                let principal = fixture.actors.get(*principal);
                let grant = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/objects/{object_id}/grants"))
                    .json(&CreateObjectGrantRequest {
                        principal: GrantPrincipal::User(principal.user_id),
                        object_role: ObjectRole::Viewer,
                    })
                    .send()
                    .await
                    .expect("send archived-workspace object grant");
                assert_http_outcome(grant, ExpectedOutcome::NotFound, operation).await;
            }
            Operation::ProbeArchivedWorkspaceObjectRestore { workspace, object, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let response = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/objects/{object_id}/unarchive"))
                    .send()
                    .await
                    .expect("send archived-workspace object restore");
                assert_http_outcome(response, ExpectedOutcome::NotFound, operation).await;
            }
            Operation::UploadObjectAttachment {
                workspace, object, output, name, content, ..
            } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let display_name = format!("{namespace}-{name}");
                let response = actor
                    .browser
                    .post(format!(
                        "{api}/workspaces/{workspace_id}/objects/{object_id}/attachments/upload\
                         ?name={display_name}&media_type=text%2Fplain\
                         &metadata=%7B%22stateful%22%3Atrue%7D"
                    ))
                    .body(content.clone())
                    .send()
                    .await
                    .expect("send attachment-upload request")
                    .error_for_status()
                    .expect("attachment upload succeeds")
                    .json::<ObjectAttachmentResponse>()
                    .await
                    .expect("decode uploaded attachment")
                    .attachment;
                assert_eq!(response.workspace_id, workspace_id);
                assert_eq!(response.object_id, object_id);
                assert_eq!(response.version_id, None);
                assert_eq!(response.source_attachment_id, None);
                assert_eq!(response.name.as_deref(), Some(display_name.as_str()));
                assert_eq!(response.media_type.as_deref(), Some("text/plain"));
                assert_eq!(response.metadata, serde_json::json!({ "stateful": true }));
                assert_eq!(response.created_by, Some(actor.user_id));
                resources.bind(*output, response.id).expect("bind attachment handle");
            }
            Operation::ListObjectAttachments { workspace, object, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let expected = readable_object_outcome(reference, *object, operation.actor());
                let context =
                    serde_json::to_string(operation).expect("serialize operation context");
                let response = fetch_list::<ObjectAttachment>(
                    actor,
                    &format!(
                        "{api}/workspaces/{workspace_id}/objects/{object_id}/attachments?limit=200"
                    ),
                    expected,
                    &context,
                )
                .await;
                let Some(response) = response else {
                    return;
                };
                let expected_handles = reference.object_attachments(*object);
                let expected = resolve_handles(resources, expected_handles.clone());
                let actual =
                    response.iter().map(|attachment| attachment.id).collect::<BTreeSet<_>>();
                assert_eq!(actual, expected, "object attachment projection");
                for handle in expected_handles {
                    let attachment_id =
                        resources.resolve(handle).expect("resolve listed attachment");
                    let attachment = response
                        .iter()
                        .find(|attachment| attachment.id == attachment_id)
                        .expect("modeled attachment appears in list");
                    assert_attachment(
                        resources,
                        namespace,
                        reference,
                        handle,
                        workspace_id,
                        object_id,
                        attachment,
                    );
                }
            }
            Operation::GetObjectAttachment { workspace, object, attachment, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let attachment_id =
                    resources.resolve(*attachment).expect("resolve attachment handle");
                let response = actor
                    .browser
                    .get(format!(
                        "{api}/workspaces/{workspace_id}/objects/{object_id}/attachments/{attachment_id}"
                    ))
                    .send()
                    .await
                    .expect("send get-object-attachment request");
                let expected = readable_object_outcome(reference, *object, operation.actor());
                let response = assert_http_outcome(response, expected, operation).await;
                if expected != ExpectedOutcome::Success {
                    return;
                }
                let response = response
                    .json::<ObjectAttachmentResponse>("decode object attachment")
                    .attachment;
                assert_attachment(
                    resources,
                    namespace,
                    reference,
                    *attachment,
                    workspace_id,
                    object_id,
                    &response,
                );
            }
            Operation::GetObjectAttachmentContent { workspace, object, attachment, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let attachment_id =
                    resources.resolve(*attachment).expect("resolve attachment handle");
                let response = actor
                    .browser
                    .get(format!(
                        "{api}/workspaces/{workspace_id}/objects/{object_id}/attachments/{attachment_id}/content"
                    ))
                    .send()
                    .await
                    .expect("send get-attachment-content request");
                let expected = readable_object_outcome(reference, *object, operation.actor());
                let response = assert_http_outcome(response, expected, operation).await;
                if expected != ExpectedOutcome::Success {
                    return;
                }
                assert_eq!(response.content_type(), Some("text/plain"));
                let expected = reference.attachment(*attachment).expect("modeled attachment");
                assert_eq!(response.body(), expected.content.as_slice());
            }
            Operation::ReuseObjectAttachment { workspace, object, source, output, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let source_id = resources.resolve(*source).expect("resolve source attachment");
                let response = actor
                    .browser
                    .post(format!(
                        "{api}/workspaces/{workspace_id}/objects/{object_id}/attachments/reuse"
                    ))
                    .json(&ReuseObjectAttachmentRequest {
                        source_attachment_id: source_id,
                        version_id: None,
                    })
                    .send()
                    .await
                    .expect("send reuse-object-attachment request")
                    .error_for_status()
                    .expect("attachment reuse succeeds")
                    .json::<ObjectAttachmentResponse>()
                    .await
                    .expect("decode reused attachment")
                    .attachment;
                let source_attachment =
                    reference.attachment(*source).expect("modeled source attachment");
                let expected_name = format!("{namespace}-{}", source_attachment.name);
                assert_eq!(response.workspace_id, workspace_id);
                assert_eq!(response.object_id, object_id);
                assert_eq!(response.version_id, None);
                assert_eq!(response.source_attachment_id, Some(source_id));
                assert_eq!(response.name.as_deref(), Some(expected_name.as_str()));
                assert_eq!(response.media_type.as_deref(), Some("text/plain"));
                assert_eq!(response.metadata, serde_json::json!({ "stateful": true }));
                assert_eq!(response.created_by, Some(actor.user_id));
                resources.bind(*output, response.id).expect("bind reused attachment handle");
            }
            Operation::UpdateObject { workspace, object, title, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let expected = active_object_outcome(
                    reference,
                    *object,
                    reference.can_edit_object(*object, operation.actor()),
                );
                let expected_current_version_id = if expected == ExpectedOutcome::Success {
                    actor
                        .browser
                        .get(format!("{api}/workspaces/{workspace_id}/objects/{object_id}"))
                        .send()
                        .await
                        .expect("send update-object preflight read")
                        .error_for_status()
                        .expect("update-object preflight read succeeds")
                        .json::<ObjectResponse>()
                        .await
                        .expect("decode update-object preflight read")
                        .current_version
                        .expect("active modeled object has current version")
                        .id
                } else {
                    uuid::Uuid::nil()
                };
                let response = actor
                    .browser
                    .patch(format!("{api}/workspaces/{workspace_id}/objects/{object_id}"))
                    .json(&UpdateObjectRequest {
                        expected_current_version_id,
                        title: Some(title.clone()),
                        body: Some(object_body(title)),
                        metadata: Some(serde_json::json!({
                            "stateful": true,
                            "version": reference
                                .object_version(*object)
                                .expect("modeled object version"),
                        })),
                    })
                    .send()
                    .await
                    .expect("send update-object request");
                let response = assert_http_outcome(response, expected, operation).await;
                if expected != ExpectedOutcome::Success {
                    let unchanged = fixture
                        .actors
                        .admin()
                        .browser
                        .get(format!("{api}/workspaces/{workspace_id}/objects/{object_id}"))
                        .send()
                        .await
                        .expect("send failed-update verification request");
                    let unchanged =
                        assert_http_outcome(unchanged, ExpectedOutcome::Success, operation).await;
                    let unchanged =
                        unchanged.json::<ObjectResponse>("decode failed-update verification");
                    assert_eq!(
                        unchanged.object.title,
                        reference.object_title(*object).expect("modeled unchanged object title")
                    );
                    assert_eq!(
                        unchanged
                            .current_version
                            .expect("unchanged object has current version")
                            .version_number,
                        reference.object_version(*object).expect("modeled unchanged version")
                    );
                    return;
                }
                let response = response.json::<ObjectResponse>("decode update-object response");
                assert_eq!(response.object.id, object_id);
                assert_eq!(response.object.title, *title);
                assert_eq!(
                    response.effective_role,
                    reference.object_role(*object, operation.actor()).expect("modeled object role")
                );
                let version = response.current_version.expect("updated object has a version");
                assert_eq!(
                    version.version_number,
                    reference.object_version(*object).expect("modeled object version")
                );
                assert_eq!(version.title, *title);
                assert_eq!(version.body, object_body(title));
                assert_object_listed(actor, &api, workspace_id, object_id, *object, reference)
                    .await;
                assert_version_listed(
                    actor,
                    &api,
                    workspace_id,
                    object_id,
                    version.id,
                    version.version_number,
                )
                .await;
            }
            Operation::ArchiveObject { workspace, object, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let response = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/objects/{object_id}/archive"))
                    .send()
                    .await
                    .expect("send archive-object request")
                    .error_for_status()
                    .expect("object archive succeeds")
                    .json::<ObjectResponse>()
                    .await
                    .expect("decode archive-object response");
                assert_eq!(response.object.id, object_id);
                assert_eq!(response.object.status, ArchiveStatus::Archived);
                assert_eq!(response.object.archived_by, Some(actor.user_id));
                assert!(response.object.archived_at.is_some());
                resources.archive(*object).expect("archive object handle");
                assert_object_listed(actor, &api, workspace_id, object_id, *object, reference)
                    .await;
            }
            Operation::UnarchiveObject { workspace, object, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let object_id = resources.resolve(*object).expect("resolve object handle");
                let response = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/objects/{object_id}/unarchive"))
                    .send()
                    .await
                    .expect("send unarchive-object request")
                    .error_for_status()
                    .expect("object restore succeeds")
                    .json::<ObjectResponse>()
                    .await
                    .expect("decode unarchive-object response");
                assert_eq!(response.object.id, object_id);
                assert_eq!(response.object.status, ArchiveStatus::Active);
                assert!(response.object.archived_by.is_none());
                assert!(response.object.archived_at.is_none());
                resources.unarchive(*object).expect("unarchive object handle");
                assert_object_listed(actor, &api, workspace_id, object_id, *object, reference)
                    .await;
            }
            Operation::CreateObjectEdge { workspace, source, target, output, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let source_id = resources.resolve(*source).expect("resolve source-object handle");
                let target_id = resources.resolve(*target).expect("resolve target-object handle");
                let response = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/edges"))
                    .json(&CreateObjectEdgeRequest {
                        source_object_id: source_id,
                        target_object_id: target_id,
                    })
                    .send()
                    .await
                    .expect("send create-object-edge request")
                    .error_for_status()
                    .expect("object edge creation succeeds")
                    .json::<ObjectEdgeResponse>()
                    .await
                    .expect("decode object-edge response")
                    .edge;
                assert_eq!(response.workspace_id, workspace_id);
                assert_eq!(response.source_object_id, source_id);
                assert_eq!(response.target_object_id, target_id);
                resources.bind(*output, response.id).expect("bind edge handle");
                assert_edge_listed(actor, &api, workspace_id, source_id, response.id, true).await;
            }
            Operation::GetObjectEdge { workspace, edge, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let edge_id = resources.resolve(*edge).expect("resolve edge handle");
                let response = actor
                    .browser
                    .get(format!("{api}/workspaces/{workspace_id}/edges/{edge_id}"))
                    .send()
                    .await
                    .expect("send get-object-edge request");
                let (_, source, target, _) = reference.edge(*edge).expect("modeled edge");
                let source_active = reference
                    .object(source)
                    .is_some_and(|(_, lifecycle)| lifecycle == Lifecycle::Active);
                let target_active = reference
                    .object(target)
                    .is_some_and(|(_, lifecycle)| lifecycle == Lifecycle::Active);
                let workspace_archived =
                    reference.workspace(*workspace) == Some(Lifecycle::Archived);
                let expected = if workspace_archived || !source_active {
                    ExpectedOutcome::NotFound
                } else if !reference.can_read_object(source, operation.actor()) {
                    ExpectedOutcome::Forbidden
                } else if !target_active {
                    ExpectedOutcome::NotFound
                } else if !reference.can_read_object(target, operation.actor()) {
                    ExpectedOutcome::Forbidden
                } else {
                    ExpectedOutcome::Success
                };
                let response = assert_http_outcome(response, expected, operation).await;
                if expected == ExpectedOutcome::Success {
                    let response =
                        response.json::<ObjectEdgeResponse>("decode get-object-edge response").edge;
                    assert_eq!(response.id, edge_id);
                }
            }
            Operation::RevokeObjectEdge { workspace, edge, .. } => {
                let workspace_id = resources.resolve(*workspace).expect("resolve workspace handle");
                let edge_id = resources.resolve(*edge).expect("resolve edge handle");
                let (_, source, _, active) = reference.edge(*edge).expect("modeled edge");
                assert!(!active);
                let source_id = resources.resolve(source).expect("resolve edge source handle");
                let response = actor
                    .browser
                    .post(format!("{api}/workspaces/{workspace_id}/edges/{edge_id}/revoke"))
                    .send()
                    .await
                    .expect("send revoke-object-edge request")
                    .error_for_status()
                    .expect("object edge revocation succeeds")
                    .json::<ObjectEdgeResponse>()
                    .await
                    .expect("decode revoked object-edge response")
                    .edge;
                assert_eq!(response.id, edge_id);
                assert!(response.revoked_at.is_some());
                resources.archive(*edge).expect("retire edge handle");
                assert_edge_listed(actor, &api, workspace_id, source_id, edge_id, false).await;
            }
        }
    }

    /// Reconciles all modeled resource visibility and effective object access.
    async fn audit_final_model(
        fixture: &Fixture,
        resources: &ResourceMap,
        namespace: &str,
        event_baseline: i64,
        reference: &Model,
    ) {
        let api = format!("{}/api/v1", fixture.base_url);
        let context = "final model-to-server audit";

        audit_workspace_collections(fixture, resources, &api, reference, context).await;
        audit_group_collections(fixture, resources, &api, reference, context).await;
        audit_events(fixture, resources, &api, reference, event_baseline, context).await;

        for workspace in reference.visible_workspaces(Actor::Admin) {
            audit_workspace_visibility(
                fixture, resources, &api, namespace, reference, workspace, context,
            )
            .await;
            assert_workspace_projections(fixture, resources, &api, reference, workspace, context)
                .await;
        }
        for group in reference.visible_groups(Actor::Admin) {
            audit_group_visibility(fixture, resources, &api, namespace, reference, group, context)
                .await;
        }
        for (object, _) in reference.objects() {
            audit_object_access(fixture, resources, &api, reference, object, context).await;
        }
    }

    /// Reconciles the stable projection of all events emitted during the case.
    async fn audit_events(
        fixture: &Fixture,
        resources: &ResourceMap,
        api: &str,
        reference: &Model,
        event_baseline: i64,
        context: &str,
    ) {
        let expected = reference
            .events()
            .iter()
            .map(|event| EventProjection {
                kind: event.kind.clone(),
                actor_user_id: Some(fixture.actors.get(event.actor).user_id),
                workspace_id: event.workspace.map(|workspace| {
                    resources.resolve(workspace).expect("resolve event workspace")
                }),
                object_id: event
                    .object
                    .map(|object| resources.resolve(object).expect("resolve event object")),
                object_edge_id: event
                    .object_edge
                    .map(|edge| resources.resolve(edge).expect("resolve event object edge")),
                object_grant_id: event
                    .object_grant
                    .map(|grant| resources.resolve(grant).expect("resolve event object grant")),
                group_id: event
                    .group
                    .map(|group| resources.resolve(group).expect("resolve event group")),
                target_user_id: event.target_user.map(|user| fixture.actors.get(user).user_id),
            })
            .collect::<Vec<_>>();

        let mut actual = Vec::new();
        let mut after_sequence = event_baseline;
        loop {
            let response = fixture
                .actors
                .admin()
                .browser
                .get(format!("{api}/events?order=asc&limit=200&after_sequence={after_sequence}"))
                .send()
                .await
                .expect("send final event-audit request");
            let response =
                assert_http_outcome_with_context(response, ExpectedOutcome::Success, context).await;
            let page = response.json::<ListResponse<Event>>("decode final event-audit response");
            if page.items.is_empty() {
                break;
            }
            assert!(
                page.items
                    .windows(2)
                    .all(|events| events[0].sequence_number < events[1].sequence_number),
                "{context}: event sequence order"
            );
            assert!(
                page.items[0].sequence_number > after_sequence,
                "{context}: event pagination made no progress"
            );
            after_sequence = page.items.last().expect("non-empty event page").sequence_number;
            actual.extend(page.items.into_iter().filter_map(|event| {
                STABLE_EVENT_KINDS.contains(&event.event_kind.as_str()).then_some(EventProjection {
                    kind: event.event_kind,
                    actor_user_id: event.actor_user_id,
                    workspace_id: event.workspace_id,
                    object_id: event.object_id,
                    object_edge_id: event.object_edge_id,
                    object_grant_id: event.object_grant_id,
                    group_id: event.group_id,
                    target_user_id: event.target_user_id,
                })
            }));
        }

        assert_eq!(actual, expected, "{context}: stable event projection");
    }

    /// Reconciles workspace collection visibility for every actor.
    async fn audit_workspace_collections(
        fixture: &Fixture,
        resources: &ResourceMap,
        api: &str,
        reference: &Model,
        context: &str,
    ) {
        for principal in Actor::ALL.into_iter().filter(|actor| *actor != Actor::Admin) {
            let items = fetch_list::<Workspace>(
                fixture.actors.get(principal),
                &format!("{api}/workspaces?status=all&limit=100"),
                ExpectedOutcome::Success,
                context,
            )
            .await
            .expect("workspace collection is always readable");
            let actual = items.into_iter().map(|workspace| workspace.id).collect::<BTreeSet<_>>();
            let expected = resolve_handles(resources, reference.visible_workspaces(principal));
            assert_eq!(actual, expected, "{context}: workspace collection for {principal:?}");
        }
    }

    /// Reconciles group collection visibility for every actor.
    async fn audit_group_collections(
        fixture: &Fixture,
        resources: &ResourceMap,
        api: &str,
        reference: &Model,
        context: &str,
    ) {
        for principal in Actor::ALL.into_iter().filter(|actor| *actor != Actor::Admin) {
            let items = fetch_list::<Group>(
                fixture.actors.get(principal),
                &format!("{api}/groups?status=all&limit=100"),
                ExpectedOutcome::Success,
                context,
            )
            .await
            .expect("group collection is always readable");
            let actual = items.into_iter().map(|group| group.id).collect::<BTreeSet<_>>();
            let expected = resolve_handles(resources, reference.visible_groups(principal));
            assert_eq!(actual, expected, "{context}: group collection for {principal:?}");
        }
    }

    /// Reconciles direct workspace visibility for every actor.
    async fn audit_workspace_visibility(
        fixture: &Fixture,
        resources: &ResourceMap,
        api: &str,
        namespace: &str,
        reference: &Model,
        workspace: kival_tests::Handle,
        context: &str,
    ) {
        let workspace_id = resources.resolve(workspace).expect("resolve audited workspace");
        for principal in Actor::ALL {
            let response = fixture
                .actors
                .get(principal)
                .browser
                .get(format!("{api}/workspaces/{workspace_id}"))
                .send()
                .await
                .expect("send final workspace-visibility request");
            let expected = if reference.can_read_workspace(workspace, principal) {
                ExpectedOutcome::Success
            } else {
                ExpectedOutcome::Forbidden
            };
            let response = assert_http_outcome_with_context(response, expected, context).await;
            if expected == ExpectedOutcome::Success {
                let actual = response
                    .json::<WorkspaceResponse>("decode final workspace visibility")
                    .workspace;
                assert_eq!(actual.id, workspace_id, "{context}: workspace ID");
                assert_eq!(
                    actual.name,
                    format!(
                        "{namespace}-{}",
                        reference.workspace_name(workspace).expect("modeled workspace name")
                    ),
                    "{context}: workspace name"
                );
                assert_eq!(
                    actual.status,
                    expected_status(reference, workspace),
                    "{context}: workspace lifecycle"
                );
            }
        }
    }

    /// Reconciles direct group visibility for every actor.
    async fn audit_group_visibility(
        fixture: &Fixture,
        resources: &ResourceMap,
        api: &str,
        namespace: &str,
        reference: &Model,
        group: kival_tests::Handle,
        context: &str,
    ) {
        let group_id = resources.resolve(group).expect("resolve audited group");
        for principal in Actor::ALL {
            let response = fixture
                .actors
                .get(principal)
                .browser
                .get(format!("{api}/groups/{group_id}"))
                .send()
                .await
                .expect("send final group-visibility request");
            let expected = if reference.can_read_group(group, principal) {
                ExpectedOutcome::Success
            } else {
                ExpectedOutcome::Forbidden
            };
            let response = assert_http_outcome_with_context(response, expected, context).await;
            if expected == ExpectedOutcome::Success {
                let actual = response.json::<GroupResponse>("decode final group visibility").group;
                assert_eq!(actual.id, group_id, "{context}: group ID");
                assert_eq!(
                    actual.name,
                    format!(
                        "{namespace}-{}",
                        reference.group_name(group).expect("modeled group name")
                    ),
                    "{context}: group name"
                );
                assert_eq!(
                    actual.status,
                    expected_group_status(reference, group),
                    "{context}: group lifecycle"
                );
            }
        }
    }

    /// Reconciles one object's effective access for every actor.
    async fn audit_object_access(
        fixture: &Fixture,
        resources: &ResourceMap,
        api: &str,
        reference: &Model,
        object: kival_tests::Handle,
        context: &str,
    ) {
        let (workspace, _) = reference.object(object).expect("modeled audited object");
        let workspace_id = resources.resolve(workspace).expect("resolve audited object workspace");
        let object_id = resources.resolve(object).expect("resolve audited object");
        for principal in Actor::ALL {
            assert_actor_object_access_with_context(
                fixture.actors.get(principal),
                api,
                workspace_id,
                object_id,
                reference.object_role(object, principal),
                readable_object_outcome(reference, object, principal),
                context,
            )
            .await;
        }
    }

    /// Fetches every page from a list endpoint after validating its first outcome.
    async fn fetch_list<T: serde::de::DeserializeOwned>(
        actor: &ActorClient,
        url: &str,
        expected: ExpectedOutcome,
        context: &str,
    ) -> Option<Vec<T>> {
        let mut items = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page_url = cursor
                .as_ref()
                .map_or_else(|| url.to_owned(), |cursor| format!("{url}&cursor={cursor}"));
            let response =
                actor.browser.get(page_url).send().await.expect("send modeled list request");
            let page_expected = if cursor.is_none() { expected } else { ExpectedOutcome::Success };
            let response = assert_http_outcome_with_context(response, page_expected, context).await;
            if page_expected != ExpectedOutcome::Success {
                return None;
            }
            let page = response.json::<ListResponse<T>>("decode modeled list response");
            items.extend(page.items);
            let Some(next_cursor) = page.next_cursor else {
                return Some(items);
            };
            cursor = Some(next_cursor);
        }
    }

    /// Resolves symbolic handles into a comparable set of concrete IDs.
    fn resolve_handles(
        resources: &ResourceMap,
        handles: impl IntoIterator<Item = kival_tests::Handle>,
    ) -> BTreeSet<uuid::Uuid> {
        handles
            .into_iter()
            .map(|handle| resources.resolve(handle).expect("resolve projected handle"))
            .collect()
    }

    /// Returns the visible depth-one neighborhood around a graph root.
    fn immediate_graph_nodes(
        reference: &Model,
        root: kival_tests::Handle,
        edges: &[kival_tests::Handle],
    ) -> BTreeSet<kival_tests::Handle> {
        let mut nodes = BTreeSet::from([root]);
        for edge in edges {
            let (_, source, target, _) = reference.edge(*edge).expect("modeled graph edge");
            if source == root || target == root {
                nodes.insert(source);
                nodes.insert(target);
            }
        }
        nodes
    }

    /// Checks all modeled object access affected by a group principal.
    async fn assert_group_principal_access(
        fixture: &Fixture,
        resources: &ResourceMap,
        api: &str,
        reference: &Model,
        group: kival_tests::Handle,
        operation: &Operation,
    ) {
        let context = serde_json::to_string(operation).expect("serialize operation context");
        let mut affected_workspaces = BTreeSet::new();
        for (object, workspace) in reference.group_granted_objects(group) {
            affected_workspaces.insert(workspace);
            let object_id = resources.resolve(object).expect("resolve group-granted object");
            let workspace_id =
                resources.resolve(workspace).expect("resolve group-granted workspace");
            for principal in Actor::ALL {
                assert_actor_object_access_with_context(
                    fixture.actors.get(principal),
                    api,
                    workspace_id,
                    object_id,
                    reference.object_role(object, principal),
                    readable_object_outcome(reference, object, principal),
                    &context,
                )
                .await;
                assert_search_visibility(
                    fixture.actors.get(principal),
                    resources,
                    api,
                    reference,
                    workspace,
                    object,
                    &context,
                )
                .await;
            }
        }
        for workspace in affected_workspaces {
            assert_workspace_projections(fixture, resources, api, reference, workspace, &context)
                .await;
        }
    }

    /// Checks direct visibility of one modeled group for every fixture actor.
    async fn assert_group_visibility(
        fixture: &Fixture,
        resources: &ResourceMap,
        api: &str,
        namespace: &str,
        reference: &Model,
        group: kival_tests::Handle,
        operation: &Operation,
    ) {
        let context = serde_json::to_string(operation).expect("serialize operation context");
        audit_group_visibility(fixture, resources, api, namespace, reference, group, &context)
            .await;
    }

    /// Reconciles object-list and workspace-graph projections for every actor.
    async fn assert_workspace_projections(
        fixture: &Fixture,
        resources: &ResourceMap,
        api: &str,
        reference: &Model,
        workspace: kival_tests::Handle,
        context: &str,
    ) {
        let workspace_id = resources.resolve(workspace).expect("resolve audited workspace");
        for principal in Actor::ALL {
            let actor = fixture.actors.get(principal);
            let expected = active_workspace_outcome(
                reference,
                workspace,
                reference.can_use_workspace(workspace, principal),
            );
            let response = fetch_list::<ObjectResource>(
                actor,
                &format!("{api}/workspaces/{workspace_id}/objects?status=all&limit=100"),
                expected,
                context,
            )
            .await;
            let Some(response) = response else {
                continue;
            };
            let actual = response.iter().map(|object| object.id).collect::<BTreeSet<_>>();
            let expected_objects =
                reference.objects().into_iter().filter_map(|(object, object_workspace)| {
                    (object_workspace == workspace && reference.can_read_object(object, principal))
                        .then_some(object)
                });
            let expected_objects = resolve_handles(resources, expected_objects);
            assert_eq!(actual, expected_objects, "{context}: modeled object-list visibility");

            let response = actor
                .browser
                .get(format!(
                    "{api}/workspaces/{workspace_id}/graph?limit_nodes=1000&limit_edges=3000"
                ))
                .send()
                .await
                .expect("send modeled workspace-graph request");
            let response =
                assert_http_outcome_with_context(response, ExpectedOutcome::Success, context).await;
            let response =
                response.json::<WorkspaceGraphResponse>("decode modeled workspace graph");
            let actual_nodes = response.nodes.iter().map(|node| node.id).collect::<BTreeSet<_>>();
            let expected_nodes =
                resolve_handles(resources, reference.visible_active_objects(workspace, principal));
            assert_eq!(actual_nodes, expected_nodes, "{context}: workspace graph nodes");
            let actual_edges = response.edges.iter().map(|edge| edge.id).collect::<BTreeSet<_>>();
            let expected_edges =
                resolve_handles(resources, reference.visible_active_edges(workspace, principal));
            assert_eq!(actual_edges, expected_edges, "{context}: workspace graph edges");
            assert!(!response.limits.has_more_nodes, "{context}: workspace graph nodes truncated");
            assert!(!response.limits.has_more_edges, "{context}: workspace graph edges truncated");
        }
    }

    /// Checks exact-title search visibility for one modeled object.
    async fn assert_search_visibility(
        actor: &ActorClient,
        resources: &ResourceMap,
        api: &str,
        reference: &Model,
        workspace: kival_tests::Handle,
        object: kival_tests::Handle,
        context: &str,
    ) {
        let workspace_id = resources.resolve(workspace).expect("resolve searched workspace");
        let object_id = resources.resolve(object).expect("resolve searched object");
        let title = reference.object_title(object).expect("modeled object title");
        let response = actor
            .browser
            .get(format!(
                "{api}/workspaces/{workspace_id}/search?q={title}&categories=title&mode=exact&limit=100"
            ))
            .send()
            .await
            .expect("send modeled search request");
        let expected = active_workspace_outcome(
            reference,
            workspace,
            reference.can_use_workspace(workspace, actor.actor),
        );
        let response = assert_http_outcome_with_context(response, expected, context).await;
        if expected == ExpectedOutcome::Success {
            let response = response.json::<SearchResponse>("decode modeled search response");
            let object_is_active =
                reference.object(object).expect("modeled object").1 == Lifecycle::Active;
            let expected_hit = object_is_active && reference.can_read_object(object, actor.actor);
            assert_eq!(
                response.items.iter().any(|hit| hit.object_id == object_id),
                expected_hit,
                "{context}: exact-title search visibility"
            );
        }
    }

    /// Reconciles one workspace and every contained object after a broad mutation.
    async fn assert_workspace_visibility_and_access(
        fixture: &Fixture,
        resources: &ResourceMap,
        api: &str,
        namespace: &str,
        reference: &Model,
        workspace: kival_tests::Handle,
        operation: &Operation,
    ) {
        let context = serde_json::to_string(operation).expect("serialize operation context");
        audit_workspace_visibility(
            fixture, resources, api, namespace, reference, workspace, &context,
        )
        .await;
        for (object, object_workspace) in reference.objects() {
            if object_workspace != workspace {
                continue;
            }
            audit_object_access(fixture, resources, api, reference, object, &context).await;
        }
        assert_workspace_projections(fixture, resources, api, reference, workspace, &context).await;
    }

    /// Checks whether an active group link appears in a workspace's group list.
    async fn assert_workspace_group_listed(
        actor: &ActorClient,
        api: &str,
        workspace_id: uuid::Uuid,
        group_id: uuid::Uuid,
        expected: bool,
    ) {
        let groups = fetch_list::<WorkspaceGroup>(
            actor,
            &format!("{api}/workspaces/{workspace_id}/groups?limit=200"),
            ExpectedOutcome::Success,
            "workspace-group lifecycle postcondition",
        )
        .await
        .expect("workspace-group collection is readable");
        assert_eq!(
            groups.iter().any(|group| group.group_id == group_id),
            expected,
            "workspace group list disagrees with its lifecycle"
        );
    }

    /// Checks whether an active edge appears in an object's edge list.
    async fn assert_edge_listed(
        actor: &ActorClient,
        api: &str,
        workspace_id: uuid::Uuid,
        object_id: uuid::Uuid,
        edge_id: uuid::Uuid,
        expected: bool,
    ) {
        let edges = fetch_list::<ObjectEdge>(
            actor,
            &format!("{api}/workspaces/{workspace_id}/objects/{object_id}/edges?limit=200"),
            ExpectedOutcome::Success,
            "object-edge lifecycle postcondition",
        )
        .await
        .expect("object-edge collection is readable");
        assert_eq!(
            edges.iter().any(|edge| edge.id == edge_id),
            expected,
            "object edge list disagrees with its lifecycle"
        );
    }

    /// Checks whether an active workspace membership appears in the administrative list.
    async fn assert_membership_listed(
        administrator: &ActorClient,
        api: &str,
        workspace_id: uuid::Uuid,
        membership_id: uuid::Uuid,
        expected: bool,
    ) {
        let memberships = fetch_list::<WorkspaceMembership>(
            administrator,
            &format!("{api}/workspaces/{workspace_id}/memberships?limit=200"),
            ExpectedOutcome::Success,
            "workspace-membership lifecycle postcondition",
        )
        .await
        .expect("workspace-membership collection is readable");
        assert_eq!(
            memberships.iter().any(|membership| membership.id == membership_id),
            expected,
            "workspace membership list disagrees with its lifecycle"
        );
    }

    /// Checks whether an active direct grant appears in the administrative list.
    async fn assert_grant_listed(
        administrator: &ActorClient,
        api: &str,
        workspace_id: uuid::Uuid,
        object_id: uuid::Uuid,
        grant_id: uuid::Uuid,
        expected: bool,
    ) {
        let grants = fetch_list::<ObjectGrant>(
            administrator,
            &format!("{api}/workspaces/{workspace_id}/objects/{object_id}/grants?limit=200"),
            ExpectedOutcome::Success,
            "object-grant lifecycle postcondition",
        )
        .await
        .expect("object-grant collection is readable");
        assert_eq!(
            grants.iter().any(|grant| grant.id == grant_id),
            expected,
            "object grant list disagrees with its lifecycle"
        );
    }

    /// Checks an actor's modeled object visibility and effective role.
    async fn assert_actor_object_access(
        actor: &ActorClient,
        api: &str,
        workspace_id: uuid::Uuid,
        object_id: uuid::Uuid,
        expected_role: Option<ObjectRole>,
        expected: ExpectedOutcome,
        operation: &Operation,
    ) {
        let context = serde_json::to_string(operation).expect("serialize operation context");
        assert_actor_object_access_with_context(
            actor,
            api,
            workspace_id,
            object_id,
            expected_role,
            expected,
            &context,
        )
        .await;
    }

    /// Checks modeled object visibility and effective role with explicit audit context.
    async fn assert_actor_object_access_with_context(
        actor: &ActorClient,
        api: &str,
        workspace_id: uuid::Uuid,
        object_id: uuid::Uuid,
        expected_role: Option<ObjectRole>,
        expected: ExpectedOutcome,
        context: &str,
    ) {
        let response = actor
            .browser
            .get(format!("{api}/workspaces/{workspace_id}/objects/{object_id}"))
            .send()
            .await
            .expect("send modeled object-access request");
        let response = assert_http_outcome_with_context(response, expected, context).await;
        if expected == ExpectedOutcome::Success {
            let response = response.json::<ObjectResponse>("decode object-access response");
            assert_eq!(response.object.id, object_id, "{context}: object ID");
            assert_eq!(response.effective_role, expected_role.expect("modeled effective role"));
        }
    }

    /// Checks that an object appears in the correct lifecycle state in an authorized list.
    async fn assert_object_listed(
        actor: &ActorClient,
        api: &str,
        workspace_id: uuid::Uuid,
        object_id: uuid::Uuid,
        object: kival_tests::Handle,
        reference: &Model,
    ) {
        let objects = fetch_list::<ObjectResource>(
            actor,
            &format!("{api}/workspaces/{workspace_id}/objects?status=all&limit=200"),
            ExpectedOutcome::Success,
            "object lifecycle postcondition",
        )
        .await
        .expect("object collection is readable");
        let listed = objects
            .iter()
            .find(|candidate| candidate.id == object_id)
            .expect("modeled object appears in object list");
        assert_eq!(listed.status, expected_object_status(reference, object));
        assert_eq!(listed.title, reference.object_title(object).expect("modeled object title"));
    }

    /// Checks that an appended object version appears in the version list.
    async fn assert_version_listed(
        actor: &ActorClient,
        api: &str,
        workspace_id: uuid::Uuid,
        object_id: uuid::Uuid,
        version_id: uuid::Uuid,
        version_number: i64,
    ) {
        let versions = fetch_list::<ObjectVersion>(
            actor,
            &format!("{api}/workspaces/{workspace_id}/objects/{object_id}/versions?limit=200"),
            ExpectedOutcome::Success,
            "object-version append postcondition",
        )
        .await
        .expect("object-version collection is readable");
        let listed = versions
            .iter()
            .find(|version| version.id == version_id)
            .expect("appended object version appears in version list");
        assert_eq!(listed.version_number, version_number);
    }

    /// Checks an attachment response against its symbolic model entry.
    fn assert_attachment(
        resources: &ResourceMap,
        namespace: &str,
        reference: &Model,
        handle: kival_tests::Handle,
        workspace_id: uuid::Uuid,
        object_id: uuid::Uuid,
        actual: &ObjectAttachment,
    ) {
        let attachment_id = resources.resolve(handle).expect("resolve attachment handle");
        let modeled = reference.attachment(handle).expect("modeled attachment");
        let source_id = modeled
            .source
            .map(|source| resources.resolve(source).expect("resolve source attachment"));
        let expected_name = format!("{namespace}-{}", modeled.name);

        assert_eq!(actual.id, attachment_id);
        assert_eq!(actual.workspace_id, workspace_id);
        assert_eq!(actual.object_id, object_id);
        assert_eq!(actual.version_id, None);
        assert_eq!(actual.source_attachment_id, source_id);
        assert_eq!(actual.name.as_deref(), Some(expected_name.as_str()));
        assert_eq!(actual.media_type.as_deref(), Some("text/plain"));
        assert_eq!(actual.metadata, serde_json::json!({ "stateful": true }));
    }

    /// Converts the modeled workspace lifecycle to its wire representation.
    fn expected_status(reference: &Model, workspace: kival_tests::Handle) -> ArchiveStatus {
        match reference.workspace(workspace).expect("modeled workspace") {
            Lifecycle::Active => ArchiveStatus::Active,
            Lifecycle::Archived => ArchiveStatus::Archived,
        }
    }

    /// Converts the modeled object lifecycle to its wire representation.
    fn expected_object_status(reference: &Model, object: kival_tests::Handle) -> ArchiveStatus {
        match reference.object(object).expect("modeled object").1 {
            Lifecycle::Active => ArchiveStatus::Active,
            Lifecycle::Archived => ArchiveStatus::Archived,
        }
    }

    /// Converts the modeled group lifecycle to its wire representation.
    fn expected_group_status(reference: &Model, group: kival_tests::Handle) -> ArchiveStatus {
        match reference.group(group).expect("modeled group") {
            Lifecycle::Active => ArchiveStatus::Active,
            Lifecycle::Archived => ArchiveStatus::Archived,
        }
    }

    /// Returns the exact outcome for an operation requiring an active workspace.
    fn active_workspace_outcome(
        reference: &Model,
        workspace: kival_tests::Handle,
        allowed: bool,
    ) -> ExpectedOutcome {
        if reference.workspace(workspace) == Some(Lifecycle::Archived) {
            ExpectedOutcome::NotFound
        } else if allowed {
            ExpectedOutcome::Success
        } else {
            ExpectedOutcome::Forbidden
        }
    }

    /// Returns the exact outcome for an operation reading an object in either lifecycle.
    fn readable_object_outcome(
        reference: &Model,
        object: kival_tests::Handle,
        actor: Actor,
    ) -> ExpectedOutcome {
        let (workspace, _) = reference.object(object).expect("modeled object");
        if reference.workspace(workspace) == Some(Lifecycle::Archived) {
            ExpectedOutcome::NotFound
        } else if reference.can_read_object(object, actor) {
            ExpectedOutcome::Success
        } else {
            ExpectedOutcome::Forbidden
        }
    }

    /// Returns the exact outcome for an operation requiring an active object.
    fn active_object_outcome(
        reference: &Model,
        object: kival_tests::Handle,
        allowed: bool,
    ) -> ExpectedOutcome {
        let (workspace, lifecycle) = reference.object(object).expect("modeled object");
        if reference.workspace(workspace) == Some(Lifecycle::Archived)
            || lifecycle == Lifecycle::Archived
        {
            ExpectedOutcome::NotFound
        } else if allowed {
            ExpectedOutcome::Success
        } else {
            ExpectedOutcome::Forbidden
        }
    }

    /// Builds the body paired with a generated object title.
    fn object_body(title: &str) -> String {
        format!("{title} body")
    }

    /// Builds the E2E configuration while allowing larger local campaigns.
    fn stateful_config() -> proptest::test_runner::Config {
        let mut config = proptest::test_runner::Config::default();
        if std::env::var_os("PROPTEST_CASES").is_none() {
            config.cases = 8;
        }
        config
    }

    /// Returns the generated action-count range for each stateful case.
    fn stateful_history_size() -> std::ops::RangeInclusive<usize> {
        let minimum = std::env::var("KIVAL_STATEFUL_MIN_STEPS")
            .map_or(Ok(128), |value| value.parse::<usize>())
            .expect("KIVAL_STATEFUL_MIN_STEPS must be a positive integer");
        let maximum = std::env::var("KIVAL_STATEFUL_STEPS")
            .map_or(Ok(256), |value| value.parse::<usize>())
            .expect("KIVAL_STATEFUL_STEPS must be a positive integer");
        assert!(minimum > 0, "KIVAL_STATEFUL_MIN_STEPS must be greater than zero");
        assert!(
            minimum <= maximum,
            "KIVAL_STATEFUL_MIN_STEPS must not exceed KIVAL_STATEFUL_STEPS"
        );
        minimum..=maximum
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn server_matches_resource_model(pool: PgPool) {
        let runtime = Handle::current();
        tokio::task::spawn_blocking(move || {
            let _context_guard = StatefulContextGuard::install(pool, runtime);
            proptest::proptest!(stateful_config(), |(
                (initial_state, transitions, seen_counter) in
                    KivalStateMachine::sequential_strategy(stateful_history_size())
            )| {
                ServerStateMachine::test_sequential(
                    stateful_config(),
                    initial_state,
                    transitions,
                    seen_counter,
                );
            });
        })
        .await
        .expect("run stateful campaign");
    }
}
