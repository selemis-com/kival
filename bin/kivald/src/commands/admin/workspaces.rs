//! Deployment-operator workspace creation and one-shot initialization.

mod initialization;

use argx::{Args, Subcommand};
use eyre::{Context, Result, bail};
use kival_tracing::info;
use serde::Serialize;
use sqlx::PgPool;

use self::initialization::{
    WorkspaceInitializer, create_workspace_as_operator, workspace_initializers,
};

/// Arguments for `kivald admin workspaces`.
#[derive(Debug, Args, Serialize)]
pub(crate) struct AdminWorkspacesCommand {
    /// Workspace-administration operation to run.
    #[argx(subcommand)]
    pub command: AdminWorkspacesSubcommand,
}

/// Deployment-operator workspace operations.
#[derive(Debug, Subcommand, Serialize)]
pub(crate) enum AdminWorkspacesSubcommand {
    /// List bundled reusable templates and demo scenarios.
    Initializers,

    /// Create a workspace, optionally initialized once from a bundled recipe.
    Create(AdminWorkspaceCreateCommand),
}

/// Arguments for `kivald admin workspaces create`.
#[derive(Debug, Args, Serialize)]
pub(crate) struct AdminWorkspaceCreateCommand {
    /// Name for the new workspace.
    #[argx(long)]
    pub name: String,

    /// Optional workspace description.
    #[argx(long)]
    pub description: Option<String>,

    /// Initialize the new workspace from a reusable template ID.
    #[argx(long, conflicts = "demo")]
    pub template: Option<String>,

    /// Initialize the new workspace from a disposable demo-scenario ID.
    #[argx(long, conflicts = "template")]
    pub demo: Option<String>,
}

impl AdminWorkspacesCommand {
    /// Runs the selected deployment-operator workspace operation.
    pub(crate) async fn run(&self, db_pool: PgPool) -> Result<()> {
        match &self.command {
            AdminWorkspacesSubcommand::Initializers => list_initializers(),
            AdminWorkspacesSubcommand::Create(command) => command.run(db_pool).await,
        }
    }
}

/// Prints the bundled initializer catalog for deployment operators.
fn list_initializers() -> Result<()> {
    for initializer in workspace_initializers().wrap_err("failed to load workspace initializers")? {
        match initializer.description {
            Some(description) => println!(
                "{}\t{}\t{}\t{}",
                initializer.kind, initializer.id, initializer.name, description
            ),
            None => println!("{}\t{}\t{}", initializer.kind, initializer.id, initializer.name),
        }
    }

    Ok(())
}

impl AdminWorkspaceCreateCommand {
    /// Creates one empty, template-initialized, or demo-initialized workspace.
    async fn run(&self, db_pool: PgPool) -> Result<()> {
        let initializer = match (&self.template, &self.demo) {
            (Some(template), None) => {
                let template = template.trim();
                if template.is_empty() {
                    bail!("template ID must not be empty");
                }
                Some(WorkspaceInitializer::template(template))
            }
            (None, Some(demo)) => {
                let demo = demo.trim();
                if demo.is_empty() {
                    bail!("demo scenario ID must not be empty");
                }
                Some(WorkspaceInitializer::demo(demo))
            }
            (None, None) => None,
            (Some(_), Some(_)) => bail!("template and demo initializers are mutually exclusive"),
        };

        let created = create_workspace_as_operator(
            &db_pool,
            &self.name,
            self.description.as_deref(),
            initializer.as_ref(),
        )
        .await?;

        info!(
            target: "kival::cli",
            workspace_id = %created.workspace_id,
            owner_user_id = %created.owner_user_id,
            owner_username = %created.owner_username,
            initializer = ?initializer,
            "Created Kival workspace",
        );
        println!("Created workspace {} for {}.", created.workspace_id, created.owner_username);

        Ok(())
    }
}
