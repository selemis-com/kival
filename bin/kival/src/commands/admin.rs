//! Admin commands.

use argx::{Args, Subcommand, argx};
use eyre::Result;
use kival_cli::runner::CliContext;
use kival_sdk::{ListResponse, UpdateUserRequest, User, UserListParams, UserListStatus};
use serde::Deserialize;
use uuid::Uuid;

use crate::utils::{
    args::DEFAULT_LIST_LIMIT,
    credentials::authenticated_client,
    error::{CommandError, command_error_codes, erase_command_error},
    input::{
        StructuredInputArgs, at_least_one_input_field, deserialize_optional_non_null,
        read_json_input, reject_conflicting_input,
    },
    output::{OutputMode, format_human_timestamp, print_output, quote_human_string},
};

command_error_codes! {
    pub(crate) enum AdminListErrorCode {
        AuthenticationRequired => ("authentication.required", AuthenticationRequired),
        PermissionDenied => ("permission.denied", PermissionDenied),
        InvalidArgument => ("invalid.argument", InvalidArgument),
        ServerUnavailable => ("server.unavailable", ServerUnavailable),
            RateLimited => ("rate_limited", RateLimited),
        RequestFailed => ("request.failed", RequestFailed),
        Internal => ("internal", Internal),
        InvalidField => ("output.invalid_field", InvalidField),
        InvalidProjection => ("output.invalid_projection", InvalidProjection),
        InvalidCursor => ("invalid.cursor", InvalidCursor),
    }
}

/// Error returned by the corresponding command handler.
type AdminListError = CommandError<AdminListErrorCode>;

command_error_codes! {
    pub(crate) enum AdminGetErrorCode {
        AuthenticationRequired => ("authentication.required", AuthenticationRequired),
        PermissionDenied => ("permission.denied", PermissionDenied),
        InvalidArgument => ("invalid.argument", InvalidArgument),
        ServerUnavailable => ("server.unavailable", ServerUnavailable),
            RateLimited => ("rate_limited", RateLimited),
        RequestFailed => ("request.failed", RequestFailed),
        Internal => ("internal", Internal),
        InvalidField => ("output.invalid_field", InvalidField),
        InvalidProjection => ("output.invalid_projection", InvalidProjection),
        ResourceNotFound => ("resource.not_found", ResourceNotFound),
    }
}

/// Error returned by the corresponding command handler.
type AdminGetError = CommandError<AdminGetErrorCode>;

command_error_codes! {
    pub(crate) enum AdminUpdateErrorCode {
        AuthenticationRequired => ("authentication.required", AuthenticationRequired),
        PermissionDenied => ("permission.denied", PermissionDenied),
        InvalidArgument => ("invalid.argument", InvalidArgument),
        ServerUnavailable => ("server.unavailable", ServerUnavailable),
            RateLimited => ("rate_limited", RateLimited),
        RequestFailed => ("request.failed", RequestFailed),
        Internal => ("internal", Internal),
        InvalidField => ("output.invalid_field", InvalidField),
        InvalidProjection => ("output.invalid_projection", InvalidProjection),
        ResourceNotFound => ("resource.not_found", ResourceNotFound),
            ResourceConflict => ("resource.conflict", ResourceConflict),
        InputReadFailed => ("input.read_failed", InputReadFailed),
        InputInvalidJson => ("input.invalid_json", InputInvalidJson),
        InputInvalidValue => ("input.invalid_value", InputInvalidValue),
        InputConflictingSources => ("input.conflicting_sources", InputConflictingSources),
    }
}

/// Error returned by the corresponding command handler.
type AdminUpdateError = CommandError<AdminUpdateErrorCode>;

command_error_codes! {
    pub(crate) enum AdminMutationErrorCode {
        AuthenticationRequired => ("authentication.required", AuthenticationRequired),
        PermissionDenied => ("permission.denied", PermissionDenied),
        InvalidArgument => ("invalid.argument", InvalidArgument),
        ServerUnavailable => ("server.unavailable", ServerUnavailable),
            RateLimited => ("rate_limited", RateLimited),
        RequestFailed => ("request.failed", RequestFailed),
        Internal => ("internal", Internal),
        InvalidField => ("output.invalid_field", InvalidField),
        InvalidProjection => ("output.invalid_projection", InvalidProjection),
        ResourceNotFound => ("resource.not_found", ResourceNotFound),
            ResourceConflict => ("resource.conflict", ResourceConflict),
    }
}

