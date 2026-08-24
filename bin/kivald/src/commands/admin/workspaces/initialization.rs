//! Private one-shot workspace initialization for `kivald admin`.
//!
//! This module is intentionally not part of Kival's HTTP API or SDK surface. Recipes are only
//! accepted while a new workspace is being created. Once the transaction commits, only ordinary
//! Kival users, memberships, objects, versions, grants, relationships, references, and events
//! remain.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
};

use eyre::{Context, Result, bail, eyre};
use kival_kernel::{
    CreateInitialObject, EventInsert, EventKind, GrantPrincipal, MembershipRole, ObjectRole,
    UpdateObjectVersion, append_event, create_comment, create_comment_thread, create_initial_object,
    create_object_edge, create_object_grant, create_user, create_workspace,
    create_workspace_membership, lock_admin_provisioning, lock_user_for_operator,
    maintain_object_references, set_thread_resolved, set_user_disabled_as_operator,
    touch_comment_thread, update_object_version,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

/// Schema version understood by the bundled initializer catalog parser.
const SCHEMA_VERSION: u32 = 2;
/// Bundled JSON catalog containing reusable templates and demo scenarios.
const CATALOG: &str = include_str!("catalog.json");

/// Semantic kind of one bundled initializer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkspaceInitializerKind {
    /// Reusable starting structure intended for continued work.
    Template,
    /// Disposable showcase scenario tailored to a target audience.
    Demo,
}

impl fmt::Display for WorkspaceInitializerKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Template => "template",
            Self::Demo => "demo",
        })
    }
}

/// Selects one bundled initializer by semantic kind and stable configuration ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WorkspaceInitializer {
    /// Initializer category.
    pub(super) kind: WorkspaceInitializerKind,
    /// Stable catalog identifier.
    pub(super) id: String,
}

impl WorkspaceInitializer {
    /// Selects one reusable template.
    #[must_use]
    pub(super) fn template(id: impl Into<String>) -> Self {
        Self { kind: WorkspaceInitializerKind::Template, id: id.into() }
    }

    /// Selects one disposable demo scenario.
    #[must_use]
    pub(super) fn demo(id: impl Into<String>) -> Self {
        Self { kind: WorkspaceInitializerKind::Demo, id: id.into() }
    }
}

/// Operator-facing catalog entry.
#[derive(Debug)]
pub(super) struct WorkspaceInitializerOption {
    /// Initializer category.
    pub(super) kind: WorkspaceInitializerKind,
    /// Stable catalog identifier.
    pub(super) id: String,
    /// Human-readable name.
    pub(super) name: String,
    /// Optional operator-facing description.
    pub(super) description: Option<String>,
}

/// Result of one privileged workspace creation.
#[derive(Debug)]
pub(super) struct InitializedWorkspace {
    /// Newly created workspace identifier.
    pub(super) workspace_id: Uuid,
    /// Bootstrapped administrator that owns the workspace.
    pub(super) owner_user_id: Uuid,
    /// Username of the bootstrapped administrator.
    pub(super) owner_username: String,
}

/// Versioned catalog of all bundled workspace initializers.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    /// Catalog schema version.
    schema_version: u32,
    /// Reusable workspace templates.
    #[serde(default)]
    templates: Vec<TemplateRecipe>,
    /// Disposable demo scenarios.
    #[serde(default)]
    demos: Vec<DemoRecipe>,
}

/// Reusable workspace initializer intended as a starting point for continued work.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateRecipe {
    /// Stable catalog identifier used by the admin CLI.
    id: String,
    /// Human-readable template name.
    name: String,
    /// Optional operator-facing description.
    #[serde(default)]
    description: Option<String>,
    /// Ordered mutations applied while the workspace is created.
    actions: Vec<SeedAction>,
}

/// Disposable showcase initializer with demo-only historical actors.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DemoRecipe {
    /// Stable catalog identifier used by the admin CLI.
    id: String,
    /// Human-readable scenario name.
    name: String,
    /// Optional operator-facing description.
    #[serde(default)]
    description: Option<String>,
    /// Synthetic historical actors keyed by names used in scenario actions.
    actors: BTreeMap<String, DemoActor>,
    /// Objects pinned for the real workspace owner after the scenario is seeded.
    #[serde(default)]
    owner_pins: Vec<String>,
    /// Objects favorited for the real workspace owner after the scenario is seeded.
    #[serde(default)]
    owner_favorites: Vec<String>,
    /// Ordered actor-attributed mutations applied during initialization.
    actions: Vec<DemoSeedAction>,
}

/// A demo-only historical fixture. These become credential-less Kival users for initialization so
/// ordinary projections can display real authorship without any demo-aware production code.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DemoActor {
    /// Preferred username for the credential-less fixture user.
    username: String,
    /// Display name shown in ordinary Kival history and attribution UI.
    display_name: String,
}

/// One demo mutation attributed to a configured synthetic actor.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DemoSeedAction {
    /// Key of the configured demo actor performing the mutation.
    actor: String,
    /// Reusable seed mutation attributed to that actor.
    action: SeedAction,
}

/// Reusable state mutations shared by templates and demos.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum SeedAction {
    /// Creates an ordinary Kival object and its initial version.
    CreateObject {
        /// Recipe-local key used by subsequent actions.
        key: String,
        /// Initial object title.
        title: String,
        /// Initial object body.
        #[serde(default)]
        body: String,
        /// Initial object metadata.
        #[serde(default = "empty_metadata")]
        metadata: Value,
    },
    /// Creates a new version of an object created earlier in the recipe.
    UpdateObject {
        /// Recipe-local key of the object to update.
        object: String,
        /// Replacement title, when changed.
        #[serde(default)]
        title: Option<String>,
        /// Replacement body, when changed.
        #[serde(default)]
        body: Option<String>,
        /// Replacement metadata, when changed.
        #[serde(default)]
        metadata: Option<Value>,
    },
    /// Creates an explicit relationship between two previously created objects.
    CreateRelationship {
        /// Recipe-local key of the source object.
        source: String,
        /// Recipe-local key of the target object.
        target: String,
    },
    /// Starts a commentary thread on a previously created object.
    CreateCommentThread {
        /// Recipe-local key used by later replies or resolution actions.
        key: String,
        /// Recipe-local key of the commented object.
        object: String,
        /// Initial comment body.
        body: String,
    },
    /// Adds a reply to a previously created commentary thread.
    ReplyToCommentThread {
        /// Recipe-local key of the thread receiving the reply.
        thread: String,
        /// Reply body.
        body: String,
    },
    /// Marks a previously created commentary thread resolved.
    ResolveCommentThread {
        /// Recipe-local key of the thread to resolve.
        thread: String,
    },
}

