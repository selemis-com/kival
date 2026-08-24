//! Event commands.

use clap::{Parser, Subcommand};
use clap_schema::{CommandSchema, schema_handler};
use eyre::Result;
use kival_cli::runner::CliContext;
use kival_sdk::{Event, ListResponse};
use uuid::Uuid;

use crate::utils::{
    args::{DEFAULT_LIST_LIMIT_HELP, event_params},
    credentials::authenticated_client,
    output::{
        OutputMode, format_human_timestamp, print_empty_list, print_output,
        push_optional_uuid_field, quote_human_string,
    },
};

/// Arguments for `kival events`.
#[derive(Debug, Parser, CommandSchema)]
pub struct EventsCommand {
    /// The event command to run.
    #[command(subcommand)]
    pub command: EventsSubcommand,
}

/// The available `kival events` commands.
#[derive(Debug, Subcommand, CommandSchema)]
pub enum EventsSubcommand {
    /// List events visible to the current user.
    ///
    /// Events are returned in ascending global sequence order. `--after-sequence` is exclusive.
    /// When multiple filters are supplied, every filter must match.
    #[command(name = "list")]
    List(EventsListCommand),
}

/// Arguments for `kival events list`.
#[derive(Debug, Parser)]
pub struct EventsListCommand {
    /// Maximum number of events to return.
    #[arg(long, value_name = "N", default_value = DEFAULT_LIST_LIMIT_HELP)]
    pub limit: Option<i64>,

    /// Return events with a global sequence number strictly greater than SEQUENCE.
    #[arg(long, value_name = "SEQUENCE")]
    pub after_sequence: Option<i64>,

    /// Filter by event kind.
    #[arg(long, value_name = "KIND")]
    pub event_kind: Option<String>,

    /// Filter by actor user ID.
    #[arg(long, value_name = "USER_ID")]
    pub actor_user_id: Option<Uuid>,

    /// Filter by target user ID.
    #[arg(long, value_name = "USER_ID")]
    pub target_user_id: Option<Uuid>,

    /// Filter by object ID.
    #[arg(long, value_name = "OBJECT_ID")]
    pub object_id: Option<Uuid>,

    /// Filter by group ID.
    #[arg(long, value_name = "GROUP_ID")]
    pub group_id: Option<Uuid>,
}

impl EventsCommand {
    /// Run `kival events`.
    ///
    /// # Errors
    ///
    /// Returns an error if the selected event command fails.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<()> {
        match self.command {
            EventsSubcommand::List(command) => {
                command.run(ctx, output).await?;
                Ok(())
            }
        }
    }
}

#[schema_handler(run)]
impl EventsListCommand {
    /// Run `kival events list`.
    ///
    /// # Errors
    ///
    /// Returns an error if events cannot be listed.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<ListResponse<Event>> {
        let after_sequence = Some(self.after_sequence.unwrap_or(0));
        let params = event_params(
            self.limit,
            after_sequence,
            self.event_kind,
            self.actor_user_id,
            self.target_user_id,
            self.object_id,
            self.group_id,
        );
        let client = authenticated_client(&ctx)?;
        let response = client.list_events(&params).await?;

        print_output(output, &response, || {
            if response.items.is_empty() {
                print_empty_list("events");
            } else {
                for event in &response.items {
                    print_event_line(event);
                }
            }
        })?;
        Ok(response)
    }
}

/// Prints a compact event line.
pub(crate) fn print_event_line(event: &Event) {
    let mut fields = vec![
        format!("#{}", event.sequence_number),
        event.event_kind.clone(),
        format_human_timestamp(event.created_at),
    ];

    push_optional_uuid_field(&mut fields, "actor", event.actor_user_id);
    push_optional_uuid_field(&mut fields, "api_key", event.api_key_id);
    if let Some(label) = &event.api_key_label {
        fields.push(format!("api_key_label={}", quote_human_string(label)));
    }
    push_optional_uuid_field(&mut fields, "workspace", event.workspace_id);
    push_optional_uuid_field(&mut fields, "object", event.object_id);
    push_optional_uuid_field(&mut fields, "version", event.object_version_id);
    push_optional_uuid_field(&mut fields, "edge", event.object_edge_id);
    push_optional_uuid_field(&mut fields, "grant", event.object_grant_id);
    push_optional_uuid_field(&mut fields, "group", event.group_id);
    push_optional_uuid_field(&mut fields, "target_user", event.target_user_id);

    println!("{}", fields.join(" "));
}