/// Error returned by the corresponding command handler.
type AdminMutationError = CommandError<AdminMutationErrorCode>;

/// Arguments for `kival admin`.
#[derive(Debug, Args)]
#[argx(schema)]
pub struct AdminCommand {
    /// The admin command to run.
    #[argx(subcommand)]
    pub command: AdminSubcommand,
}

/// The available `kival admin` commands.
#[derive(Debug, Subcommand)]
#[argx(schema)]
pub enum AdminSubcommand {
    /// Manage users.
    Users(AdminUsersCommand),
}

/// Arguments for `kival admin users`.
#[derive(Debug, Args)]
#[argx(schema)]
pub struct AdminUsersCommand {
    /// The user command to run.
    #[argx(subcommand)]
    pub command: AdminUsersSubcommand,
}

/// The available `kival admin users` commands.
#[derive(Debug, Subcommand)]
#[argx(schema)]
pub enum AdminUsersSubcommand {
    /// List users.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
            "requiredScopes": ["admin"],
        })
    )]
    List(AdminUsersListCommand),

    /// Get a user by ID.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
            "requiredScopes": ["admin"],
        })
    )]
    Get(AdminUsersGetCommand),

    /// Update a user.
    ///
    /// With `--input`, the JSON object requires `display_name` (string); unknown properties are
    /// rejected.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": false,
            "idempotent": false,
            "requiredScopes": ["admin"],
        })
    )]
    Update(AdminUsersUpdateCommand),

    /// Disable a user.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": true,
            "idempotent": false,
            "requiresConfirmation": true,
            "requiredScopes": ["admin"],
        })
    )]
    Disable(AdminUsersDisableCommand),

    /// Enable a disabled user.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": false,
            "idempotent": false,
            "requiredScopes": ["admin"],
        })
    )]
    Enable(AdminUsersEnableCommand),
}

/// Arguments for `kival admin users list`.
#[derive(Debug, Args)]
pub struct AdminUsersListCommand {
    /// Show disabled users only.
    #[argx(long, conflicts = "all")]
    pub disabled: bool,

    /// Show active and disabled users.
    #[argx(long)]
    pub all: bool,

    /// Case-insensitive username or display-name search.
    #[argx(long)]
    pub query: Option<String>,

    /// Maximum number of users to return.
    #[argx(long, default = DEFAULT_LIST_LIMIT)]
    pub limit: Option<i64>,

    /// Opaque `response.next_cursor` from the previous page; reuse it with the same filters.
    #[argx(long)]
    pub cursor: Option<String>,
}

/// Arguments for `kival admin users get`.
#[derive(Debug, Clone, Copy, Args)]
pub struct AdminUsersGetCommand {
    /// User ID.
    pub user_id: Uuid,
}

/// Semantic input for updating a user.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateUserInput {
    /// New display name.
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    pub display_name: Option<String>,
}

/// Arguments for `kival admin users update`.
#[derive(Debug, Args)]
#[argx(one_of = ["input", "display_name"])]
pub struct AdminUsersUpdateCommand {
    /// Structured input source.
    #[argx(flatten)]
    pub input_source: StructuredInputArgs,
    /// User ID.
    pub user_id: Uuid,

    /// New display name.
    #[argx(long)]
    pub display_name: Option<String>,
}

/// Arguments for `kival admin users disable`.
#[derive(Debug, Clone, Copy, Args)]
pub struct AdminUsersDisableCommand {
    /// User ID.
    pub user_id: Uuid,
}

/// Arguments for `kival admin users enable`.
#[derive(Debug, Clone, Copy, Args)]
pub struct AdminUsersEnableCommand {
    /// User ID.
    pub user_id: Uuid,
}

impl AdminCommand {
    /// Run `kival admin`.
    ///
    /// # Errors
    ///
    /// Returns an error if the selected admin command fails.
    pub(crate) async fn run(self, ctx: CliContext, output: OutputMode) -> Result<()> {
        match self.command {
            AdminSubcommand::Users(command) => command.run(ctx, output).await,
        }
    }
}