/// Runtime identifiers tracked for an object created by the current initializer.
#[derive(Debug, Clone, Copy)]
struct SeedObjectState {
    /// Persisted object identifier.
    object_id: Uuid,
    /// Identifier of the object's latest seeded version.
    current_version_id: Uuid,
}

/// Runtime identifiers tracked for a commentary thread created by the current initializer.
#[derive(Debug, Clone, Copy)]
struct SeedCommentThreadState {
    /// Object containing the thread.
    object_id: Uuid,
    /// Persisted thread identifier.
    thread_id: Uuid,
    /// Root comment identifier used as the parent for replies.
    root_comment_id: Uuid,
    /// Author of the root comment, used for ordinary reply-event attribution.
    root_author_user_id: Uuid,
}

/// Execution-scoped identities shared across one seed action.
#[derive(Debug, Clone, Copy)]
struct SeedExecutionContext<'a> {
    /// Newly created workspace receiving the seeded content.
    workspace_id: Uuid,
    /// Bootstrapped administrator that owns initialized content.
    owner_user_id: Uuid,
    /// User attributed as the actor for the current action.
    actor_id: Uuid,
    /// Demo fixture users that may receive ordinary object grants.
    demo_actor_ids: &'a [Uuid],
}

/// Borrowed initializer selected from the validated catalog.
enum ResolvedInitializer<'a> {
    /// Reusable template selection.
    Template(&'a TemplateRecipe),
    /// Disposable demo scenario selection.
    Demo(&'a DemoRecipe),
}

/// Lists all bundled one-shot workspace initializers.
///
/// # Errors
///
/// Returns an error when the embedded catalog cannot be parsed or validated.
pub(super) fn workspace_initializers() -> Result<Vec<WorkspaceInitializerOption>> {
    let catalog = load_catalog()?;
    validate_catalog(&catalog)?;

    Ok(catalog
        .templates
        .iter()
        .map(|recipe| WorkspaceInitializerOption {
            kind: WorkspaceInitializerKind::Template,
            id: recipe.id.clone(),
            name: recipe.name.clone(),
            description: recipe.description.clone(),
        })
        .chain(catalog.demos.iter().map(|recipe| WorkspaceInitializerOption {
            kind: WorkspaceInitializerKind::Demo,
            id: recipe.id.clone(),
            name: recipe.name.clone(),
            description: recipe.description.clone(),
        }))
        .collect())
}

/// Creates a workspace for the bootstrapped administrator and optionally initializes it once.
///
/// The initializer cannot be applied to an existing workspace because this is the only execution
/// entry point and it owns workspace creation and initialization in the same transaction.
///
/// # Errors
///
/// Returns an error when Kival is not bootstrapped, the catalog is invalid, the initializer is
/// unknown, or any workspace initialization mutation fails.
pub(super) async fn create_workspace_as_operator(
    pool: &PgPool,
    name: &str,
    description: Option<&str>,
    initializer: Option<&WorkspaceInitializer>,
) -> Result<InitializedWorkspace> {
    let name = name.trim();
    if name.is_empty() {
        bail!("workspace name must not be empty");
    }

    let catalog = load_catalog()?;
    validate_catalog(&catalog)?;
    let resolved = resolve_initializer(&catalog, initializer)?;

    let mut tx = pool.begin().await.wrap_err("failed to begin workspace creation transaction")?;
    lock_admin_provisioning(&mut tx).await.wrap_err("failed to acquire admin provisioning lock")?;

    let Some((owner_user_id, owner_username, owner_disabled)) = lock_bootstrap_admin(&mut tx)
        .await
        .wrap_err("failed to resolve bootstrap administrator")?
    else {
        bail!("Kival is not bootstrapped; run `kivald admin bootstrap` first");
    };
    if owner_disabled {
        bail!("bootstrap administrator is disabled");
    }

    let workspace = create_workspace(&mut tx, name, description.map(str::trim), owner_user_id)
        .await
        .wrap_err("failed to create workspace")?;
    emit(
        &mut tx,
        EventInsert::new(
            owner_user_id,
            EventKind::WorkspaceCreated,
            json!({ "workspace_id": workspace.id }),
        )
        .workspace(workspace.id),
    )
    .await?;

    match resolved {
        None => {}
        Some(ResolvedInitializer::Template(recipe)) => {
            execute_template(&mut tx, workspace.id, owner_user_id, recipe).await?;
        }
        Some(ResolvedInitializer::Demo(recipe)) => {
            execute_demo(&mut tx, workspace.id, owner_user_id, recipe).await?;
        }
    }

    tx.commit().await.wrap_err("failed to commit workspace creation transaction")?;

    Ok(InitializedWorkspace { workspace_id: workspace.id, owner_user_id, owner_username })
}

/// Locks and returns the administrator created by the original bootstrap operation.
async fn lock_bootstrap_admin(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<Option<(Uuid, String, bool)>> {
    Ok(sqlx::query_as::<_, (Uuid, String, bool)>(
        r#"
        SELECT u.id, u.username, u.disabled_at IS NOT NULL
        FROM kival.events e
        JOIN kival.users u
            ON u.id = e.target_user_id
        JOIN kival.global_admins ga
            ON ga.user_id = u.id
            AND ga.revoked_at IS NULL
        WHERE e.event_kind = $1
        ORDER BY e.sequence_number ASC
        LIMIT 1
        FOR UPDATE OF u
        "#,
    )
    .bind(EventKind::AdminBootstrapCompleted.as_str())
    .fetch_optional(&mut **tx)
    .await?)
}

/// Executes a reusable template as the bootstrapped workspace owner.
async fn execute_template(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    owner_user_id: Uuid,
    recipe: &TemplateRecipe,
) -> Result<()> {
    let mut objects = HashMap::new();
    let mut comment_threads = HashMap::new();
    let context = SeedExecutionContext {
        workspace_id,
        owner_user_id,
        actor_id: owner_user_id,
        demo_actor_ids: &[],
    };
    for action in &recipe.actions {
        execute_action(tx, context, &mut objects, &mut comment_threads, action).await?;
    }
    Ok(())
}

/// Executes a demo scenario using temporary credential-less fixture users for attribution.
async fn execute_demo(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    owner_user_id: Uuid,
    recipe: &DemoRecipe,
) -> Result<()> {
    let actors = provision_demo_actors(tx, workspace_id, owner_user_id, &recipe.actors).await?;
    let actor_ids = actors.values().copied().collect::<Vec<_>>();
    let mut objects = HashMap::new();
    let mut comment_threads = HashMap::new();

    for seeded in &recipe.actions {
        let actor_id = *actors
            .get(&seeded.actor)
            .ok_or_else(|| eyre!("demo action references unknown actor {:?}", seeded.actor))?;
        let context = SeedExecutionContext {
            workspace_id,
            owner_user_id,
            actor_id,
            demo_actor_ids: &actor_ids,
        };
        execute_action(tx, context, &mut objects, &mut comment_threads, &seeded.action).await?;
    }

    seed_owner_object_markers(
        tx,
        workspace_id,
        owner_user_id,
        &objects,
        &recipe.owner_pins,
        &recipe.owner_favorites,
    )
    .await?;

    // Demo actors are historical fixtures, not accounts that should ever authenticate.
    for actor_id in actor_ids {
        set_user_disabled_as_operator(tx, actor_id, true)
            .await
            .wrap_err("failed to disable demo actor fixture")?;
        emit(
            tx,
            EventInsert::new(
                owner_user_id,
                EventKind::UserDisabled,
                json!({ "user_id": actor_id, "demo_fixture": true }),
            )
            .target_user(actor_id),
        )
        .await?;
    }

    Ok(())
}

/// Creates demo-only fixture users and workspace memberships for historical attribution.
async fn provision_demo_actors(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    owner_user_id: Uuid,
    actors: &BTreeMap<String, DemoActor>,
) -> Result<HashMap<String, Uuid>> {
    let mut provisioned = HashMap::new();

    for (key, actor) in actors {
        let username = available_demo_username(tx, workspace_id, &actor.username).await?;
        let created = create_user(tx, &username, actor.display_name.trim())
            .await
            .wrap_err_with(|| format!("failed to create demo actor {key:?}"))?;

        emit(
            tx,
            EventInsert::new(
                owner_user_id,
                EventKind::UserCreated,
                json!({
                    "user_id": created.id,
                    "username": created.username,
                    "demo_fixture": true,
                }),
            )
            .target_user(created.id),
        )
        .await?;

        let membership = create_workspace_membership(
            tx,
            workspace_id,
            Some(created.id),
            None,
            MembershipRole::Member,
            owner_user_id,
        )
        .await
        .wrap_err("failed to create demo actor workspace membership")?;
        emit(
            tx,
            EventInsert::new(
                owner_user_id,
                EventKind::WorkspaceMembershipCreated,
                json!({
                    "workspace_membership_id": membership.id,
                    "workspace_role": membership.workspace_role.as_str(),
                    "demo_fixture": true,
                }),
            )
            .workspace(workspace_id)
            .target_user(created.id),
        )
        .await?;

        provisioned.insert(key.clone(), created.id);
    }

    Ok(provisioned)
}

/// Resolves a collision-free username for a demo fixture user.
async fn available_demo_username(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    preferred: &str,
) -> Result<String> {
    if lock_user_for_operator(tx, preferred)
        .await
        .wrap_err("failed to check demo actor username")?
        .is_none()
    {
        return Ok(preferred.to_owned());
    }

    let compact = workspace_id.simple().to_string();
    let suffix = &compact[compact.len() - 8..];
    let fallback = format!("{preferred}-demo-{suffix}");
    if lock_user_for_operator(tx, &fallback)
        .await
        .wrap_err("failed to check fallback demo actor username")?
        .is_some()
    {
        bail!("demo actor username collision for {preferred:?}");
    }
    Ok(fallback)
}

/// Applies one validated seed action and updates recipe-local object state.
async fn execute_action(
    tx: &mut Transaction<'_, Postgres>,
    context: SeedExecutionContext<'_>,
    objects: &mut HashMap<String, SeedObjectState>,
    comment_threads: &mut HashMap<String, SeedCommentThreadState>,
    action: &SeedAction,
) -> Result<()> {
    match action {
        SeedAction::CreateObject { key, title, body, metadata } => {
            let state = create_seed_object(tx, context, title, body, metadata.clone()).await?;
            objects.insert(key.clone(), state);
        }
        SeedAction::UpdateObject { object, title, body, metadata } => {
            let state = objects
                .get_mut(object)
                .ok_or_else(|| eyre!("initializer references unknown object {object:?}"))?;
            state.current_version_id = update_seed_object(
                tx,
                context.workspace_id,
                context.actor_id,
                *state,
                title.clone(),
                body.clone(),
                metadata.clone(),
            )
            .await?;
        }
        SeedAction::CreateRelationship { source, target } => {
            let source = objects
                .get(source)
                .ok_or_else(|| eyre!("initializer references unknown source object {source:?}"))?;
            let target = objects
                .get(target)
                .ok_or_else(|| eyre!("initializer references unknown target object {target:?}"))?;
            create_seed_relationship(
                tx,
                context.workspace_id,
                context.actor_id,
                source.object_id,
                target.object_id,
            )
            .await?;
        }
        SeedAction::CreateCommentThread { key, object, body } => {
            let object = objects.get(object).ok_or_else(|| {
                eyre!("initializer commentary references unknown object {object:?}")
            })?;
            let thread = create_seed_comment_thread(
                tx,
                context.workspace_id,
                object.object_id,
                context.actor_id,
                body,
            )
            .await?;
            comment_threads.insert(key.clone(), thread);
        }
        SeedAction::ReplyToCommentThread { thread, body } => {
            let thread = *comment_threads.get(thread).ok_or_else(|| {
                eyre!("initializer references unknown commentary thread {thread:?}")
            })?;
            reply_to_seed_comment_thread(
                tx,
                context.workspace_id,
                context.actor_id,
                thread,
                body,
            )
            .await?;
        }
        SeedAction::ResolveCommentThread { thread } => {
            let thread = *comment_threads.get(thread).ok_or_else(|| {
                eyre!("initializer references unknown commentary thread {thread:?}")
            })?;
            resolve_seed_comment_thread(
                tx,
                context.workspace_id,
                context.actor_id,
                thread,
            )
            .await?;
        }
    }

    Ok(())
}

/// Seeds personal pins and favorites for the real workspace owner.
async fn seed_owner_object_markers(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    owner_user_id: Uuid,
    objects: &HashMap<String, SeedObjectState>,
    pins: &[String],
    favorites: &[String],
) -> Result<()> {
    for key in pins {
        let object = objects
            .get(key)
            .ok_or_else(|| eyre!("demo owner pin references unknown object {key:?}"))?;
        sqlx::query(
            r#"
            INSERT INTO kival.object_pins (user_id, workspace_id, object_id)
            VALUES ($1, $2, $3)
            ON CONFLICT (user_id, object_id) DO NOTHING
            "#,
        )
        .bind(owner_user_id)
        .bind(workspace_id)
        .bind(object.object_id)
        .execute(&mut **tx)
        .await
        .wrap_err("failed to seed demo object pin")?;
    }

    for key in favorites {
        let object = objects
            .get(key)
            .ok_or_else(|| eyre!("demo owner favorite references unknown object {key:?}"))?;
        sqlx::query(
            r#"
            INSERT INTO kival.object_favorites (user_id, workspace_id, object_id)
            VALUES ($1, $2, $3)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(owner_user_id)
        .bind(workspace_id)
        .bind(object.object_id)
        .execute(&mut **tx)
        .await
        .wrap_err("failed to seed demo object favorite")?;
    }

    Ok(())
}

/// Starts one seeded commentary thread and emits the ordinary creation event.
async fn create_seed_comment_thread(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    object_id: Uuid,
    actor_id: Uuid,
    body: &str,
) -> Result<SeedCommentThreadState> {
    let thread_id = create_comment_thread(tx, workspace_id, object_id, actor_id)
        .await
        .wrap_err("failed to create initialized comment thread")?;
    let comment_id = create_comment(
        tx,
        workspace_id,
        object_id,
        thread_id,
        None,
        actor_id,
        body,
    )
    .await
    .wrap_err("failed to create initialized comment")?;

    emit(
        tx,
        EventInsert::new(
            actor_id,
            EventKind::CommentCreated,
            json!({ "thread_id": thread_id, "comment_id": comment_id }),
        )
        .workspace(workspace_id)
        .object(object_id)
        .comment_thread(thread_id)
        .comment(comment_id),
    )
    .await?;

    Ok(SeedCommentThreadState {
        object_id,
        thread_id,
        root_comment_id: comment_id,
        root_author_user_id: actor_id,
    })
}

/// Adds one seeded reply and emits the ordinary reply event.
async fn reply_to_seed_comment_thread(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    actor_id: Uuid,
    thread: SeedCommentThreadState,
    body: &str,
) -> Result<()> {
    let comment_id = create_comment(
        tx,
        workspace_id,
        thread.object_id,
        thread.thread_id,
        Some(thread.root_comment_id),
        actor_id,
        body,
    )
    .await
    .wrap_err("failed to create initialized comment reply")?;
    touch_comment_thread(tx, thread.thread_id)
        .await
        .wrap_err("failed to update initialized comment thread activity")?;

    emit(
        tx,
        EventInsert::new(
            actor_id,
            EventKind::CommentReplied,
            json!({
                "thread_id": thread.thread_id,
                "comment_id": comment_id,
                "parent_comment_id": thread.root_comment_id,
            }),
        )
        .workspace(workspace_id)
        .object(thread.object_id)
        .comment_thread(thread.thread_id)
        .comment(comment_id)
        .target_user(thread.root_author_user_id),
    )
    .await
}

/// Resolves one seeded commentary thread and emits the ordinary resolution event.
async fn resolve_seed_comment_thread(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    actor_id: Uuid,
    thread: SeedCommentThreadState,
) -> Result<()> {
    set_thread_resolved(
        tx,
        workspace_id,
        thread.object_id,
        thread.thread_id,
        actor_id,
        true,
    )
    .await
    .wrap_err("failed to resolve initialized comment thread")?;

    emit(
        tx,
        EventInsert::new(
            actor_id,
            EventKind::CommentThreadResolved,
            json!({ "thread_id": thread.thread_id }),
        )
        .workspace(workspace_id)
        .object(thread.object_id)
        .comment_thread(thread.thread_id),
    )
    .await
}

/// Creates one seeded object with normal grants, references, and events.
async fn create_seed_object(
    tx: &mut Transaction<'_, Postgres>,
    context: SeedExecutionContext<'_>,
    title: &str,
    body: &str,
    metadata: Value,
) -> Result<SeedObjectState> {
    let created = create_initial_object(
        tx,
        CreateInitialObject {
            workspace_id: context.workspace_id,
            title: title.to_owned(),
            body: body.to_owned(),
            metadata,
            created_by: context.actor_id,
        },
    )
    .await
    .wrap_err("failed to create initialized object")?;

    let object_id = created.object_id;
    let version_id = created.version.id;
    let maintenance = maintain_object_references(
        tx,
        context.workspace_id,
        object_id,
        version_id,
        &[title.to_owned()],
    )
    .await
    .wrap_err("failed to maintain initialized object references")?;

    emit(
        tx,
        EventInsert::new(
            context.actor_id,
            EventKind::ObjectCreated,
            json!({ "object_id": object_id, "object_version_id": version_id }),
        )
        .workspace(context.workspace_id)
        .object(object_id)
        .object_version(version_id),
    )
    .await?;
    emit(
        tx,
        EventInsert::new(
            context.actor_id,
            EventKind::ObjectGrantCreated,
            json!({
                "object_grant_id": created.creator_grant_id,
                "object_role": "admin",
            }),
        )
        .workspace(context.workspace_id)
        .object(object_id)
        .object_grant(created.creator_grant_id)
        .target_user(context.actor_id),
    )
    .await?;

    emit_reference_events(
        tx,
        context.workspace_id,
        context.actor_id,
        object_id,
        version_id,
        title,
        maintenance,
    )
    .await?;

    // A demo represents a shared workspace. Grant every other demo actor editor access so later
    // configured actions are permissions-consistent with the normal API even though this privileged
    // executor itself does not run HTTP authorization checks.
    for grantee in
        context.demo_actor_ids.iter().copied().filter(|grantee| *grantee != context.actor_id)
    {
        let grant = create_object_grant(
            tx,
            context.workspace_id,
            object_id,
            GrantPrincipal::User(grantee),
            ObjectRole::Editor,
            context.owner_user_id,
        )
        .await
        .wrap_err("failed to grant demo actor object access")?;
        emit(
            tx,
            EventInsert::new(
                context.owner_user_id,
                EventKind::ObjectGrantCreated,
                json!({ "object_grant_id": grant.id, "object_role": "editor", "demo_fixture": true }),
            )
            .workspace(context.workspace_id)
            .object(object_id)
            .object_grant(grant.id)
            .target_user(grantee),
        )
        .await?;
    }

    Ok(SeedObjectState { object_id, current_version_id: version_id })
}

/// Creates a new seeded object version while preserving normal reference and event behavior.
async fn update_seed_object(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    actor_id: Uuid,
    state: SeedObjectState,
    title: Option<String>,
    body: Option<String>,
    metadata: Option<Value>,
) -> Result<Uuid> {
    let updated = update_object_version(
        tx,
        UpdateObjectVersion {
            workspace_id,
            object_id: state.object_id,
            expected_current_version_id: state.current_version_id,
            title,
            body,
            metadata,
            created_by: actor_id,
        },
    )
    .await
    .wrap_err("failed to update initialized object")?;

    if !updated.changed {
        return Ok(updated.version.id);
    }

    let new_title = updated.version.title.clone();
    let affected_titles = if updated.previous_title == new_title {
        Vec::new()
    } else {
        vec![updated.previous_title, new_title]
    };
    let maintenance = maintain_object_references(
        tx,
        workspace_id,
        state.object_id,
        updated.version.id,
        &affected_titles,
    )
    .await
    .wrap_err("failed to maintain updated object references")?;

    emit(
        tx,
        EventInsert::new(
            actor_id,
            EventKind::ObjectVersionAppended,
            json!({
                "object_id": state.object_id,
                "object_version_id": updated.version.id,
            }),
        )
        .workspace(workspace_id)
        .object(state.object_id)
        .object_version(updated.version.id),
    )
    .await?;
    emit(
        tx,
        EventInsert::new(
            actor_id,
            EventKind::ObjectUpdated,
            json!({ "object_id": state.object_id }),
        )
        .workspace(workspace_id)
        .object(state.object_id)
        .object_version(updated.version.id),
    )
    .await?;

    if maintenance.reference_update.changed() {
        let reference_update = maintenance.reference_update;
        emit(
            tx,
            EventInsert::new(
                actor_id,
                EventKind::ObjectReferencesUpdated,
                json!({
                    "object_id": state.object_id,
                    "version_id": updated.version.id,
                    "resolved_count": reference_update.resolved_count,
                    "unresolved_count": reference_update.unresolved_count,
                    "ambiguous_count": reference_update.ambiguous_count,
                    "stale_count": reference_update.stale_count,
                }),
            )
            .workspace(workspace_id)
            .object(state.object_id)
            .object_version(updated.version.id),
        )
        .await?;
    }

    emit_reresolution_event(
        tx,
        workspace_id,
        actor_id,
        state.object_id,
        &affected_titles,
        maintenance.reresolution,
    )
    .await?;

    Ok(updated.version.id)
}

/// Creates one explicit relationship between seeded objects and emits its normal event.
async fn create_seed_relationship(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    actor_id: Uuid,
    source_object_id: Uuid,
    target_object_id: Uuid,
) -> Result<()> {
    let edge = create_object_edge(tx, workspace_id, source_object_id, target_object_id, actor_id)
        .await
        .wrap_err("failed to create initialized object relationship")?;

    emit(
        tx,
        EventInsert::new(
            actor_id,
            EventKind::ObjectEdgeCreated,
            json!({
                "object_edge_id": edge.id,
                "source_object_id": edge.source_object_id,
                "target_object_id": edge.target_object_id,
            }),
        )
        .workspace(workspace_id)
        .object(edge.source_object_id)
        .object_edge(edge.id),
    )
    .await
}

/// Emits reference-maintenance events produced by seeded object creation or updates.
async fn emit_reference_events(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    actor_id: Uuid,
    object_id: Uuid,
    version_id: Uuid,
    title: &str,
    maintenance: kival_kernel::ObjectReferenceMaintenance,
) -> Result<()> {
    if maintenance.reference_update.changed() {
        let update = maintenance.reference_update;
        emit(
            tx,
            EventInsert::new(
                actor_id,
                EventKind::ObjectReferencesUpdated,
                json!({
                    "object_id": object_id,
                    "version_id": version_id,
                    "resolved_count": update.resolved_count,
                    "unresolved_count": update.unresolved_count,
                    "ambiguous_count": update.ambiguous_count,
                    "stale_count": update.stale_count,
                }),
            )
            .workspace(workspace_id)
            .object(object_id)
            .object_version(version_id),
        )
        .await?;
    }

    emit_reresolution_event(
        tx,
        workspace_id,
        actor_id,
        object_id,
        &[title.to_owned()],
        maintenance.reresolution,
    )
    .await
}

/// Emits the aggregate event describing reference re-resolution after a seed mutation.
async fn emit_reresolution_event(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: Uuid,
    actor_id: Uuid,
    object_id: Uuid,
    affected_titles: &[String],
    summary: kival_kernel::ReferenceReresolutionSummary,
) -> Result<()> {
    if !summary.changed() {
        return Ok(());
    }

    emit(
        tx,
        EventInsert::new(
            actor_id,
            EventKind::ObjectWikilinksReresolved,
            json!({
                "affected_titles": affected_titles,
                "updated_count": summary.updated_count,
                "resolved_count": summary.resolved_count,
                "unresolved_count": summary.unresolved_count,
                "ambiguous_count": summary.ambiguous_count,
            }),
        )
        .workspace(workspace_id)
        .object(object_id),
    )
    .await
}

/// Appends one event produced by the privileged initializer.
async fn emit(tx: &mut Transaction<'_, Postgres>, event: EventInsert) -> Result<()> {
    append_event(tx, event).await.wrap_err("failed to append initialization event")?;
    Ok(())
}

/// Parses the embedded initializer catalog.
fn load_catalog() -> Result<Catalog> {
    serde_json::from_str(CATALOG).wrap_err("invalid bundled workspace initializer catalog JSON")
}

/// Resolves an optional CLI initializer selection against the loaded catalog.
fn resolve_initializer<'a>(
    catalog: &'a Catalog,
    initializer: Option<&WorkspaceInitializer>,
) -> Result<Option<ResolvedInitializer<'a>>> {
    let Some(initializer) = initializer else {
        return Ok(None);
    };

    match initializer.kind {
        WorkspaceInitializerKind::Template => {
            if let Some(recipe) =
                catalog.templates.iter().find(|recipe| recipe.id == initializer.id)
            {
                return Ok(Some(ResolvedInitializer::Template(recipe)));
            }
            if catalog.demos.iter().any(|recipe| recipe.id == initializer.id) {
                bail!(
                    "initializer {:?} is a demo scenario; use `--demo {}` instead of `--template`",
                    initializer.id,
                    initializer.id
                );
            }
            bail!("unknown workspace template {:?}", initializer.id)
        }
        WorkspaceInitializerKind::Demo => {
            if let Some(recipe) =
                catalog.demos.iter().find(|recipe| recipe.id == initializer.id)
            {
                return Ok(Some(ResolvedInitializer::Demo(recipe)));
            }
            if catalog.templates.iter().any(|recipe| recipe.id == initializer.id) {
                bail!(
                    "initializer {:?} is a reusable template; use `--template {}` instead of `--demo`",
                    initializer.id,
                    initializer.id
                );
            }
            bail!("unknown demo scenario {:?}", initializer.id)
        }
    }
}

/// Validates catalog versioning, identities, demo actors, and action ordering.
fn validate_catalog(catalog: &Catalog) -> Result<()> {
    if catalog.schema_version != SCHEMA_VERSION {
        bail!(
            "unsupported workspace initializer catalog schema version {}; expected {}",
            catalog.schema_version,
            SCHEMA_VERSION
        );
    }

    let template_ids =
        catalog.templates.iter().map(|recipe| recipe.id.as_str()).collect::<Vec<_>>();
    let demo_ids = catalog.demos.iter().map(|recipe| recipe.id.as_str()).collect::<Vec<_>>();
    validate_recipe_ids(&template_ids, "template")?;
    validate_recipe_ids(&demo_ids, "demo")?;

    for recipe in &catalog.templates {
        validate_identity(&recipe.id, &recipe.name, "template")?;
        validate_actions(&recipe.actions)?;
    }

    for recipe in &catalog.demos {
        validate_identity(&recipe.id, &recipe.name, "demo")?;
        if recipe.actors.is_empty() {
            bail!("demo {:?} must declare at least one actor", recipe.id);
        }

        let mut usernames = HashSet::new();
        for (key, actor) in &recipe.actors {
            validate_demo_actor(key, actor)?;
            if !usernames.insert(actor.username.to_ascii_lowercase()) {
                bail!(
                    "demo {:?} declares duplicate actor username {:?}",
                    recipe.id,
                    actor.username
                );
            }
        }

        for seeded in &recipe.actions {
            if !recipe.actors.contains_key(&seeded.actor) {
                bail!("demo {:?} action references unknown actor {:?}", recipe.id, seeded.actor);
            }
        }
        let actions = recipe.actions.iter().map(|seeded| seeded.action.clone()).collect::<Vec<_>>();
        validate_actions(&actions)?;
        validate_demo_owner_markers(recipe, &actions)?;
    }

    Ok(())
}

/// Validates demo owner pins and favorites against objects declared by the recipe.
fn validate_demo_owner_markers(recipe: &DemoRecipe, actions: &[SeedAction]) -> Result<()> {
    let object_keys = actions
        .iter()
        .filter_map(|action| match action {
            SeedAction::CreateObject { key, .. } => Some(key.as_str()),
            _ => None,
        })
        .collect::<HashSet<_>>();

    for (kind, keys) in [("pin", &recipe.owner_pins), ("favorite", &recipe.owner_favorites)] {
        let mut seen = HashSet::new();
        for key in keys {
            if !object_keys.contains(key.as_str()) {
                bail!("demo {:?} owner {kind} references unknown object {key:?}", recipe.id);
            }
            if !seen.insert(key.as_str()) {
                bail!("demo {:?} declares duplicate owner {kind} for object {key:?}", recipe.id);
            }
        }
    }

    Ok(())
}

/// Rejects duplicate initializer identifiers within one catalog section.
fn validate_recipe_ids(ids: &[&str], kind: &str) -> Result<()> {
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(*id) {
            bail!("duplicate {kind} initializer ID {id:?}");
        }
    }
    Ok(())
}

