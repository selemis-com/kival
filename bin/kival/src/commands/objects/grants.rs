//! Direct object-grant commands.

use argx::{Args, Subcommand, argx};
use eyre::Result;
use kival_cli::runner::CliContext;
use kival_sdk::{
    CreateObjectGrantRequest, ListResponse, ObjectGrant, ObjectRole, UpdateObjectGrantRequest,
};
use uuid::Uuid;

use super::{ObjectCommandError, ObjectTargetArgs, object_error_codes};
use crate::utils::{
    args::{CliObjectRole, DEFAULT_LIST_LIMIT, grant_principal, list_params},
    credentials::authenticated_client,
    error::erase_command_error,
    output::{OutputMode, print_empty_list, print_output},
};

object_error_codes! {
    pub(crate) enum ObjectGrantListErrorCode {
        failures {
            AuthenticationRequired => ("authentication.required", AuthenticationRequired),
            PermissionDenied => ("permission.denied", PermissionDenied),
            InvalidArgument => ("invalid.argument", InvalidArgument),
            ResourceNotFound => ("resource.not_found", ResourceNotFound),
            ServerUnavailable => ("server.unavailable", ServerUnavailable),
            RateLimited => ("rate_limited", RateLimited),
            RequestFailed => ("request.failed", RequestFailed),
            InvalidField => ("output.invalid_field", InvalidField),
            InvalidProjection => ("output.invalid_projection", InvalidProjection),
            InvalidCursor => ("invalid.cursor", InvalidCursor),
            Internal => ("internal", Internal),
        }
        objects { ObjectNotFound => ("object.not_found", NotFound) }
    }
}

/// Error returned by the corresponding command handler.
type ObjectGrantListError = ObjectCommandError<ObjectGrantListErrorCode>;

object_error_codes! {
    pub(crate) enum ObjectGrantMutationErrorCode {
        failures {
            AuthenticationRequired => ("authentication.required", AuthenticationRequired),
            PermissionDenied => ("permission.denied", PermissionDenied),
            InvalidArgument => ("invalid.argument", InvalidArgument),
            ResourceNotFound => ("resource.not_found", ResourceNotFound),
            ResourceConflict => ("resource.conflict", ResourceConflict),
            ServerUnavailable => ("server.unavailable", ServerUnavailable),
            RateLimited => ("rate_limited", RateLimited),
            RequestFailed => ("request.failed", RequestFailed),
            InvalidField => ("output.invalid_field", InvalidField),
            InvalidProjection => ("output.invalid_projection", InvalidProjection),
            Internal => ("internal", Internal),
        }
        objects {
            ObjectNotFound => ("object.not_found", NotFound),
        }
    }
}

/// Error returned by the corresponding command handler.
type ObjectGrantMutationError = ObjectCommandError<ObjectGrantMutationErrorCode>;

/// Arguments for `kival objects grants`.
#[derive(Debug, Args)]
#[argx(schema)]
pub struct ObjectGrantsCommand {
    /// The grant command to run.
    #[argx(subcommand)]
    pub command: ObjectGrantsSubcommand,
}

/// The available `kival objects grants` commands.
#[derive(Debug, Subcommand)]
#[argx(schema)]
pub enum ObjectGrantsSubcommand {
    /// List active direct object grants, newest first.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
            "requiredScopes": ["access:manage"],
        })
    )]
    List(ObjectGrantsListCommand),
    /// Grant a user or linked group a role on an object.
    ///
    /// A grant is direct object access and is distinct from workspace or group membership. Exactly
    /// one principal must be supplied with `--user-id` or `--group-id`.
    ///
    /// Examples: grant a user with `--user-id <USER_ID> --role viewer`, or a linked group with
    /// `--group-id <GROUP_ID> --role editor`.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": false,
            "idempotent": false,
            "requiredScopes": ["access:manage"],
        })
    )]
    Create(ObjectGrantsCreateCommand),
    /// Change an active direct object grant's role.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": false,
            "idempotent": false,
            "requiredScopes": ["access:manage"],
        })
    )]
    Update(ObjectGrantsUpdateCommand),
    /// Revoke a direct object grant without deleting its historical record.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": true,
            "idempotent": false,
            "requiresConfirmation": true,
            "requiredScopes": ["access:manage"],
        })
    )]
    Revoke(ObjectGrantsRevokeCommand),
}