impl AdminUsersCommand {
    /// Run `kival admin users`.
    ///
    /// # Errors
    ///
    /// Returns an error if the selected user command fails.
    pub(crate) async fn run(self, ctx: CliContext, output: OutputMode) -> Result<()> {
        match self.command {
            AdminUsersSubcommand::List(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
            AdminUsersSubcommand::Get(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
            AdminUsersSubcommand::Update(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
            AdminUsersSubcommand::Disable(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
            AdminUsersSubcommand::Enable(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
        }
        Ok(())
    }
}

#[argx(handler = run)]
impl AdminUsersListCommand {
    /// Run `kival admin users list`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key cannot be loaded or users cannot be listed.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ListResponse<User>, AdminListError> {
        let client = authenticated_client(&ctx)?;

        let response = client
            .list_users(&UserListParams {
                limit: self.limit,
                cursor: self.cursor,
                status: if self.all {
                    UserListStatus::All
                } else if self.disabled {
                    UserListStatus::Disabled
                } else {
                    UserListStatus::Active
                },
                q: self.query,
            })
            .await?;

        print_output(&output, &response, || {
            for user in &response.items {
                print_user_line(user, None);
            }

            if let Some(cursor) = &response.next_cursor {
                println!();
                println!("Next cursor: {cursor}");
            }
        })?;
        Ok(response)
    }
}

#[argx(handler = run)]
impl AdminUsersGetCommand {
    /// Run `kival admin users get`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key cannot be loaded or the user cannot be fetched.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<User, AdminGetError> {
        let client = authenticated_client(&ctx)?;
        let user = client.get_user(self.user_id).await?;

        print_output(&output, &user, || print_user_line(&user, None))?;
        Ok(user)
    }
}

#[argx(handler = run)]
impl AdminUsersUpdateCommand {
    /// Run `kival admin users update`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key cannot be loaded or the user cannot be updated.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<User, AdminUpdateError> {
        let user_id = self.user_id;
        let input = self.into_input()?;
        let display_name = input.display_name.as_deref().map(str::trim);

        if display_name.is_none() {
            return Err(AdminUpdateError::invalid_argument("at least one field must be provided"));
        }

        if matches!(display_name, Some("")) {
            return Err(AdminUpdateError::invalid_argument("display name must not be empty"));
        }

        let client = authenticated_client(&ctx)?;
        let user = client
            .update_user(
                user_id,
                UpdateUserRequest { display_name: display_name.map(ToOwned::to_owned) },
            )
            .await?;

        print_output(&output, &user, || print_user_line(&user, Some("updated")))?;
        Ok(user)
    }

    /// Resolves semantic update input from either `--input` or CLI payload fields.
    fn into_input(self) -> Result<UpdateUserInput> {
        reject_conflicting_input(
            &self.input_source.input,
            &[("display_name", self.display_name.is_some())],
        )?;

        if let Some(input) = self.input_source.input {
            let input: UpdateUserInput = read_json_input(input)?;
            if input.display_name.is_none() {
                return Err(AdminUpdateError::input_invalid_value(at_least_one_input_field(&[
                    "display_name",
                ]))
                .into());
            }
            return Ok(input);
        }

        Ok(UpdateUserInput { display_name: self.display_name })
    }
}

#[argx(handler = run)]
impl AdminUsersDisableCommand {
    /// Run `kival admin users disable`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key cannot be loaded or the user cannot be disabled.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<User, AdminMutationError> {
        let client = authenticated_client(&ctx)?;
        let user = client.disable_user(self.user_id).await?;

        print_output(&output, &user, || print_user_line(&user, Some("disabled")))?;
        Ok(user)
    }
}

#[argx(handler = run)]
impl AdminUsersEnableCommand {
    /// Run `kival admin users enable`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key cannot be loaded or the user cannot be enabled.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<User, AdminMutationError> {
        let client = authenticated_client(&ctx)?;
        let user = client.enable_user(self.user_id).await?;

        print_output(&output, &user, || print_user_line(&user, Some("enabled")))?;
        Ok(user)
    }
}

/// Prints a compact user line.
fn print_user_line(user: &User, action: Option<&str>) {
    let mut fields = vec![user.id.to_string()];
    if let Some(action) = action {
        fields.push(format!("action={action}"));
    }
    fields.extend([
        format!("created={}", format_human_timestamp(user.created_at)),
        format!("updated={}", format_human_timestamp(user.updated_at)),
        format!("status={}", user.status),
        format!("username={}", quote_human_string(&user.username)),
        format!("display_name={}", quote_human_string(&user.display_name)),
    ]);
    println!("{}", fields.join(" "));
}