/// Validates the required identifier and display name of one initializer.
fn validate_identity(id: &str, name: &str, kind: &str) -> Result<()> {
    if id.trim().is_empty() {
        bail!("{kind} initializer ID must not be empty");
    }
    if name.trim().is_empty() {
        bail!("{kind} initializer {id:?} name must not be empty");
    }
    Ok(())
}

/// Validates one demo actor key, display name, and preferred username.
fn validate_demo_actor(key: &str, actor: &DemoActor) -> Result<()> {
    if key.is_empty() {
        bail!("demo actor key must not be empty");
    }
    if actor.display_name.trim().is_empty() {
        bail!("demo actor {key:?} display name must not be empty");
    }

    let username = actor.username.as_str();
    if username.is_empty() || username.len() > 16 {
        bail!("demo actor {key:?} username must contain 1 to 16 characters");
    }
    let Some(first) = username.chars().next() else {
        bail!("demo actor {key:?} username must not be empty");
    };
    let Some(last) = username.chars().last() else {
        bail!("demo actor {key:?} username must not be empty");
    };
    if !first.is_ascii_alphanumeric()
        || !last.is_ascii_alphanumeric()
        || !username.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || "._-".contains(character)
        })
    {
        bail!("demo actor {key:?} has invalid username {username:?}");
    }
    Ok(())
}

