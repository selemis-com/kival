//! Object comment and discussion-thread commands.

use argx::{Args, Subcommand};
use eyre::Result;
use kival_cli::runner::CliContext;
use kival_sdk::{
    Comment, CommentMentionCandidate, CommentMentionCandidateParams, CommentStatus, CommentThread,
    CreateCommentRequest, KivalClient, ListParams, ListResponse, MAX_LIMIT, UpdateCommentRequest,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::utils::error::CliResult;
use crate::utils::{
    args::{DEFAULT_LIST_LIMIT, list_params},
    credentials::authenticated_client,
    error::{CliError, CliErrorCode},
    input::{StructuredInputArgs, read_json_input, reject_conflicting_input},
    output::{
        OutputMode, format_human_timestamp, print_empty_list, print_output, quote_human_string,
    },
};

/// Arguments for `kival comments`.
#[derive(Debug, Args)]
#[argx(schema)]
pub struct CommentsCommand {
    /// The comments command to run.
    #[argx(subcommand)]
    pub command: CommentsSubcommand,
}

/// The available `kival comments` commands.
#[derive(Debug, Subcommand)]
#[argx(schema)]
pub enum CommentsSubcommand {
    /// List discussion threads, or list one thread's comments with `--thread-id`.
    #[argx(name = "list")]
    List(CommentsListCommand),
    /// List users who can currently view the object and may be mentioned in comments.
    #[argx(name = "mentions")]
    Mentions(CommentsMentionsCommand),
    /// Create a new root comment and discussion thread.
    #[argx(name = "create")]
    Create(CommentsCreateCommand),
    /// Reply in the discussion thread containing a comment.
    ///
    /// Kival commentary is thread-rooted rather than arbitrarily nested. The supplied comment may
    /// be any comment in the thread; the server retains its canonical root comment as the reply's
    /// parent.
    #[argx(name = "reply")]
    Reply(CommentsReplyCommand),
    /// Update a comment authored by the current user.
    #[argx(name = "update")]
    Update(CommentsUpdateCommand),
    /// Delete a comment authored by the current user, or moderate it as an object admin.
    #[argx(name = "delete")]
    Delete(CommentsDeleteCommand),
    /// Resolve a discussion thread.
    #[argx(name = "resolve")]
    Resolve(CommentsResolveCommand),
    /// Reopen a resolved discussion thread.
    #[argx(name = "reopen")]
    Reopen(CommentsReopenCommand),
}

/// Shared workspace/object selector for object commentary commands.
#[derive(Debug, Clone, Copy, Args)]
pub struct CommentObjectTargetArgs {
    /// Workspace ID.

    pub workspace_id: Uuid,
    /// Object ID.

    pub object_id: Uuid,
}

/// Semantic input for creating or updating a comment.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommentWriteInput {
    /// Complete comment body.
    pub body: String,
    /// Stable user IDs to mention in addition to `@username` tokens in the body.
    #[serde(default)]
    pub mentioned_user_ids: Vec<Uuid>,
}

/// Arguments for `kival comments list`.
#[derive(Debug, Args)]
pub struct CommentsListCommand {
    /// Object whose discussion should be listed.
    #[argx(flatten)]
    pub target: CommentObjectTargetArgs,
    /// List comments in this thread instead of listing object threads.
    #[argx(long)]
    pub thread_id: Option<Uuid>,
    /// Maximum number of threads or comments to return.
    #[argx(long, default = DEFAULT_LIST_LIMIT)]
    pub limit: Option<i64>,
    /// Opaque `response.next_cursor` from the previous page; reuse it with the same list mode.
    #[argx(long)]
    pub cursor: Option<String>,
}

/// Structured output from `kival comments list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "collection", rename_all = "snake_case")]
pub enum CommentsListOutput {
    /// A page of discussion threads. Each thread includes its initial bounded comment page.
    Threads {
        /// Thread items ordered by most recent activity.
        items: Vec<CommentThread>,
        /// Opaque cursor for the next thread page.
        #[serde(skip_serializing_if = "Option::is_none")]
        next_cursor: Option<String>,
    },
    /// A page of comments in one thread.
    Comments {
        /// Thread whose comments were listed.
        thread_id: Uuid,
        /// Comments ordered by creation time, with the root first when present on this page.
        items: Vec<Comment>,
        /// Opaque cursor for the next comment page.
        #[serde(skip_serializing_if = "Option::is_none")]
        next_cursor: Option<String>,
    },
}

