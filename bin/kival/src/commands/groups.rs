//! Group commands.

use clap::{Parser, Subcommand};
use clap_schema::{CommandSchema, schema_handler};
use eyre::Result;
use kival_cli::runner::CliContext;
use kival_sdk::{
    CreateGroupMembershipRequest, CreateGroupRequest, Group, GroupListParams, GroupMembership,
    ListResponse, MembershipRole, PatchField, UpdateGroupRequest,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::utils::{
    args::{
        CliArchiveListStatus, CliMembershipRole, DEFAULT_LIST_LIMIT, DEFAULT_LIST_LIMIT_HELP,
        list_params,
    },
    credentials::authenticated_client,
    error::CliError,
    input::{
        StructuredInputArgs, at_least_one_input_field, deserialize_optional_non_null,
        deserialize_optional_nullable, read_json_input, reject_conflicting_input,
    },
    output::{
        OutputMode, format_human_timestamp, print_empty_list, print_output, quote_human_string,
    },
};

/// Arguments for `kival groups`.
#[derive(Debug, Parser, CommandSchema)]
pub struct GroupsCommand {
    /// The group command to run.
    #[command(subcommand)]
    pub command: GroupsSubcommand,
}

/// The available `kival groups` commands.
#[derive(Debug, Subcommand, CommandSchema)]
pub enum GroupsSubcommand {
    /// List groups, newest first.
    ///
    /// Active groups are returned by default. Use `--status` to select archived groups or both
    /// lifecycle states.
    #[command(name = "list")]
    List(GroupsListCommand),
    /// Get a group by ID.
    #[command(name = "get")]
    Get(GroupsGetCommand),
    /// Create a group.
    #[command(
        name = "create",
        after_help = "Examples:\n  kival groups create --name \"Editors\"\n  kival groups create --input group.json\n  cat group.json | kival groups create --input -"
    )]
    Create(GroupsCreateCommand),
    /// Update a group.
    #[command(
        name = "update",
        after_help = "Examples:\n  kival groups update <GROUP_ID> --name \"Reviewers\"\n  kival groups update <GROUP_ID> --clear-description\n  kival groups update <GROUP_ID> --input update.json"
    )]
    Update(GroupsUpdateCommand),
    /// Archive a group while retaining its memberships and historical record.
    ///
    /// Archived groups no longer participate in group-based object access.
    #[command(name = "archive")]
    Archive(GroupsArchiveCommand),
    /// Restore an archived group to active status.
    #[command(name = "unarchive")]
    Unarchive(GroupsUnarchiveCommand),
    /// Manage group memberships.
    #[command(name = "memberships")]
    Memberships(GroupMembershipsCommand),
}

/// Arguments for `kival groups list`.
#[derive(Debug, Parser)]
pub struct GroupsListCommand {
    /// Archive status filter: active, archived, or all.
    #[arg(long, value_name = "STATUS", value_enum, default_value = "active")]
    pub status: CliArchiveListStatus,
    /// Maximum number of groups to return.
    #[arg(long, value_name = "N", default_value = DEFAULT_LIST_LIMIT_HELP)]
    pub limit: Option<i64>,
    /// Opaque `response.next_cursor` from the previous page; reuse it with the same filters.
    #[arg(long, value_name = "CURSOR")]
    pub cursor: Option<String>,
}

/// Arguments for `kival groups get`.
#[derive(Debug, Clone, Copy, Parser)]
pub struct GroupsGetCommand {
    /// Group ID.
    #[arg(value_name = "GROUP_ID")]
    pub group_id: Uuid,
}

/// Semantic input for creating a group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateGroupInput {
    /// Group name.
    pub name: String,
    /// Group description.
    #[serde(default)]
    pub description: Option<String>,
}

/// Semantic input for updating a group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateGroupInput {
    /// New group name.
    #[serde(default, deserialize_with = "deserialize_optional_non_null")]
    pub name: Option<String>,
    /// New group description, or null to clear it.
    #[serde(default, deserialize_with = "deserialize_optional_nullable")]
    pub description: Option<Option<String>>,
}

/// Arguments for `kival groups create`.
#[derive(Debug, Parser)]
pub struct GroupsCreateCommand {
    /// Structured input source.
    #[command(flatten)]
    pub input_source: StructuredInputArgs,
    /// Group name.
    #[arg(long, value_name = "NAME", required_unless_present = "input")]
    pub name: Option<String>,
    /// Group description.
    #[arg(long, value_name = "DESCRIPTION")]
    pub description: Option<String>,
}