/// Validates seed action ordering and all recipe-local object references.
fn validate_actions(actions: &[SeedAction]) -> Result<()> {
    let mut objects = HashSet::new();
    let mut comment_threads = HashSet::new();
    let mut resolved_comment_threads = HashSet::new();

    for action in actions {
        match action {
            SeedAction::CreateObject { key, title, metadata, .. } => {
                if key.trim().is_empty() {
                    bail!("initializer object key must not be empty");
                }
                if !objects.insert(key.as_str()) {
                    bail!("initializer declares duplicate object key {key:?}");
                }
                if title.trim().is_empty() {
                    bail!("initializer object {key:?} title must not be empty");
                }
                validate_metadata(metadata)?;
            }
            SeedAction::UpdateObject { object, title, body, metadata } => {
                if !objects.contains(object.as_str()) {
                    bail!("initializer updates object {object:?} before it is created");
                }
                if title.is_none() && body.is_none() && metadata.is_none() {
                    bail!("initializer update for {object:?} must change at least one field");
                }
                if title.as_deref().is_some_and(|title| title.trim().is_empty()) {
                    bail!("initializer update for {object:?} has an empty title");
                }
                if let Some(metadata) = metadata {
                    validate_metadata(metadata)?;
                }
            }
            SeedAction::CreateRelationship { source, target } => {
                if !objects.contains(source.as_str()) {
                    bail!("initializer relationship source {source:?} is not created yet");
                }
                if !objects.contains(target.as_str()) {
                    bail!("initializer relationship target {target:?} is not created yet");
                }
                if source == target {
                    bail!("initializer relationship cannot connect object {source:?} to itself");
                }
            }
            SeedAction::CreateCommentThread { key, object, body } => {
                if key.trim().is_empty() {
                    bail!("initializer commentary thread key must not be empty");
                }
                if !comment_threads.insert(key.as_str()) {
                    bail!("initializer declares duplicate commentary thread key {key:?}");
                }
                if !objects.contains(object.as_str()) {
                    bail!("initializer commentary object {object:?} is not created yet");
                }
                if body.trim().is_empty() {
                    bail!("initializer commentary thread {key:?} body must not be empty");
                }
            }
            SeedAction::ReplyToCommentThread { thread, body } => {
                if !comment_threads.contains(thread.as_str()) {
                    bail!("initializer replies to commentary thread {thread:?} before it is created");
                }
                if resolved_comment_threads.contains(thread.as_str()) {
                    bail!("initializer replies to resolved commentary thread {thread:?}");
                }
                if body.trim().is_empty() {
                    bail!("initializer commentary reply for {thread:?} must not be empty");
                }
            }
            SeedAction::ResolveCommentThread { thread } => {
                if !comment_threads.contains(thread.as_str()) {
                    bail!("initializer resolves commentary thread {thread:?} before it is created");
                }
                if !resolved_comment_threads.insert(thread.as_str()) {
                    bail!("initializer resolves commentary thread {thread:?} more than once");
                }
            }
        }
    }

    Ok(())
}

