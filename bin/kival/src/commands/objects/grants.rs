//! Direct object-grant commands.

use clap::{ArgGroup, Parser, Subcommand};
use clap_schema::{CommandSchema, schema_handler};
use eyre::Result;
use kival_cli::runner::CliContext;
use kival_sdk::{CreateObjectGrantRequest, ListResponse, ObjectGrant, ObjectRole};
use uuid::Uuid;

use super::ObjectTargetArgs;
use crate::utils::{
    args::{CliObjectRole, DEFAULT_LIST_LIMIT_HELP, grant_principal, list_params},
    credentials::authenticated_client,
    output::{OutputMode, print_empty_list, print_output},
};

/// Arguments for `kival objects grants`.
#[derive(Debug, Parser, CommandSchema)]
pub struct ObjectGrantsCommand {
    /// The grant command to run.
    #[command(subcommand)]
    pub command: ObjectGrantsSubcommand,
}

/// The available `kival objects grants` commands.
#[derive(Debug, Subcommand, CommandSchema)]
pub enum ObjectGrantsSubcommand {
    /// List active direct object grants, newest first.
    #[command(name = "list")]
    List(ObjectGrantsListCommand),
    /// Grant a user or linked group a role on an object.
    ///
    /// A grant is direct object access and is distinct from workspace or group membership. Exactly
    /// one principal must be supplied with `--user-id` or `--group-id`.
    #[command(
        name = "create",
        after_help = "Examples:\n  kival objects grants create <WORKSPACE_ID> <OBJECT_ID> --user-id <USER_ID> --role viewer\n  kival objects grants create <WORKSPACE_ID> <OBJECT_ID> --group-id <GROUP_ID> --role editor"
    )]
    Create(ObjectGrantsCreateCommand),
    /// Revoke a direct object grant without deleting its historical record.
    #[command(name = "revoke")]
    Revoke(ObjectGrantsRevokeCommand),
}

/// Arguments for `kival objects grants list`.
#[derive(Debug, Parser)]
pub struct ObjectGrantsListCommand {
    /// Object target.
    #[command(flatten)]
    pub target: ObjectTargetArgs,
    /// Maximum number of grants to return.
    #[arg(long, value_name = "N", default_value = DEFAULT_LIST_LIMIT_HELP)]
    pub limit: Option<i64>,
    /// Opaque `response.next_cursor` from the previous page; reuse it with the same filters.
    #[arg(long, value_name = "CURSOR")]
    pub cursor: Option<String>,
}

/// Arguments for `kival objects grants create`.
#[derive(Debug, Clone, Copy, Parser)]
#[command(group(ArgGroup::new("principal").required(true).args(["user_id", "group_id"])))]
pub struct ObjectGrantsCreateCommand {
    /// Object target.
    #[command(flatten)]
    pub target: ObjectTargetArgs,
    /// User principal ID.
    #[arg(long, value_name = "USER_ID", conflicts_with = "group_id")]
    pub user_id: Option<Uuid>,
    /// Group principal ID.
    #[arg(long, value_name = "GROUP_ID")]
    pub group_id: Option<Uuid>,
    /// Object role: viewer, editor, or admin.
    #[arg(long, value_name = "ROLE", value_enum)]
    pub role: CliObjectRole,
}

/// Arguments for `kival objects grants revoke`.
#[derive(Debug, Clone, Copy, Parser)]
pub struct ObjectGrantsRevokeCommand {
    /// Object target.
    #[command(flatten)]
    pub target: ObjectTargetArgs,
    /// Grant ID.
    #[arg(value_name = "GRANT_ID")]
    pub grant_id: Uuid,
}

impl ObjectGrantsCommand {
    /// Run `kival objects grants`.
    ///
    /// # Errors
    ///
    /// Returns an error if the selected grant command fails.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<()> {
        match self.command {
            ObjectGrantsSubcommand::List(command) => {
                command.run(ctx, output).await?;
            }
            ObjectGrantsSubcommand::Create(command) => {
                command.run(ctx, output).await?;
            }
            ObjectGrantsSubcommand::Revoke(command) => {
                command.run(ctx, output).await?;
            }
        }
        Ok(())
    }
}

#[schema_handler(run)]
impl ObjectGrantsListCommand {
    /// Run `kival objects grants list`.
    ///
    /// # Errors
    ///
    /// Returns an error if grants cannot be listed.
    pub async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> Result<ListResponse<ObjectGrant>> {
        let client = authenticated_client(&ctx)?;
        let response = client
            .list_object_grants(
                self.target.workspace_id,
                self.target.object_id,
                &list_params(self.limit, self.cursor),
            )
            .await?;
        print_output(output, &response, || {
            if response.items.is_empty() {
                print_empty_list("grants");
            } else {
                for grant in &response.items {
                    print_grant_line(grant, None);
                }
            }
            if let Some(cursor) = &response.next_cursor {
                println!("\nNext cursor: {cursor}");
            }
        })?;
        Ok(response)
    }
}

#[schema_handler(run)]
impl ObjectGrantsCreateCommand {
    /// Run `kival objects grants create`.
    ///
    /// # Errors
    ///
    /// Returns an error if the grant cannot be created.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<ObjectGrant> {
        let principal = grant_principal(self.user_id, self.group_id)?;
        let role = ObjectRole::from(self.role);
        let client = authenticated_client(&ctx)?;
        let grant = client
            .create_object_grant(
                self.target.workspace_id,
                self.target.object_id,
                CreateObjectGrantRequest { principal, object_role: role },
            )
            .await?;
        print_output(output, &grant, || print_grant_line(&grant, Some("created")))?;
        Ok(grant)
    }
}

#[schema_handler(run)]
impl ObjectGrantsRevokeCommand {
    /// Run `kival objects grants revoke`.
    ///
    /// # Errors
    ///
    /// Returns an error if the grant cannot be revoked.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<ObjectGrant> {
        let client = authenticated_client(&ctx)?;
        let grant = client
            .revoke_object_grant(self.target.workspace_id, self.target.object_id, self.grant_id)
            .await?;
        print_output(output, &grant, || print_grant_line(&grant, Some("revoked")))?;
        Ok(grant)
    }
}

/// Prints an object grant as a compact human-readable line, optionally including an action.
fn print_grant_line(grant: &ObjectGrant, action: Option<&str>) {
    let mut fields = vec![grant.id.to_string()];
    if let Some(action) = action {
        fields.push(format!("action={action}"));
    }
    fields.push(format!("object={}", grant.object_id));
    if let Some(user_id) = grant.principal_user_id {
        fields.push(format!("user={user_id}"));
    } else {
        fields.push(format!(
            "group={}",
            grant.principal_group_id.expect("grant must have a principal")
        ));
    }
    fields.push(format!("role={}", grant.object_role));
    println!("{}", fields.join(" "));
}