/// Arguments for `kival groups update`.
#[derive(Debug, Parser)]
pub struct GroupsUpdateCommand {
    /// Structured input source.
    #[command(flatten)]
    pub input_source: StructuredInputArgs,
    /// Group ID.
    #[arg(value_name = "GROUP_ID")]
    pub group_id: Uuid,
    /// Set the group name.
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,
    /// Set the group description.
    #[arg(long, value_name = "DESCRIPTION", conflicts_with = "clear_description")]
    pub description: Option<String>,
    /// Clear the group description.
    #[arg(long)]
    pub clear_description: bool,
}

/// Arguments for `kival groups archive`.
#[derive(Debug, Clone, Copy, Parser)]
pub struct GroupsArchiveCommand {
    /// Group ID.
    #[arg(value_name = "GROUP_ID")]
    pub group_id: Uuid,
}

/// Arguments for `kival groups unarchive`.
#[derive(Debug, Clone, Copy, Parser)]
pub struct GroupsUnarchiveCommand {
    /// Group ID.
    #[arg(value_name = "GROUP_ID")]
    pub group_id: Uuid,
}

/// Arguments for `kival groups memberships`.
#[derive(Debug, Parser, CommandSchema)]
pub struct GroupMembershipsCommand {
    /// The membership command to run.
    #[command(subcommand)]
    pub command: GroupMembershipsSubcommand,
}

/// The available `kival groups memberships` commands.
#[derive(Debug, Subcommand, CommandSchema)]
pub enum GroupMembershipsSubcommand {
    /// List active group memberships, newest first.
    #[command(name = "list")]
    List(GroupMembershipsListCommand),

    /// Add a user as a member or administrator of a group.
    ///
    /// Group membership can contribute to object access only where the group is actively linked to
    /// the workspace and has an active object grant.
    #[command(name = "create")]
    Create(GroupMembershipsCreateCommand),

    /// Revoke a group membership without deleting its historical record.
    ///
    /// Revocation removes access derived through this membership but does not revoke the user's
    /// independent direct access.
    #[command(name = "revoke")]
    Revoke(GroupMembershipsRevokeCommand),
}

/// Arguments for `kival groups memberships list`.
#[derive(Debug, Parser)]
pub struct GroupMembershipsListCommand {
    /// Group ID.
    #[arg(value_name = "GROUP_ID")]
    pub group_id: Uuid,

    /// Maximum number of memberships to return.
    #[arg(long, value_name = "N", default_value = DEFAULT_LIST_LIMIT_HELP)]
    pub limit: Option<i64>,

    /// Opaque `response.next_cursor` from the previous page; reuse it with the same filters.
    #[arg(long, value_name = "CURSOR")]
    pub cursor: Option<String>,
}

/// Arguments for `kival groups memberships create`.
#[derive(Debug, Clone, Copy, Parser)]
pub struct GroupMembershipsCreateCommand {
    /// Group ID.
    #[arg(value_name = "GROUP_ID")]
    pub group_id: Uuid,

    /// User ID.
    #[arg(long, value_name = "USER_ID")]
    pub user_id: Uuid,

    /// Group role: member or admin.
    #[arg(long, value_name = "ROLE", value_enum)]
    pub role: CliMembershipRole,
}

/// Arguments for `kival groups memberships revoke`.
#[derive(Debug, Clone, Copy, Parser)]
pub struct GroupMembershipsRevokeCommand {
    /// Group ID.
    #[arg(value_name = "GROUP_ID")]
    pub group_id: Uuid,

    /// Membership ID.
    #[arg(value_name = "MEMBERSHIP_ID")]
    pub membership_id: Uuid,
}