/// Arguments for `kival objects grants list`.
#[derive(Debug, Args)]
pub struct ObjectGrantsListCommand {
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
    /// Maximum number of grants to return.
    #[argx(long, default = DEFAULT_LIST_LIMIT)]
    pub limit: Option<i64>,
    /// Opaque `response.next_cursor` from the previous page; reuse it with the same filters.
    #[argx(long)]
    pub cursor: Option<String>,
}

/// Arguments for `kival objects grants create`.
#[derive(Debug, Clone, Copy, Args)]
#[argx(one_of = ["user_id", "group_id"])]
pub struct ObjectGrantsCreateCommand {
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
    /// User principal ID.
    #[argx(long)]
    pub user_id: Option<Uuid>,
    /// Group principal ID.
    #[argx(long)]
    pub group_id: Option<Uuid>,
    /// Object role: viewer, editor, or admin.
    #[argx(long, value_enum)]
    pub role: CliObjectRole,
}

/// Arguments for `kival objects grants update`.
#[derive(Debug, Clone, Copy, Args)]
pub struct ObjectGrantsUpdateCommand {
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
    /// Grant ID.
    pub grant_id: Uuid,
    /// New object role: viewer, editor, or admin.
    #[argx(long, value_enum)]
    pub role: CliObjectRole,
}

/// Arguments for `kival objects grants revoke`.
#[derive(Debug, Clone, Copy, Args)]
pub struct ObjectGrantsRevokeCommand {
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
    /// Grant ID.
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
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
            ObjectGrantsSubcommand::Create(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
            ObjectGrantsSubcommand::Update(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
            ObjectGrantsSubcommand::Revoke(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
        }
        Ok(())
    }
}

#[argx(handler = run)]
impl ObjectGrantsListCommand {
    /// Run `kival objects grants list`.
    ///
    /// # Errors
    ///
    /// Returns an error if grants cannot be listed.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ListResponse<ObjectGrant>, ObjectGrantListError> {
        let client = authenticated_client(&ctx)?;
        let response = client
            .list_object_grants(
                self.target.workspace_id,
                self.target.object_id,
                &list_params(self.limit, self.cursor),
            )
            .await?;
        print_output(&output, &response, || {
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

#[argx(handler = run)]
impl ObjectGrantsCreateCommand {
    /// Run `kival objects grants create`.
    ///
    /// # Errors
    ///
    /// Returns an error if the grant cannot be created.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ObjectGrant, ObjectGrantMutationError> {
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
        print_output(&output, &grant, || print_grant_line(&grant, Some("created")))?;
        Ok(grant)
    }
}

#[argx(handler = run)]
impl ObjectGrantsUpdateCommand {
    /// Run `kival objects grants update`.
    ///
    /// # Errors
    ///
    /// Returns an error if the active grant role cannot be updated.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ObjectGrant, ObjectGrantMutationError> {
        let client = authenticated_client(&ctx)?;
        let grant = client
            .update_object_grant(
                self.target.workspace_id,
                self.target.object_id,
                self.grant_id,
                UpdateObjectGrantRequest { object_role: self.role.into() },
            )
            .await?;
        print_output(&output, &grant, || print_grant_line(&grant, Some("updated")))?;
        Ok(grant)
    }
}

#[argx(handler = run)]
impl ObjectGrantsRevokeCommand {
    /// Run `kival objects grants revoke`.
    ///
    /// # Errors
    ///
    /// Returns an error if the grant cannot be revoked.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ObjectGrant, ObjectGrantMutationError> {
        let client = authenticated_client(&ctx)?;
        let grant = client
            .revoke_object_grant(self.target.workspace_id, self.target.object_id, self.grant_id)
            .await?;
        print_output(&output, &grant, || print_grant_line(&grant, Some("revoked")))?;
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
