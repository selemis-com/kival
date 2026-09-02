//! Object attachment commands.

use std::path::{Path, PathBuf};

use argx::{Args, Subcommand, argx};
use eyre::Result;
use kival_cli::runner::CliContext;
use kival_sdk::{
    ListResponse, ObjectAttachment, ReuseObjectAttachmentRequest, UploadObjectAttachmentParams,
};
use serde::Serialize;
use uuid::Uuid;

use super::{
    ObjectCommandError, ObjectTargetArgs,
    io::{ensure_output_available, write_output_file},
    object_error_codes,
};
use crate::utils::{
    args::{DEFAULT_LIST_LIMIT, list_params, metadata_value},
    credentials::authenticated_client,
    error::{CliFailure, erase_command_error},
    output::{
        OutputMode, format_human_timestamp, print_empty_list, print_output,
        push_optional_uuid_field, quote_human_string,
    },
};

object_error_codes! {
    pub(crate) enum AttachmentListErrorCode {
        failures {
            AuthenticationRequired => ("authentication.required", AuthenticationRequired),
            PermissionDenied => ("permission.denied", PermissionDenied),
            InvalidArgument => ("invalid.argument", InvalidArgument),
            ResourceNotFound => ("resource.not_found", ResourceNotFound),
            InvalidCursor => ("invalid.cursor", InvalidCursor),
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
type AttachmentListError = ObjectCommandError<AttachmentListErrorCode>;

object_error_codes! {
    pub(crate) enum AttachmentUploadErrorCode {
        failures {
            AuthenticationRequired => ("authentication.required", AuthenticationRequired),
            PermissionDenied => ("permission.denied", PermissionDenied),
            InvalidArgument => ("invalid.argument", InvalidArgument),
            ResourceNotFound => ("resource.not_found", ResourceNotFound),
            ResourceConflict => ("resource.conflict", ResourceConflict),
            PayloadTooLarge => ("payload_too_large", PayloadTooLarge),
            ServerUnavailable => ("server.unavailable", ServerUnavailable),
            RateLimited => ("rate_limited", RateLimited),
            RequestFailed => ("request.failed", RequestFailed),
            InvalidField => ("output.invalid_field", InvalidField),
            InvalidProjection => ("output.invalid_projection", InvalidProjection),
            Internal => ("internal", Internal),
        }
        objects { ObjectNotFound => ("object.not_found", NotFound) }
    }
}

/// Error returned by the corresponding command handler.
type AttachmentUploadError = ObjectCommandError<AttachmentUploadErrorCode>;

object_error_codes! {
    pub(crate) enum AttachmentReuseErrorCode {
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
        objects { ObjectNotFound => ("object.not_found", NotFound) }
    }
}

/// Error returned by the corresponding command handler.
type AttachmentReuseError = ObjectCommandError<AttachmentReuseErrorCode>;

object_error_codes! {
    pub(crate) enum AttachmentReadErrorCode {
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
            Internal => ("internal", Internal),
        }
        objects {
            ObjectNotFound => ("object.not_found", NotFound),
        }
    }
}

/// Error returned by the corresponding command handler.
type AttachmentReadError = ObjectCommandError<AttachmentReadErrorCode>;

/// Arguments for `kival objects attachments`.
#[derive(Debug, Args)]
#[argx(schema)]
pub struct ObjectAttachmentsCommand {
    /// The attachment command to run.
    #[argx(subcommand)]
    pub command: ObjectAttachmentsSubcommand,
}

/// The available `kival objects attachments` commands.
#[derive(Debug, Subcommand)]
#[argx(schema)]
pub enum ObjectAttachmentsSubcommand {
    /// List object attachments, newest first.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
            "requiredScopes": ["attachments:read"],
        })
    )]
    List(ObjectAttachmentsListCommand),
    /// Upload a file and create an attachment record on an object.
    ///
    /// `--version-id` associates the attachment with a specific version of the target object; omit
    /// it for an object-level attachment.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": false,
            "idempotent": false,
            "requiredScopes": ["attachments:write"],
        })
    )]
    Upload(ObjectAttachmentsUploadCommand),
    /// Reuse accessible attachment content on another object without re-uploading it.
    ///
    /// The source attachment must be inspectable by the current user, and the target object must be
    /// editable. Reuse creates a new attachment record on the target that references the existing
    /// stored content and records the source attachment as provenance.
    #[argx(
        metadata({
            "readOnly": false,
            "destructive": false,
            "idempotent": false,
            "requiredScopes": ["attachments:write"],
        })
    )]
    Reuse(ObjectAttachmentsReuseCommand),
    /// Get object attachment metadata.
    ///
    /// This command returns the attachment record only; it does not download the attachment bytes.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
            "requiredScopes": ["attachments:read"],
        })
    )]
    Get(ObjectAttachmentsGetCommand),
    /// Get object attachment content and write it to a file.
    #[argx(
        metadata({
            "readOnly": true,
            "destructive": false,
            "idempotent": true,
            "writesLocalFiles": true,
            "requiredScopes": ["attachments:read"],
        })
    )]
    Content(ObjectAttachmentsContentCommand),
}

