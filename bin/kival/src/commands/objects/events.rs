//! Object event-listing command.

use argx::Args;
use eyre::Result;
use kival_cli::runner::CliContext;
use kival_sdk::{Event, ListResponse};
use uuid::Uuid;

use super::ObjectTargetArgs;
use crate::{
    commands::events::print_event_line,
    utils::{
        args::{DEFAULT_LIST_LIMIT, event_params},
        credentials::authenticated_client,
        output::{OutputMode, print_empty_list, print_output},
    },
};

/// Arguments for `kival objects events`.
#[derive(Debug, Args)]
pub struct ObjectEventsCommand {
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
    /// Maximum number of events to return.
    #[argx(long, default = DEFAULT_LIST_LIMIT)]
    pub limit: Option<i64>,
    /// Return events with a global sequence number strictly greater than SEQUENCE.
    #[argx(long)]
    pub after_sequence: Option<i64>,
    /// Filter by event kind.
    #[argx(long)]
    pub event_kind: Option<String>,
    /// Filter by actor user ID.
    #[argx(long)]
    pub actor_user_id: Option<Uuid>,
    /// Filter by target user ID.
    #[argx(long)]
    pub target_user_id: Option<Uuid>,
    /// Filter by group ID.
    #[argx(long)]
    pub group_id: Option<Uuid>,
}

impl ObjectEventsCommand {
    /// Run `kival objects events`.
    ///
    /// # Errors
    ///
    /// Returns an error if events cannot be listed.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<ListResponse<Event>> {
        let params = event_params(
            self.limit,
            self.after_sequence,
            self.event_kind,
            self.actor_user_id,
            self.target_user_id,
            Some(self.target.object_id),
            self.group_id,
        );
        let client = authenticated_client(&ctx)?;
        let response = client
            .list_object_events(self.target.workspace_id, self.target.object_id, &params)
            .await?;
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