impl GroupsCommand {
    /// Run `kival groups`.
    ///
    /// # Errors
    ///
    /// Returns an error if the selected group command fails.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<()> {
        match self.command {
            GroupsSubcommand::List(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            GroupsSubcommand::Get(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            GroupsSubcommand::Create(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            GroupsSubcommand::Update(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            GroupsSubcommand::Archive(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            GroupsSubcommand::Unarchive(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
            GroupsSubcommand::Memberships(command) => command.run(ctx, output).await,
        }
    }
}

#[schema_handler(run)]
impl GroupsListCommand {
    /// Run `kival groups list`.
    ///
    /// # Errors
    ///
    /// Returns an error if groups cannot be listed.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<ListResponse<Group>> {
        let client = authenticated_client(&ctx)?;
        let response = client
            .list_groups(&GroupListParams {
                limit: Some(self.limit.unwrap_or(DEFAULT_LIST_LIMIT)),
                cursor: self.cursor,
                status: self.status.into(),
                q: None,
            })
            .await?;
        print_output(output, &response, || {
            print_group_page(&response.items, response.next_cursor.as_deref())
        })?;
        Ok(response)
    }
}

#[schema_handler(run)]
impl GroupsGetCommand {
    /// Run `kival groups get`.
    ///
    /// # Errors
    ///
    /// Returns an error if the group cannot be fetched.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<Group> {
        let client = authenticated_client(&ctx)?;
        let group = client.get_group(self.group_id).await?;
        print_output(output, &group, || print_group_line(&group, None))?;
        Ok(group)
    }
}

#[schema_handler(run)]
impl GroupsCreateCommand {
    /// Run `kival groups create`.
    ///
    /// # Errors
    ///
    /// Returns an error if the group cannot be created.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<Group> {
        let input = self.into_input()?;
        let name = input.name.trim();
        if name.is_empty() {
            return Err(CliError::invalid_argument("name must not be empty").into());
        }
        let description = input.description.as_deref().map(str::trim);
        if matches!(description, Some("")) {
            return Err(CliError::invalid_argument("description must not be empty").into());
        }
        let client = authenticated_client(&ctx)?;
        let group = client
            .create_group(CreateGroupRequest {
                name: name.to_owned(),
                description: description.map(ToOwned::to_owned),
            })
            .await?;
        print_output(output, &group, || print_group_line(&group, Some("created")))?;
        Ok(group)
    }

    /// Resolves semantic create input from either `--input` or CLI payload fields.
    fn into_input(self) -> Result<CreateGroupInput> {
        reject_conflicting_input(
            &self.input_source.input,
            &[("name", self.name.is_some()), ("description", self.description.is_some())],
        )?;

        if let Some(input) = self.input_source.input {
            return read_json_input(input);
        }

        let name = self.name.ok_or_else(|| CliError::invalid_argument("name is required"))?;
        Ok(CreateGroupInput { name, description: self.description })
    }
}

#[schema_handler(run)]
impl GroupsUpdateCommand {
    /// Run `kival groups update`.
    ///
    /// # Errors
    ///
    /// Returns an error if the group cannot be updated.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<Group> {
        let group_id = self.group_id;
        let input = self.into_input()?;
        let name = input.name.as_deref().map(str::trim);
        let description = input.description.as_ref().map(|value| value.as_deref().map(str::trim));
        if name.is_none() && input.description.is_none() {
            return Err(CliError::invalid_argument("at least one field must be provided").into());
        }
        if matches!(name, Some("")) {
            return Err(CliError::invalid_argument("name must not be empty").into());
        }
        if matches!(description, Some(Some(""))) {
            return Err(CliError::invalid_argument("description must not be empty").into());
        }
        let client = authenticated_client(&ctx)?;
        let description = match description {
            None => PatchField::Missing,
            Some(None) => PatchField::Null,
            Some(Some(value)) => PatchField::Value(value.to_owned()),
        };
        let group = client
            .update_group(
                group_id,
                UpdateGroupRequest { name: name.map(ToOwned::to_owned), description },
            )
            .await?;
        print_output(output, &group, || print_group_line(&group, Some("updated")))?;
        Ok(group)
    }

    /// Resolves semantic update input from either `--input` or CLI payload fields.
    fn into_input(self) -> Result<UpdateGroupInput> {
        reject_conflicting_input(
            &self.input_source.input,
            &[
                ("name", self.name.is_some()),
                ("description", self.description.is_some()),
                ("clear_description", self.clear_description),
            ],
        )?;

        if let Some(input) = self.input_source.input {
            let input: UpdateGroupInput = read_json_input(input)?;
            if input.name.is_none() && input.description.is_none() {
                return Err(CliError::input_invalid_value(at_least_one_input_field(&[
                    "name",
                    "description",
                ]))
                .into());
            }
            return Ok(input);
        }

        Ok(UpdateGroupInput {
            name: self.name,
            description: if self.clear_description {
                Some(None)
            } else {
                self.description.map(Some)
            },
        })
    }
}

#[schema_handler(run)]
impl GroupsArchiveCommand {
    /// Run `kival groups archive`.
    ///
    /// # Errors
    ///
    /// Returns an error if the group cannot be archived.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<Group> {
        let client = authenticated_client(&ctx)?;
        let group = client.archive_group(self.group_id).await?;
        print_output(output, &group, || print_group_line(&group, Some("archived")))?;
        Ok(group)
    }
}

#[schema_handler(run)]
impl GroupsUnarchiveCommand {
    /// Run `kival groups unarchive`.
    ///
    /// # Errors
    ///
    /// Returns an error if the group cannot be unarchived.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<Group> {
        let client = authenticated_client(&ctx)?;
        let group = client.unarchive_group(self.group_id).await?;
        print_output(output, &group, || print_group_line(&group, Some("unarchived")))?;
        Ok(group)
    }
}