/// Validates initializer metadata against Kival's supported scalar metadata shape.
fn validate_metadata(metadata: &Value) -> Result<()> {
    let Some(metadata) = metadata.as_object() else {
        bail!("initializer metadata must be a JSON object");
    };
    for (key, value) in metadata {
        if !is_metadata_value(value) {
            bail!(
                "initializer metadata key {key:?} must be a JSON scalar or one-dimensional scalar array"
            );
        }
    }
    Ok(())
}

/// Returns whether a JSON value is valid as one Kival metadata value.
fn is_metadata_value(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => true,
        Value::Array(values) => values.iter().all(is_metadata_scalar),
        Value::Object(_) => false,
    }
}

/// Returns whether a JSON value is a scalar allowed inside a metadata array.
const fn is_metadata_scalar(value: &Value) -> bool {
    matches!(value, Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_))
}

/// Returns the default empty metadata object used by deserialization.
fn empty_metadata() -> Value {
    json!({})
}

#[cfg(test)]
mod tests {
    use super::{
        WorkspaceInitializer, WorkspaceInitializerKind, load_catalog, resolve_initializer,
        validate_catalog, workspace_initializers,
    };

    #[test]
    fn bundled_catalog_is_valid_and_contains_both_initializer_kinds() {
        let catalog = load_catalog().expect("catalog JSON should parse");
        validate_catalog(&catalog).expect("bundled catalog should validate");

        let initializers = workspace_initializers().expect("initializers should load");
        assert!(initializers.iter().any(|item| item.kind == WorkspaceInitializerKind::Template));
        assert!(initializers.iter().any(|item| item.kind == WorkspaceInitializerKind::Demo));
    }