/// Arguments for `kival objects attachments list`.
#[derive(Debug, Args)]
pub struct ObjectAttachmentsListCommand {
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
    /// Maximum number of attachments to return.
    #[argx(long, default = DEFAULT_LIST_LIMIT)]
    pub limit: Option<i64>,
    /// Opaque `response.next_cursor` from the previous page; reuse it with the same filters.
    #[argx(long)]
    pub cursor: Option<String>,
}

/// Arguments for `kival objects attachments upload`.
#[derive(Debug, Args)]
pub struct ObjectAttachmentsUploadCommand {
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
    /// File to upload.
    #[argx(long)]
    pub file: PathBuf,
    /// Associate the attachment with this version of the target object.
    #[argx(long)]
    pub version_id: Option<Uuid>,
    /// Optional attachment display name. Defaults to the file name.
    #[argx(long)]
    pub name: Option<String>,
    /// Optional media type.
    #[argx(long)]
    pub media_type: Option<String>,
    /// Attachment metadata as a flat JSON object with scalar or scalar-list values.
    #[argx(long)]
    pub metadata: Option<String>,
}

/// Arguments for kival objects attachments reuse.
#[derive(Debug, Clone, Copy, Args)]
pub struct ObjectAttachmentsReuseCommand {
    /// Target object.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
    /// Source attachment ID whose content the current user is authorized to inspect.
    pub source_attachment_id: Uuid,
    /// Associate the new attachment record with this version of the target object.
    #[argx(long)]
    pub version_id: Option<Uuid>,
}

/// Arguments for `kival objects attachments content`.
#[derive(Debug, Args)]
pub struct ObjectAttachmentsContentCommand {
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
    /// Attachment ID.
    pub attachment_id: Uuid,
    /// File to write the attachment content to.
    #[argx(short = 'o', long = "file")]
    pub file: PathBuf,
    /// Overwrite the output file if it already exists.
    #[argx(long)]
    pub force: bool,
}

/// Successful attachment content result.
#[derive(Debug, Serialize)]
#[argx(schema)]
pub struct ObjectAttachmentContentOutput {
    /// Attachment ID that was fetched.
    pub attachment_id: Uuid,
    /// Local output path.
    pub output: String,
    /// Number of bytes written.
    pub bytes_written: usize,
}

/// Arguments for `kival objects attachments get`.
#[derive(Debug, Clone, Copy, Args)]
pub struct ObjectAttachmentsGetCommand {
    /// Object target.
    #[argx(flatten)]
    pub target: ObjectTargetArgs,
    /// Attachment ID.
    pub attachment_id: Uuid,
}