/// Arguments for `kival comments mentions`.
#[derive(Debug, Args)]
pub struct CommentsMentionsCommand {
    /// Object whose mention candidates should be listed.
    #[argx(flatten)]
    pub target: CommentObjectTargetArgs,
    /// Username prefix or display-name fragment.
    #[argx(long)]
    pub query: Option<String>,
    /// Maximum number of candidates to return. Defaults to 8 and is capped at 20.
    #[argx(long)]
    pub limit: Option<i64>,
}

/// Arguments for `kival comments create`.
#[derive(Debug, Args)]
pub struct CommentsCreateCommand {
    /// Object receiving the new discussion thread.
    #[argx(flatten)]
    pub target: CommentObjectTargetArgs,
    /// Structured input source.
    #[argx(flatten)]
    pub input_source: StructuredInputArgs,
    /// Comment body. Required unless supplied by `--input`.
    #[argx(long)]
    pub body: Option<String>,
    /// Stable user ID to mention. May be repeated.
    #[argx(long)]
    pub mention_user_id: Vec<Uuid>,
}

/// Arguments for `kival comments reply`.
#[derive(Debug, Args)]
pub struct CommentsReplyCommand {
    /// Object containing the discussion.
    #[argx(flatten)]
    pub target: CommentObjectTargetArgs,
    /// Any comment in the thread to reply in.

    pub comment_id: Uuid,
    /// Structured input source.
    #[argx(flatten)]
    pub input_source: StructuredInputArgs,
    /// Reply body. Required unless supplied by `--input`.
    #[argx(long)]
    pub body: Option<String>,
    /// Stable user ID to mention. May be repeated.
    #[argx(long)]
    pub mention_user_id: Vec<Uuid>,
}

/// Arguments for `kival comments update`.
#[derive(Debug, Args)]
pub struct CommentsUpdateCommand {
    /// Object containing the comment.
    #[argx(flatten)]
    pub target: CommentObjectTargetArgs,
    /// Comment ID.

    pub comment_id: Uuid,
    /// Structured input source.
    #[argx(flatten)]
    pub input_source: StructuredInputArgs,
    /// Complete replacement body. Required unless supplied by `--input`.
    #[argx(long)]
    pub body: Option<String>,
    /// Stable replacement mention user ID. May be repeated.
    #[argx(long)]
    pub mention_user_id: Vec<Uuid>,
}

/// Arguments for `kival comments delete`.
#[derive(Debug, Clone, Copy, Args)]
pub struct CommentsDeleteCommand {
    /// Object containing the comment.
    #[argx(flatten)]
    pub target: CommentObjectTargetArgs,
    /// Comment ID.

    pub comment_id: Uuid,
}

/// Arguments for `kival comments resolve`.
#[derive(Debug, Clone, Copy, Args)]
pub struct CommentsResolveCommand {
    /// Object containing the thread.
    #[argx(flatten)]
    pub target: CommentObjectTargetArgs,
    /// Thread ID.

    pub thread_id: Uuid,
}

/// Arguments for `kival comments reopen`.
#[derive(Debug, Clone, Copy, Args)]
pub struct CommentsReopenCommand {
    /// Object containing the thread.
    #[argx(flatten)]
    pub target: CommentObjectTargetArgs,
    /// Thread ID.

    pub thread_id: Uuid,
}