    #[test]
    fn initializer_kind_mismatch_suggests_the_correct_flag() {
        let catalog = load_catalog().expect("catalog JSON should parse");

        let template = WorkspaceInitializer::template("acme-engineering");
        let error = resolve_initializer(&catalog, Some(&template))
            .err()
            .expect("demo selected as template should fail");
        assert_eq!(
            error.to_string(),
            "initializer \"acme-engineering\" is a demo scenario; use `--demo acme-engineering` instead of `--template`"
        );

        let demo = WorkspaceInitializer::demo("project");
        let error = resolve_initializer(&catalog, Some(&demo))
            .err()
            .expect("template selected as demo should fail");
        assert_eq!(
            error.to_string(),
            "initializer \"project\" is a reusable template; use `--template project` instead of `--demo`"
        );
    }

    #[sqlx::test(migrations = "../../crates/kernel/migrations")]
    async fn demo_initialization_uses_inert_users_without_demo_aware_core_state(
        pool: sqlx::PgPool,
    ) -> eyre::Result<()> {
        use kival_kernel::{
            create_user, grant_global_admin_as_operator, record_bootstrap_completed,
        };
        use uuid::Uuid;

        let mut tx = pool.begin().await?;
        let admin = create_user(&mut tx, "admin", "Admin").await?;
        grant_global_admin_as_operator(&mut tx, admin.id).await?;
        record_bootstrap_completed(&mut tx, admin.id, "admin", Uuid::now_v7()).await?;
        tx.commit().await?;

        let initializer = super::WorkspaceInitializer::demo("acme-engineering");
        let created =
            super::create_workspace_as_operator(&pool, "Kival Demo", None, Some(&initializer))
                .await?;

        let object_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM kival.objects WHERE workspace_id = $1",
        )
        .bind(created.workspace_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(object_count, 45);

        let membership_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM kival.workspace_memberships WHERE workspace_id = $1 AND revoked_at IS NULL",
        )
        .bind(created.workspace_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(membership_count, 8);

        let disabled_demo_actors = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(DISTINCT u.id)
            FROM kival.users u
            JOIN kival.workspace_memberships m ON m.user_id = u.id
            WHERE m.workspace_id = $1
              AND u.id <> $2
              AND u.disabled_at IS NOT NULL
            "#,
        )
        .bind(created.workspace_id)
        .bind(admin.id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(disabled_demo_actors, 7);

        let authored_actors = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(DISTINCT actor_user_id)
            FROM kival.events
            WHERE workspace_id = $1
              AND event_kind = 'object.created'
            "#,
        )
        .bind(created.workspace_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(authored_actors, 7);

        Ok(())
    }

