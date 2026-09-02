//! Attachment API scenario tests.

#[cfg(test)]
mod tests {
    use axum::{
        body::to_bytes,
        http::{
            Method, StatusCode,
            header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE, ETAG},
        },
    };
    use eyre::Result;
    use kival_sdk::{GrantPrincipal, MembershipRole, ObjectRole, ReuseObjectAttachmentRequest};
    use kival_tests::{
        TestFixtureExt, TestKival, TestRawResponseExt, TestResponseExt, object_metadata, test_body,
    };
    use serde_json::{Value, json};

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn attachment_upload_returns_public_content_metadata_without_storage_paths(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let space = r.object_space("attachments").await?;
        let workspace = space.workspace;

        let object = r
            .create_object(
                workspace.id,
                "Attachment Target",
                &test_body("Attachment Target", "Target body."),
                object_metadata("target"),
            )
            .await?;

        let path = format!(
            "/workspaces/{}/objects/{}/attachments/upload?name=source.txt&media_type=text%2Fplain&metadata=%7B%22kind%22%3A%22upload%22%7D",
            workspace.id, object.id,
        );

        let response: Value = r
            .request_bytes_as(
                &r.admin,
                Method::POST,
                &path,
                b"hello attachment".to_vec(),
                Some("text/plain"),
            )
            .await?
            .into_success()?;

        let attachment = response.get("attachment").expect("response should contain attachment");

        assert_eq!(attachment["object_id"], object.id.to_string());
        assert_eq!(attachment["name"], "source.txt");
        assert_eq!(attachment["media_type"], "text/plain");
        assert_eq!(attachment["metadata"], json!({ "kind": "upload" }));
        assert_eq!(attachment["size_bytes"], 16);
        assert_eq!(
            attachment["content_ref"].as_str().map(str::len),
            Some(64),
            "content reference must be a canonical SHA-256 digest",
        );
        assert!(
            attachment.get("blob_ref").is_none(),
            "public attachment response must not expose the storage column name",
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn attachment_upload_and_download_stream_beyond_default_json_limit(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("large streamed attachment").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Large Attachment Target",
                &test_body("Large Attachment Target", "Body."),
                object_metadata("large-attachment-target"),
            )
            .await?;
        let expected = vec![0x5a; 2 * 1024 * 1024];

        let attachment = r
            .upload_attachment(
                space.workspace.id,
                object.id,
                None,
                Some("large.bin"),
                Some("application/octet-stream"),
                json!({}),
                expected.clone(),
            )
            .await?;

        assert_eq!(attachment.size_bytes, expected.len() as u64);
        assert_eq!(attachment.content_ref.len(), 64);

        let response = r
            .request(
                Some(&r.admin),
                Method::GET,
                &format!(
                    "/workspaces/{}/objects/{}/attachments/{}/content",
                    space.workspace.id, object.id, attachment.id,
                ),
                None,
            )
            .await?;

        assert_eq!(response.status(), StatusCode::OK);
        let expected_length = expected.len().to_string();
        assert_eq!(
            response.headers().get(CONTENT_LENGTH).and_then(|value| value.to_str().ok()),
            Some(expected_length.as_str()),
        );
        assert!(response.headers().get(ETAG).is_some());
        let body = to_bytes(response.into_body(), expected.len() + 1).await?;
        assert_eq!(body.as_ref(), expected.as_slice());

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn attachment_upload_rejects_nested_metadata(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("attachment nested metadata").await?;
        let object = r
            .create_object(
                space.workspace.id,
                "Attachment Nested Metadata Target",
                &test_body("Attachment Nested Metadata Target", "Body."),
                object_metadata("attachment-nested-metadata-target"),
            )
            .await?;

        let path = format!(
            "/workspaces/{}/objects/{}/attachments/upload?metadata=%7B%22config%22%3A%7B%22enabled%22%3Atrue%7D%7D",
            space.workspace.id, object.id,
        );
        let response = r
            .request_bytes(
                Some(&r.admin),
                Method::POST,
                &path,
                b"nested metadata".to_vec(),
                Some("text/plain"),
            )
            .await?;

        response.assert_status(StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn attachment_content_returns_uploaded_bytes_and_safe_image_headers(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let space = r.object_space("attachment content").await?;
        let workspace = space.workspace;

        let object = r
            .create_object(
                workspace.id,
                "Attachment Content Target",
                &test_body("Attachment Content Target", "Target body."),
                object_metadata("attachment-content-target"),
            )
            .await?;

        let expected = b"not-a-real-png-but-valid-test-bytes".to_vec();
        let attachment = r
            .upload_attachment(
                workspace.id,
                object.id,
                None,
                Some("image.png"),
                Some("image/png"),
                json!({}),
                expected.clone(),
            )
            .await?;

        let response = r
            .request(
                Some(&r.admin),
                Method::GET,
                &format!(
                    "/workspaces/{}/objects/{}/attachments/{}/content",
                    workspace.id, object.id, attachment.id,
                ),
                None,
            )
            .await?;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok()),
            Some("image/png"),
        );
        assert_eq!(
            response.headers().get(CONTENT_DISPOSITION).and_then(|value| value.to_str().ok()),
            Some("inline"),
        );
        assert_eq!(
            response.headers().get(CACHE_CONTROL).and_then(|value| value.to_str().ok()),
            Some("private, no-store"),
        );

        let body = to_bytes(response.into_body(), usize::MAX).await?;
        assert_eq!(body.as_ref(), expected.as_slice());

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn attachment_content_forces_non_image_content_to_download(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let space = r.object_space("attachment content download").await?;
        let workspace = space.workspace;

        let object = r
            .create_object(
                workspace.id,
                "Attachment Download Target",
                &test_body("Attachment Download Target", "Target body."),
                object_metadata("attachment-download-target"),
            )
            .await?;

        let attachment = r
            .upload_attachment(
                workspace.id,
                object.id,
                None,
                Some("page.html"),
                Some("text/html"),
                json!({}),
                b"<script>alert('nope')</script>".to_vec(),
            )
            .await?;

        let response = r
            .request(
                Some(&r.admin),
                Method::GET,
                &format!(
                    "/workspaces/{}/objects/{}/attachments/{}/content",
                    workspace.id, object.id, attachment.id,
                ),
                None,
            )
            .await?;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok()),
            Some("text/html"),
        );
        assert_eq!(
            response.headers().get(CONTENT_DISPOSITION).and_then(|value| value.to_str().ok()),
            Some("attachment"),
        );

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn attachment_content_requires_object_view_permission(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let workspace = r.create_workspace("attachment content auth").await?;
        let reader = r.create_user("attachment-content-reader").await?;
        r.add_user_to_workspace(workspace.id, reader.id, MembershipRole::Member).await?;

        let object = r
            .create_object(
                workspace.id,
                "Private Attachment Target",
                &test_body("Private Attachment Target", "Target body."),
                object_metadata("private-attachment-target"),
            )
            .await?;

        let attachment = r
            .upload_attachment(
                workspace.id,
                object.id,
                None,
                Some("private.png"),
                Some("image/png"),
                json!({}),
                b"private bytes".to_vec(),
            )
            .await?;

        let path = format!(
            "/workspaces/{}/objects/{}/attachments/{}/content",
            workspace.id, object.id, attachment.id,
        );

        let denied = r.request(Some(&reader), Method::GET, &path, None).await?;
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);

        r.create_object_grant(
            workspace.id,
            object.id,
            GrantPrincipal::User(reader.id),
            ObjectRole::Viewer,
        )
        .await?;

        let allowed = r.request(Some(&reader), Method::GET, &path, None).await?;
        assert_eq!(allowed.status(), StatusCode::OK);
        let body = to_bytes(allowed.into_body(), usize::MAX).await?;
        assert_eq!(body.as_ref(), b"private bytes");

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn attachment_reuse_copies_metadata_and_sets_provenance(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let space = r.object_space("attachments reuse").await?;
        let workspace = space.workspace;

        let source_object = r
            .create_object(
                workspace.id,
                "Source Object",
                &test_body("Source Object", "Source body."),
                object_metadata("source-object"),
            )
            .await?;

        let target_object = r
            .create_object(
                workspace.id,
                "Target Object",
                &test_body("Target Object", "Target body."),
                object_metadata("target-object"),
            )
            .await?;

        let source_attachment = r
            .upload_attachment(
                workspace.id,
                source_object.id,
                None,
                Some("source.txt"),
                Some("text/plain"),
                json!({ "kind": "source-attachment", "copy": true }),
                b"shared bytes".to_vec(),
            )
            .await?;

        let reused_attachment = r
            .reuse_attachment_as(
                &r.admin,
                workspace.id,
                target_object.id,
                source_attachment.id,
                None,
            )
            .await?;

        assert_ne!(reused_attachment.id, source_attachment.id);
        assert_eq!(reused_attachment.object_id, target_object.id);
        assert_eq!(reused_attachment.source_attachment_id, Some(source_attachment.id));
        assert_eq!(reused_attachment.name.as_deref(), Some("source.txt"));
        assert_eq!(reused_attachment.media_type.as_deref(), Some("text/plain"));
        assert_eq!(reused_attachment.metadata, source_attachment.metadata);

        let fetched = r
            .get_attachment_as(&r.admin, workspace.id, target_object.id, reused_attachment.id)
            .await?;

        assert_eq!(fetched, reused_attachment);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn attachment_can_be_reused_on_the_same_object(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("same object attachment reuse").await?;
        let object = r
            .create_object(
                workspace.id,
                "Same Object Attachment Reuse",
                &test_body("Same Object Attachment Reuse", "Body."),
                object_metadata("same-object-attachment-reuse"),
            )
            .await?;

        let source = r
            .upload_attachment(
                workspace.id,
                object.id,
                None,
                Some("source.txt"),
                Some("text/plain"),
                json!({ "kind": "same-object-source" }),
                b"same object bytes".to_vec(),
            )
            .await?;

        let reused =
            r.reuse_attachment_as(&r.admin, workspace.id, object.id, source.id, None).await?;

        assert_ne!(reused.id, source.id);
        assert_eq!(reused.object_id, object.id);
        assert_eq!(reused.source_attachment_id, Some(source.id));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn attachment_reuse_requires_source_inspect(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let workspace = r.create_workspace("attachments source auth").await?;
        let source_group = r.create_group("source editors").await?;
        let target_group = r.create_group("target editors").await?;

        r.add_group_to_workspace(workspace.id, source_group.id).await?;
        r.add_group_to_workspace(workspace.id, target_group.id).await?;

        let reader = r.create_user("attachment-source-reader").await?;
        r.add_user_to_workspace(workspace.id, reader.id, MembershipRole::Member).await?;
        r.add_user_to_group(target_group.id, reader.id, MembershipRole::Member).await?;

        let source_object = r
            .create_object(
                workspace.id,
                "Hidden Source Object",
                &test_body("Hidden Source Object", "Hidden source body."),
                object_metadata("source-object"),
            )
            .await?;

        let target_object = r
            .create_object(
                workspace.id,
                "Editable Target Object",
                &test_body("Editable Target Object", "Target body."),
                object_metadata("target-object"),
            )
            .await?;

        r.create_object_grant(
            workspace.id,
            target_object.id,
            GrantPrincipal::Group(target_group.id),
            ObjectRole::Editor,
        )
        .await?;

        let source_attachment = r
            .upload_attachment(
                workspace.id,
                source_object.id,
                None,
                Some("hidden.txt"),
                Some("text/plain"),
                json!({ "kind": "hidden-source" }),
                b"hidden bytes".to_vec(),
            )
            .await?;

        let response = r
            .request(
                Some(&reader),
                Method::POST,
                &format!(
                    "/workspaces/{}/objects/{}/attachments/reuse",
                    workspace.id, target_object.id,
                ),
                Some(json!(ReuseObjectAttachmentRequest {
                    source_attachment_id: source_attachment.id,
                    version_id: None,
                })),
            )
            .await?;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn attachment_reuse_requires_target_edit(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let workspace = r.create_workspace("attachments target auth").await?;
        let source_group = r.create_group("source editors").await?;
        let target_group = r.create_group("target viewers").await?;

        r.add_group_to_workspace(workspace.id, source_group.id).await?;
        r.add_group_to_workspace(workspace.id, target_group.id).await?;

        let reader = r.create_user("attachment-target-reader").await?;
        r.add_user_to_workspace(workspace.id, reader.id, MembershipRole::Member).await?;
        r.add_user_to_group(source_group.id, reader.id, MembershipRole::Member).await?;
        r.add_user_to_group(target_group.id, reader.id, MembershipRole::Member).await?;

        let source_object = r
            .create_object(
                workspace.id,
                "Readable Source Object",
                &test_body("Readable Source Object", "Source body."),
                object_metadata("source-object"),
            )
            .await?;

        let target_object = r
            .create_object(
                workspace.id,
                "Read Only Target Object",
                &test_body("Read Only Target Object", "Target body."),
                object_metadata("target-object"),
            )
            .await?;

        r.create_object_grant(
            workspace.id,
            source_object.id,
            GrantPrincipal::Group(source_group.id),
            ObjectRole::Viewer,
        )
        .await?;

        r.create_object_grant(
            workspace.id,
            target_object.id,
            GrantPrincipal::Group(target_group.id),
            ObjectRole::Viewer,
        )
        .await?;

        let source_attachment = r
            .upload_attachment(
                workspace.id,
                source_object.id,
                None,
                Some("source.txt"),
                Some("text/plain"),
                json!({ "kind": "source" }),
                b"source bytes".to_vec(),
            )
            .await?;

        let response = r
            .request(
                Some(&reader),
                Method::POST,
                &format!(
                    "/workspaces/{}/objects/{}/attachments/reuse",
                    workspace.id, target_object.id,
                ),
                Some(json!(ReuseObjectAttachmentRequest {
                    source_attachment_id: source_attachment.id,
                    version_id: None,
                })),
            )
            .await?;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn attachment_reuse_rejects_version_from_other_object(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let space = r.object_space("attachments wrong version").await?;
        let workspace = space.workspace;

        let source_object = r
            .create_object(
                workspace.id,
                "Source Object",
                &test_body("Source Object", "Source body."),
                object_metadata("source-object"),
            )
            .await?;

        let target_object = r
            .create_object(
                workspace.id,
                "Target Object",
                &test_body("Target Object", "Target body."),
                object_metadata("target-object"),
            )
            .await?;

        let other_object = r
            .create_object(
                workspace.id,
                "Other Object",
                &test_body("Other Object", "Other body."),
                object_metadata("other-object"),
            )
            .await?;

        let source_attachment = r
            .upload_attachment(
                workspace.id,
                source_object.id,
                None,
                Some("source.txt"),
                Some("text/plain"),
                json!({ "kind": "source" }),
                b"source bytes".to_vec(),
            )
            .await?;

        let response = r
            .request(
                Some(&r.admin),
                Method::POST,
                &format!(
                    "/workspaces/{}/objects/{}/attachments/reuse",
                    workspace.id, target_object.id,
                ),
                Some(json!(ReuseObjectAttachmentRequest {
                    source_attachment_id: source_attachment.id,
                    version_id: other_object.current_version_id,
                })),
            )
            .await?;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn archived_object_admin_can_list_and_get_attachments(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("archived attachment reads").await?;
        let object_admin = r.create_user("archived-attachment-admin").await?;
        r.add_user_to_workspace(space.workspace.id, object_admin.id, MembershipRole::Member)
            .await?;

        let object = r
            .create_object(
                space.workspace.id,
                "Archived Attachment Object",
                &test_body("Archived Attachment Object", "Body."),
                object_metadata("archived-attachment-object"),
            )
            .await?;
        r.create_object_grant(
            space.workspace.id,
            object.id,
            GrantPrincipal::User(object_admin.id),
            ObjectRole::Admin,
        )
        .await?;

        let attachment = r
            .upload_attachment(
                space.workspace.id,
                object.id,
                object.current_version_id,
                Some("archived.txt"),
                Some("text/plain"),
                json!({ "kind": "archived-read" }),
                b"archived bytes".to_vec(),
            )
            .await?;
        r.archive_object(space.workspace.id, object.id).await?;

        let listed = r.list_attachments_as(&object_admin, space.workspace.id, object.id).await?;
        assert!(listed.items.iter().any(|item| item.id == attachment.id));

        let fetched = r
            .get_attachment_as(&object_admin, space.workspace.id, object.id, attachment.id)
            .await?;
        assert_eq!(fetched.id, attachment.id);
        assert_eq!(fetched.object_id, object.id);
        assert_eq!(fetched.version_id, object.current_version_id);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn archived_object_editor_cannot_list_attachments(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let space = r.object_space("archived attachment editor denial").await?;
        let object_editor = r.create_user("archived-attachment-editor").await?;
        r.add_user_to_workspace(space.workspace.id, object_editor.id, MembershipRole::Member)
            .await?;

        let object = r
            .create_object(
                space.workspace.id,
                "Archived Attachment Editor Object",
                &test_body("Archived Attachment Editor Object", "Body."),
                object_metadata("archived-attachment-editor-object"),
            )
            .await?;
        r.create_object_grant(
            space.workspace.id,
            object.id,
            GrantPrincipal::User(object_editor.id),
            ObjectRole::Editor,
        )
        .await?;
        r.upload_attachment(
            space.workspace.id,
            object.id,
            object.current_version_id,
            Some("editor-hidden.txt"),
            Some("text/plain"),
            json!({ "kind": "editor-hidden" }),
            b"hidden archived bytes".to_vec(),
        )
        .await?;
        r.archive_object(space.workspace.id, object.id).await?;

        let response = r
            .request(
                Some(&object_editor),
                Method::GET,
                &format!("/workspaces/{}/objects/{}/attachments", space.workspace.id, object.id,),
                None,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        Ok(())
    }
}
