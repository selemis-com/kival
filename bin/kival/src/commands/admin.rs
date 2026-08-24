//! Admin commands.

use clap::{Parser, Subcommand};
use clap_schema::{CommandSchema, schema_handler};
use eyre::Result;
use kival_cli::runner::CliContext;
use kival_sdk::{ListResponse, UpdateUserRequest, User, UserListParams, UserListStatus};
use serde::Deserialize;
use uuid::Uuid;

use crate::utils::{
    args::DEFAULT_LIST_LIMIT_HELP,
    credentials::authenticated_client,
    error::CliError,
    input::{
        StructuredInputArgs, at_least_one_input_field, deserialize_optional_non_null,
        read_json_input, reject_conflicting_input,
    },
    output::{OutputMode, format_human_timestamp, print_output, quote_human_string},
};

/// Arguments for `kival admin`.
#[derive(Debug, Parser, CommandSchema)]
pub struct AdminCommand {
    /// The admin command to run.
    #[command(subcommand)]
    pub command: AdminSubcommand,
}

/// The available `kival admin` commands.
#[derive(Debug, Subcommand, CommandSchema)]
pub enum AdminSubcommand {
    /// Manage users.
    #[command(name = "users")]
    Users(AdminUsersCommand),
}

/// Arguments for `kival admin users`.
#[derive(Debug, Parser, CommandSchema)]
pub struct AdminUsersCommand {
    /// The user command to run.
    #[command(subcommand)]
    pub command: AdminUsersSubcommand,
}

/// The available `kival admin users` commands.
#[derive(Debug, Subcommand, CommandSchema)]
pub enum AdminUsersSubcommand {
    /// List users.
    #[command(name = "list")]
    List(AdminUsersListCommand),

    /// Get a user by ID.
    #[command(name = "get")]
    Get(AdminUsersGetCommand),

    /// Update a user.
    #[command(name = "update")]
    Update(AdminUsersUpdateCommand),

    /// Disable a user.
    #[command(name = "disable")]
    Disable(AdminUsersDisableCommand),

    /// Enable a disabled user.
    #[command(name = "enable")]
    Enable(AdminUsersEnableCommand),
}

/// Arguments for `kival admin users list`.
#[derive(Debug, Parser)]
pub struct AdminUsersListCommand {
    /// Show disabled users only.
    #[arg(long, conflicts_with = "all")]
    pub disabled: bool,

    /// Show active and disabled users.
    #[arg(long)]
    pub all: bool,

    /// Case-insensitive username or display-name search.
    #[arg(long, value_name = "QUERY")]
    pub query: Option<String>,

    /// Maximum number of users to return.
    #[arg(long, value_name = "N", default_value = DEFAULT_LIST_LIMIT_HELP)]
    pub limit: Option<i64>,

    /// Opaque `response.next_cursor` from the previous page; reuse it with the same filters.
    #[arg(long, value_name = "CURSOR")]
    pub cursor: Option<String>,
}

/// Arguments for `kival admin users get`.
#[derive(Debug, Clone, Copy, Parser)]
pub struct AdminUsersGetCommand {
    /// User ID.
    #[arg(value_name = "USER_ID")]
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
#[derive(Debug, Parser)]
pub struct AdminUsersUpdateCommand {
    /// Structured input source.
    #[command(flatten)]
    pub input_source: StructuredInputArgs,
    /// User ID.
    #[arg(value_name = "USER_ID")]
    pub user_id: Uuid,

    /// New display name.
    #[arg(long, value_name = "NAME")]
    pub display_name: Option<String>,
}

/// Arguments for `kival admin users disable`.
#[derive(Debug, Clone, Copy, Parser)]
pub struct AdminUsersDisableCommand {
    /// User ID.
    #[arg(value_name = "USER_ID")]
    pub user_id: Uuid,
}

/// Arguments for `kival admin users enable`.
#[derive(Debug, Clone, Copy, Parser)]
pub struct AdminUsersEnableCommand {
    /// User ID.
    #[arg(value_name = "USER_ID")]
    pub user_id: Uuid,
}

impl AdminCommand {
    /// Run `kival admin`.
    ///
    /// # Errors
    ///
    /// Returns an error if the selected admin command fails.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<()> {
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
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<()> {
        match self.command {
            AdminUsersSubcommand::List(command) => {
                command.run(ctx, output).await?;
            }
            AdminUsersSubcommand::Get(command) => {
                command.run(ctx, output).await?;
            }
            AdminUsersSubcommand::Update(command) => {
                command.run(ctx, output).await?;
            }
            AdminUsersSubcommand::Disable(command) => {
                command.run(ctx, output).await?;
            }
            AdminUsersSubcommand::Enable(command) => {
                command.run(ctx, output).await?;
            }
        }
        Ok(())
    }
}

#[schema_handler(run)]
impl AdminUsersListCommand {
    /// Run `kival admin users list`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key cannot be loaded or users cannot be listed.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<ListResponse<User>> {
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

        print_output(output, &response, || {
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

#[schema_handler(run)]
impl AdminUsersGetCommand {
    /// Run `kival admin users get`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key cannot be loaded or the user cannot be fetched.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<User> {
        let client = authenticated_client(&ctx)?;
        let user = client.get_user(self.user_id).await?;

        print_output(output, &user, || print_user_line(&user, None))?;
        Ok(user)
    }
}

#[schema_handler(run)]
impl AdminUsersUpdateCommand {
    /// Run `kival admin users update`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key cannot be loaded or the user cannot be updated.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<User> {
        let user_id = self.user_id;
        let input = self.into_input()?;
        let display_name = input.display_name.as_deref().map(str::trim);

        if display_name.is_none() {
            return Err(CliError::invalid_argument("at least one field must be provided").into());
        }

        if matches!(display_name, Some("")) {
            return Err(CliError::invalid_argument("display name must not be empty").into());
        }

        let client = authenticated_client(&ctx)?;
        let user = client
            .update_user(
                user_id,
                UpdateUserRequest { display_name: display_name.map(ToOwned::to_owned) },
            )
            .await?;

        print_output(output, &user, || print_user_line(&user, Some("updated")))?;
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
                return Err(CliError::input_invalid_value(at_least_one_input_field(&[
                    "display_name",
                ]))
                .into());
            }
            return Ok(input);
        }

        Ok(UpdateUserInput { display_name: self.display_name })
    }
}

#[schema_handler(run)]
impl AdminUsersDisableCommand {
    /// Run `kival admin users disable`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key cannot be loaded or the user cannot be disabled.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<User> {
        let client = authenticated_client(&ctx)?;
        let user = client.disable_user(self.user_id).await?;

        print_output(output, &user, || print_user_line(&user, Some("disabled")))?;
        Ok(user)
    }
}

#[schema_handler(run)]
impl AdminUsersEnableCommand {
    /// Run `kival admin users enable`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key cannot be loaded or the user cannot be enabled.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<User> {
        let client = authenticated_client(&ctx)?;
        let user = client.enable_user(self.user_id).await?;

        print_output(output, &user, || print_user_line(&user, Some("enabled")))?;
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