    #[sqlx::test(migrations = "../../crates/kernel/migrations")]
    async fn acme_demo_seeds_owner_markers_commentary_and_sparse_graph(
        pool: sqlx::PgPool,
    ) -> eyre::Result<()> {
        use kival_kernel::{
            create_user, grant_global_admin_as_operator, record_bootstrap_completed,
        };
        use uuid::Uuid;

        let mut tx = pool.begin().await?;
        let admin = create_user(&mut tx, "admin", "Admin").await?;
        grant_global_admin_as_operator(&mut tx, admin.id).await?;
        record_bootstrap_completed(&mut tx, admin.id, "admin", Uuid::now_v7()).await?;
        tx.commit().await?;

        let initializer = WorkspaceInitializer::demo("acme-engineering");
        let created =
            super::create_workspace_as_operator(&pool, "ACME Demo", None, Some(&initializer))
                .await?;

        let object_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM kival.objects WHERE workspace_id = $1",
        )
        .bind(created.workspace_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(object_count, 45);

        let edge_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM kival.object_edges WHERE workspace_id = $1 AND revoked_at IS NULL",
        )
        .bind(created.workspace_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(edge_count, 46);

        let pin_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM kival.object_pins WHERE workspace_id = $1 AND user_id = $2",
        )
        .bind(created.workspace_id)
        .bind(admin.id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(pin_count, 4);

        let favorite_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM kival.object_favorites WHERE workspace_id = $1 AND user_id = $2",
        )
        .bind(created.workspace_id)
        .bind(admin.id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(favorite_count, 6);

        let thread_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM kival.comment_threads WHERE workspace_id = $1",
        )
        .bind(created.workspace_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(thread_count, 20);

        let comment_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM kival.comments WHERE workspace_id = $1",
        )
        .bind(created.workspace_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(comment_count, 42);

        let resolved_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM kival.comment_threads WHERE workspace_id = $1 AND resolved_at IS NOT NULL",
        )
        .bind(created.workspace_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(resolved_count, 6);

        let commented_object_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(DISTINCT object_id) FROM kival.comment_threads WHERE workspace_id = $1",
        )
        .bind(created.workspace_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(commented_object_count, 20);

        let comment_author_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(DISTINCT author_user_id) FROM kival.comments WHERE workspace_id = $1",
        )
        .bind(created.workspace_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(comment_author_count, 7);

        Ok(())
    }
}