impl CommentsCommand {
    /// Run `kival comments`.
    ///
    /// # Errors
    ///
    /// Returns an error if the selected comments command fails.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<()> {
        match self.command {
            CommentsSubcommand::List(command) => {
                command.run(ctx, output).await?;
            }
            CommentsSubcommand::Mentions(command) => {
                command.run(ctx, output).await?;
            }
            CommentsSubcommand::Create(command) => {
                command.run(ctx, output).await?;
            }
            CommentsSubcommand::Reply(command) => {
                command.run(ctx, output).await?;
            }
            CommentsSubcommand::Update(command) => {
                command.run(ctx, output).await?;
            }
            CommentsSubcommand::Delete(command) => {
                command.run(ctx, output).await?;
            }
            CommentsSubcommand::Resolve(command) => {
                command.run(ctx, output).await?;
            }
            CommentsSubcommand::Reopen(command) => {
                command.run(ctx, output).await?;
            }
        }
        Ok(())
    }
}

#[argx(handler = run)]
impl CommentsListCommand {
    /// Run `kival comments list`.
    ///
    /// # Errors
    ///
    /// Returns an error if commentary cannot be listed.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> CliResult<CommentsListOutput> {
        let client = authenticated_client(&ctx)?;
        let params = list_params(self.limit, self.cursor);
        let result = if let Some(thread_id) = self.thread_id {
            let response = client
                .list_comment_thread_comments(
                    self.target.workspace_id,
                    self.target.object_id,
                    thread_id,
                    &params,
                )
                .await?;
            CommentsListOutput::Comments {
                thread_id,
                items: response.items,
                next_cursor: response.next_cursor,
            }
        } else {
            let response = client
                .list_object_commentary(self.target.workspace_id, self.target.object_id, &params)
                .await?;
            CommentsListOutput::Threads { items: response.items, next_cursor: response.next_cursor }
        };

        print_output(output, &result, || print_list_output(&result))?;
        Ok(result)
    }
}

#[argx(handler = run)]
impl CommentsMentionsCommand {
    /// Run `kival comments mentions`.
    ///
    /// # Errors
    ///
    /// Returns an error if mention candidates cannot be listed.
    pub async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> CliResult<ListResponse<CommentMentionCandidate>> {
        let client = authenticated_client(&ctx)?;
        let response = client
            .list_comment_mention_candidates(
                self.target.workspace_id,
                self.target.object_id,
                &CommentMentionCandidateParams {
                    q: self.query.unwrap_or_default(),
                    limit: self.limit,
                },
            )
            .await?;
        print_output(output, &response, || {
            if response.items.is_empty() {
                print_empty_list("mention candidates");
            } else {
                for candidate in &response.items {
                    println!(
                        "{} username={} display_name={}",
                        candidate.user_id,
                        candidate.username,
                        quote_human_string(&candidate.display_name),
                    );
                }
            }
        })?;
        Ok(response)
    }
}

#[argx(handler = run)]
impl CommentsCreateCommand {
    /// Run `kival comments create`.
    ///
    /// # Errors
    ///
    /// Returns an error if the root comment cannot be created.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> CliResult<CommentThread> {
        let target = self.target;
        let input = self.into_input()?;
        let client = authenticated_client(&ctx)?;
        let thread = client
            .create_comment_thread(target.workspace_id, target.object_id, &input.into_request())
            .await?;
        print_output(output, &thread, || print_thread_action(&thread, "created"))?;
        Ok(thread)
    }

    /// Resolves semantic comment input from either `--input` or CLI payload fields.
    fn into_input(self) -> Result<CommentWriteInput> {
        comment_write_input(self.input_source, self.body, self.mention_user_id)
    }
}

#[argx(handler = run)]
impl CommentsReplyCommand {
    /// Run `kival comments reply`.
    ///
    /// # Errors
    ///
    /// Returns an error if the target comment cannot be found or its thread cannot be replied to.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> CliResult<Comment> {
        let target = self.target;
        let comment_id = self.comment_id;
        let input = self.into_input()?;
        let client = authenticated_client(&ctx)?;
        let thread_id = find_comment_thread(&client, target, comment_id).await?;
        let comment = client
            .reply_to_comment_thread(
                target.workspace_id,
                target.object_id,
                thread_id,
                &input.into_request(),
            )
            .await?;
        print_output(output, &comment, || print_comment_action(&comment, "created"))?;
        Ok(comment)
    }

    /// Resolves semantic reply input from either `--input` or CLI payload fields.
    fn into_input(self) -> Result<CommentWriteInput> {
        comment_write_input(self.input_source, self.body, self.mention_user_id)
    }
}