impl GroupMembershipsCommand {
    /// Run `kival groups memberships`.
    ///
    /// # Errors
    ///
    /// Returns an error if the selected membership command fails.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<()> {
        match self.command {
            GroupMembershipsSubcommand::List(command) => {
                command.run(ctx, output).await?;
            }
            GroupMembershipsSubcommand::Create(command) => {
                command.run(ctx, output).await?;
            }
            GroupMembershipsSubcommand::Revoke(command) => {
                command.run(ctx, output).await?;
            }
        }
        Ok(())
    }
}

#[schema_handler(run)]
impl GroupMembershipsListCommand {
    /// Run `kival groups memberships list`.
    ///
    /// # Errors
    ///
    /// Returns an error if memberships cannot be listed.
    pub async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> Result<ListResponse<GroupMembership>> {
        let client = authenticated_client(&ctx)?;
        let response = client
            .list_group_memberships(self.group_id, &list_params(self.limit, self.cursor))
            .await?;
        print_output(output, &response, || {
            if response.items.is_empty() {
                print_empty_list("memberships");
            } else {
                for membership in &response.items {
                    print_group_membership_line(membership, None);
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
impl GroupMembershipsCreateCommand {
    /// Run `kival groups memberships create`.
    ///
    /// # Errors
    ///
    /// Returns an error if the membership cannot be created.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<GroupMembership> {
        let role = MembershipRole::from(self.role);
        let client = authenticated_client(&ctx)?;
        let membership = client
            .create_group_membership(
                self.group_id,
                CreateGroupMembershipRequest {
                    user_id: Some(self.user_id),
                    username: None,
                    group_role: role,
                },
            )
            .await?;
        print_output(output, &membership, || {
            print_group_membership_line(&membership, Some("created"));
        })?;
        Ok(membership)
    }
}

#[schema_handler(run)]
impl GroupMembershipsRevokeCommand {
    /// Run `kival groups memberships revoke`.
    ///
    /// # Errors
    ///
    /// Returns an error if the membership cannot be revoked.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<GroupMembership> {
        let client = authenticated_client(&ctx)?;
        let membership = client.revoke_group_membership(self.group_id, self.membership_id).await?;
        print_output(output, &membership, || {
            print_group_membership_line(&membership, Some("revoked"));
        })?;
        Ok(membership)
    }
}

/// Prints a compact group page.
fn print_group_page(groups: &[Group], next_cursor: Option<&str>) {
    if groups.is_empty() {
        print_empty_list("groups");
    } else {
        for group in groups {
            print_group_line(group, None);
        }
    }
    if let Some(cursor) = next_cursor {
        println!("\nNext cursor: {cursor}");
    }
}

/// Prints a compact group line.
fn print_group_line(group: &Group, action: Option<&str>) {
    let mut fields = vec![group.id.to_string()];
    if let Some(action) = action {
        fields.push(format!("action={action}"));
    }
    fields.extend([
        format!("created={}", format_human_timestamp(group.created_at)),
        format!("updated={}", format_human_timestamp(group.updated_at)),
        format!("status={}", group.status),
        format!("name={}", quote_human_string(&group.name)),
    ]);
    if let Some(description) = &group.description {
        fields.push(format!("description={}", quote_human_string(description)));
    }
    println!("{}", fields.join(" "));
}

/// Prints a compact group membership line.
fn print_group_membership_line(membership: &GroupMembership, action: Option<&str>) {
    let mut fields = vec![membership.id.to_string()];
    if let Some(action) = action {
        fields.push(format!("action={action}"));
    }
    fields.extend([
        format!("group={}", membership.group_id),
        format!("user={}", membership.user_id),
        format!("role={}", membership.group_role),
    ]);
    println!("{}", fields.join(" "));
}

#[cfg(test)]
mod tests {
    use serde_json::error::Category;

    use super::*;

    #[test]
    fn update_group_input_preserves_description_null_state() {
        let omitted: UpdateGroupInput = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(omitted.description, None);

        let cleared: UpdateGroupInput = serde_json::from_str(r#"{"description":null}"#).unwrap();
        assert_eq!(cleared.description, Some(None));

        let set: UpdateGroupInput = serde_json::from_str(r#"{"description":"New"}"#).unwrap();
        assert_eq!(set.description, Some(Some("New".to_owned())));

        let null_name = serde_json::from_str::<UpdateGroupInput>(r#"{"name":null}"#)
            .expect_err("name null should be rejected during deserialization");
        assert_eq!(null_name.classify(), Category::Data);
    }
}