impl ObjectAttachmentsCommand {
    /// Run `kival objects attachments`.
    ///
    /// # Errors
    ///
    /// Returns an error if the selected attachment command fails.
    pub async fn run(self, ctx: CliContext, output: OutputMode) -> Result<()> {
        match self.command {
            ObjectAttachmentsSubcommand::List(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
            ObjectAttachmentsSubcommand::Upload(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
            ObjectAttachmentsSubcommand::Reuse(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
            ObjectAttachmentsSubcommand::Get(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
            ObjectAttachmentsSubcommand::Content(command) => {
                command.run(ctx, output).await.map_err(erase_command_error)?;
            }
        }
        Ok(())
    }
}

#[argx(handler = run)]
impl ObjectAttachmentsListCommand {
    /// Run `kival objects attachments list`.
    ///
    /// # Errors
    ///
    /// Returns an error if attachments cannot be listed.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ListResponse<ObjectAttachment>, AttachmentListError> {
        let client = authenticated_client(&ctx)?;
        let response = client
            .list_object_attachments(
                self.target.workspace_id,
                self.target.object_id,
                &list_params(self.limit, self.cursor),
            )
            .await?;
        print_output(&output, &response, || {
            if response.items.is_empty() {
                print_empty_list("attachments");
            } else {
                for attachment in &response.items {
                    print_attachment_line(attachment, None);
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
impl ObjectAttachmentsUploadCommand {
    /// Run `kival objects attachments upload`.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be uploaded.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ObjectAttachment, AttachmentUploadError> {
        let name = match self.name.as_deref().map(str::trim) {
            Some("") => {
                return Err(AttachmentUploadError::invalid_argument("name must not be empty"));
            }
            Some(name) => Some(name.to_owned()),
            None => self
                .file
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .filter(|name| !name.trim().is_empty()),
        };
        let media_type = resolve_media_type(&self.file, self.media_type.as_deref())?;
        let metadata = metadata_value(self.metadata.as_deref())?;
        let bytes = std::fs::read(&self.file)?;
        let client = authenticated_client(&ctx)?;
        let attachment = client
            .upload_object_attachment(
                self.target.workspace_id,
                self.target.object_id,
                &UploadObjectAttachmentParams {
                    version_id: self.version_id,
                    name,
                    media_type,
                    metadata: Some(metadata.to_string()),
                },
                bytes,
            )
            .await?;
        print_output(&output, &attachment, || {
            print_attachment_line(&attachment, Some("uploaded"));
        })?;
        Ok(attachment)
    }
}

/// Returns an explicit media type or infers one from the file extension.
fn resolve_media_type(path: &Path, media_type: Option<&str>) -> Result<Option<String>> {
    match media_type.map(str::trim) {
        Some("") => Err(CliFailure::invalid_argument("--media-type must not be empty").into()),
        Some(media_type) => Ok(Some(media_type.to_owned())),
        None => Ok(infer_media_type_from_path(path).map(str::to_owned)),
    }
}

/// Infers a media type from a small set of common file extensions.
///
/// This is convenience metadata only. It must not be used as a security
/// boundary or as proof of file contents.
fn infer_media_type_from_path(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();

    match extension.as_str() {
        "pdf" => Some("application/pdf"),
        "md" | "markdown" => Some("text/markdown"),
        "txt" => Some("text/plain"),
        "json" => Some("application/json"),
        "csv" => Some("text/csv"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        _ => None,
    }
}

#[argx(handler = run)]
impl ObjectAttachmentsReuseCommand {
    /// Runs attachment reuse.
    ///
    /// # Errors
    ///
    /// Returns an error if the source attachment cannot be authorized or reused.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ObjectAttachment, AttachmentReuseError> {
        let client = authenticated_client(&ctx)?;
        let attachment = client
            .reuse_object_attachment(
                self.target.workspace_id,
                self.target.object_id,
                ReuseObjectAttachmentRequest {
                    source_attachment_id: self.source_attachment_id,
                    version_id: self.version_id,
                },
            )
            .await?;
        print_output(&output, &attachment, || {
            print_attachment_line(&attachment, Some("reused"));
        })?;
        Ok(attachment)
    }
}

#[argx(handler = run)]
impl ObjectAttachmentsContentCommand {
    /// Run `kival objects attachments content`.
    ///
    /// # Errors
    ///
    /// Returns an error if the attachment cannot be fetched or the output file cannot be written.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ObjectAttachmentContentOutput, AttachmentReadError> {
        ensure_output_available(&self.file, self.force)?;

        let client = authenticated_client(&ctx)?;
        let bytes = client
            .get_object_attachment_content(
                self.target.workspace_id,
                self.target.object_id,
                self.attachment_id,
            )
            .await?;

        write_output_file(&self.file, &bytes, self.force)?;

        let result = ObjectAttachmentContentOutput {
            attachment_id: self.attachment_id,
            output: self.file.display().to_string(),
            bytes_written: bytes.len(),
        };
        print_output(&output, &result, || {
            println!(
                "{} action=written output={} bytes_written={}",
                result.attachment_id,
                quote_human_string(&result.output),
                result.bytes_written,
            );
        })?;
        Ok(result)
    }
}

#[argx(handler = run)]
impl ObjectAttachmentsGetCommand {
    /// Run `kival objects attachments get`.
    ///
    /// # Errors
    ///
    /// Returns an error if the attachment cannot be fetched.
    pub(crate) async fn run(
        self,
        ctx: CliContext,
        output: OutputMode,
    ) -> std::result::Result<ObjectAttachment, AttachmentReadError> {
        let client = authenticated_client(&ctx)?;
        let attachment = client
            .get_object_attachment(
                self.target.workspace_id,
                self.target.object_id,
                self.attachment_id,
            )
            .await?;
        print_output(&output, &attachment, || print_attachment_line(&attachment, None))?;
        Ok(attachment)
    }
}

/// Prints a compact attachment line.
fn print_attachment_line(attachment: &ObjectAttachment, action: Option<&str>) {
    let mut fields = vec![attachment.id.to_string()];
    if let Some(action) = action {
        fields.push(format!("action={action}"));
    }
    fields.extend([
        format!("object={}", attachment.object_id),
        format!("created={}", format_human_timestamp(attachment.created_at)),
    ]);

    push_optional_uuid_field(&mut fields, "version", attachment.version_id);
    push_optional_uuid_field(&mut fields, "source_attachment", attachment.source_attachment_id);
    if let Some(name) = &attachment.name {
        fields.push(format!("name={}", quote_human_string(name)));
    }
    if let Some(media_type) = &attachment.media_type {
        fields.push(format!("media_type={media_type}"));
    }

    println!("{}", fields.join(" "));
}