#[argx(handler = run)]
impl CommentsUpdateCommand {
    /// Run `kival comments update`.
    ///
    /// # Errors
    ///
    /// Returns an error if the comment cannot be updated.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> CliResult<Comment> {
        let target = self.target;
        let comment_id = self.comment_id;
        let input = self.into_input()?;
        let client = authenticated_client(&ctx)?;
        let comment = client
            .update_comment(
                target.workspace_id,
                target.object_id,
                comment_id,
                &UpdateCommentRequest {
                    body: input.body,
                    mentioned_user_ids: input.mentioned_user_ids,
                },
            )
            .await?;
        print_output(output, &comment, || print_comment_action(&comment, "updated"))?;
        Ok(comment)
    }

    /// Resolves semantic update input from either `--input` or CLI payload fields.
    fn into_input(self) -> Result<CommentWriteInput> {
        comment_write_input(self.input_source, self.body, self.mention_user_id)
    }
}

#[argx(handler = run)]
impl CommentsDeleteCommand {
    /// Run `kival comments delete`.
    ///
    /// # Errors
    ///
    /// Returns an error if the comment cannot be deleted.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> CliResult<Comment> {
        let client = authenticated_client(&ctx)?;
        let comment = client
            .delete_comment(self.target.workspace_id, self.target.object_id, self.comment_id)
            .await?;
        print_output(output, &comment, || print_comment_action(&comment, "deleted"))?;
        Ok(comment)
    }
}

#[argx(handler = run)]
impl CommentsResolveCommand {
    /// Run `kival comments resolve`.
    ///
    /// # Errors
    ///
    /// Returns an error if the thread cannot be resolved.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> CliResult<CommentThread> {
        let client = authenticated_client(&ctx)?;
        let thread = client
            .resolve_comment_thread(self.target.workspace_id, self.target.object_id, self.thread_id)
            .await?;
        print_output(output, &thread, || print_thread_action(&thread, "resolved"))?;
        Ok(thread)
    }
}

#[argx(handler = run)]
impl CommentsReopenCommand {
    /// Run `kival comments reopen`.
    ///
    /// # Errors
    ///
    /// Returns an error if the thread cannot be reopened.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> CliResult<CommentThread> {
        let client = authenticated_client(&ctx)?;
        let thread = client
            .reopen_comment_thread(self.target.workspace_id, self.target.object_id, self.thread_id)
            .await?;
        print_output(output, &thread, || print_thread_action(&thread, "reopened"))?;
        Ok(thread)
    }
}

impl CommentWriteInput {
    /// Converts semantic CLI input into the existing SDK request type.
    fn into_request(self) -> CreateCommentRequest {
        CreateCommentRequest { body: self.body, mentioned_user_ids: self.mentioned_user_ids }
    }
}

/// Resolves a write payload while preserving the shared stable structured-input error surface.
fn comment_write_input(
    input_source: StructuredInputArgs,
    body: Option<String>,
    mention_user_id: Vec<Uuid>,
) -> Result<CommentWriteInput> {
    reject_conflicting_input(
        &input_source.input,
        &[("body", body.is_some()), ("mentioned_user_ids", !mention_user_id.is_empty())],
    )?;

    if let Some(input) = input_source.input {
        return read_json_input(input);
    }

    let body = body.ok_or_else(|| CliError::invalid_argument("comment body is required"))?;
    Ok(CommentWriteInput { body, mentioned_user_ids: mention_user_id })
}

