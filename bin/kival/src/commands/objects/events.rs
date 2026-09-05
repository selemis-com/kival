//! Object event-listing command.

use argx::{Args, argx};
use kival_cli::runner::CliContext;
use kival_sdk::{Event, EventListParams, EventOrder, ListResponse};
use uuid::Uuid;

use super::{ObjectCommandError, ObjectTargetArgs, object_error_codes};
use crate::{
    commands::events::print_event_line,
    utils::{
        args::DEFAULT_LIST_LIMIT,
        credentials::authenticated_client,
        output::{OutputMode, print_empty_list, print_output},
    },
};

object_error_codes! {
    pub(crate) enum ObjectEventsErrorCode {
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
type ObjectEventsError = ObjectCommandError<ObjectEventsErrorCode>;

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
    /// Return events with a global sequence number strictly less than SEQUENCE.
    #[argx(long)]
    pub before_sequence: Option<i64>,
    /// Event sequence ordering.
    #[argx(long, default = EventOrder::Asc)]
    pub order: EventOrder,
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

#[argx(handler = run)]
impl ObjectEventsCommand {
    /// Run `kival objects events`.
    ///
    /// # Errors
    ///
    /// Returns an error if events cannot be listed.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> Result<ListResponse<Event>, ObjectEventsError> {
        let params = EventListParams {
            limit: self.limit,
            after_sequence: self.after_sequence,
            before_sequence: self.before_sequence,
            order: self.order,
            event_kind: self.event_kind,
            actor_user_id: self.actor_user_id,
            target_user_id: self.target_user_id,
            object_id: Some(self.target.object_id),
            group_id: self.group_id,
        };
        let client = authenticated_client(&ctx)?;
        let response = client
            .list_object_events(self.target.workspace_id, self.target.object_id, &params)
            .await?;
        print_output(&output, &response, || {
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