/// Finds the thread containing a comment using only the existing public commentary list APIs.
async fn find_comment_thread(
    client: &KivalClient,
    target: CommentObjectTargetArgs,
    comment_id: Uuid,
) -> Result<Uuid> {
    let mut thread_cursor = None;

    loop {
        let response = client
            .list_object_commentary(
                target.workspace_id,
                target.object_id,
                &ListParams { limit: Some(MAX_LIMIT), cursor: thread_cursor },
            )
            .await?;

        for thread in &response.items {
            if thread.comments.iter().any(|comment| comment.id == comment_id) {
                return Ok(thread.id);
            }

            let mut comment_cursor = thread.comments_next_cursor.clone();
            while let Some(cursor) = comment_cursor {
                let comments = client
                    .list_comment_thread_comments(
                        target.workspace_id,
                        target.object_id,
                        thread.id,
                        &ListParams { limit: Some(MAX_LIMIT), cursor: Some(cursor) },
                    )
                    .await?;
                if comments.items.iter().any(|comment| comment.id == comment_id) {
                    return Ok(thread.id);
                }
                comment_cursor = comments.next_cursor;
            }
        }

        match response.next_cursor {
            Some(cursor) => thread_cursor = Some(cursor),
            None => break,
        }
    }

    Err(CliError {
        code: CliErrorCode::ResourceNotFound,
        message: "Comment was not found on this object.".to_owned(),
        details: None,
    }
    .into())
}

/// Prints either a thread page or a single-thread comment page.
fn print_list_output(output: &CommentsListOutput) {
    match output {
        CommentsListOutput::Threads { items, next_cursor } => {
            if items.is_empty() {
                print_empty_list("comment threads");
            } else {
                for thread in items {
                    print_thread(thread);
                }
            }
            if let Some(cursor) = next_cursor {
                println!("next_cursor={cursor}");
            }
        }
        CommentsListOutput::Comments { thread_id, items, next_cursor } => {
            if items.is_empty() {
                print_empty_list("comments");
            } else {
                for comment in items {
                    print_comment(comment, None);
                }
            }
            if let Some(cursor) = next_cursor {
                println!("thread={thread_id} next_cursor={cursor}");
            }
        }
    }
}

/// Prints one discussion thread and the bounded comments included with it.
fn print_thread(thread: &CommentThread) {
    let state = if thread.resolved_at.is_some() { "resolved" } else { "open" };
    println!(
        "{} status={state} created_by={} created_at={} updated_at={} comments={}",
        thread.id,
        thread.created_by,
        format_human_timestamp(thread.created_at),
        format_human_timestamp(thread.updated_at),
        thread.comments.len(),
    );
    for comment in &thread.comments {
        print_comment(comment, Some("  "));
    }
    if let Some(cursor) = &thread.comments_next_cursor {
        println!("  comments_next_cursor={cursor}");
    }
}

/// Prints one comment in compact human-readable form.
fn print_comment(comment: &Comment, prefix: Option<&str>) {
    let prefix = prefix.unwrap_or_default();
    let mut fields = vec![
        format!("thread={}", comment.thread_id),
        format!("author={}", comment.author.username),
        format!("status={}", comment_status_name(comment.status)),
    ];
    if let Some(parent_comment_id) = comment.parent_comment_id {
        fields.push(format!("parent={parent_comment_id}"));
    }
    fields.push(format!("created_at={}", format_human_timestamp(comment.created_at)));
    if let Some(body) = &comment.body {
        fields.push(format!("body={}", quote_human_string(body)));
    }
    println!("{prefix}{} {}", comment.id, fields.join(" "));
}

/// Prints one comment mutation result.
fn print_comment_action(comment: &Comment, action: &str) {
    let parent = comment.parent_comment_id.map(|id| format!(" parent={id}")).unwrap_or_default();
    println!(
        "{} action={action} thread={}{} status={}",
        comment.id,
        comment.thread_id,
        parent,
        comment_status_name(comment.status),
    );
}

/// Prints one thread mutation result, including its root comment when available.
fn print_thread_action(thread: &CommentThread, action: &str) {
    let root = thread.comments.first().map(|comment| comment.id);
    let root = root.map(|id| format!(" root_comment={id}")).unwrap_or_default();
    println!("{} action={action}{root}", thread.id);
}

/// Returns the stable human-readable name of a comment lifecycle state.
const fn comment_status_name(status: CommentStatus) -> &'static str {
    match status {
        CommentStatus::Active => "active",
        CommentStatus::Deleted => "deleted",
        CommentStatus::Expired => "expired",
    }
}
