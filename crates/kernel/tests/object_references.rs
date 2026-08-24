//! Integration tests for kernel object reference maintenance.

#[cfg(test)]
mod tests {
    use kival_kernel::{
        CreateObjectVersion, KernelError, ObjectReferenceUpdate, ObjectVersion,
        ReferenceReresolutionSummary, Result, maintain_object_references,
        re_resolve_current_wikilinks_for_titles,
    };
    use sqlx::{Postgres, Transaction};
    use uuid::Uuid;

    async fn insert_workspace(pool: &sqlx::PgPool, name: &str) -> Result<Uuid> {
        Ok(sqlx::query_scalar(
            r#"
            INSERT INTO kival.workspaces (name)
            VALUES ($1)
            RETURNING id
            "#,
        )
        .bind(name)
        .fetch_one(pool)
        .await?)
    }

    async fn insert_unversioned_object(pool: &sqlx::PgPool, workspace_id: Uuid) -> Result<Uuid> {
        Ok(sqlx::query_scalar(
            r#"
            INSERT INTO kival.objects (workspace_id)
            VALUES ($1)
            RETURNING id
            "#,
        )
        .bind(workspace_id)
        .fetch_one(pool)
        .await?)
    }

    async fn insert_version(
        tx: &mut Transaction<'_, Postgres>,
        request: CreateObjectVersion,
    ) -> Result<ObjectVersion> {
        let (id, object_id, version_number, title, body, metadata, created_by, created_at) =
            sqlx::query_as::<
                _,
                (
                    Uuid,
                    Uuid,
                    i64,
                    String,
                    String,
                    serde_json::Value,
                    Option<Uuid>,
                    time::OffsetDateTime,
                ),
            >(
                r#"
                INSERT INTO kival.object_versions (
                    object_id, version_number, title, body_text, metadata, created_by
                )
                VALUES ($1, $2, $3, $4, $5, $6)
                RETURNING
                    id, object_id, version_number, title, body_text, metadata, created_by, created_at
                "#,
            )
            .bind(request.object_id)
            .bind(request.version_number)
            .bind(request.title)
            .bind(request.body)
            .bind(request.metadata)
            .bind(request.created_by)
            .fetch_one(&mut **tx)
            .await?;

        Ok(ObjectVersion {
            id,
            object_id,
            version_number,
            title,
            body,
            metadata,
            created_by,
            created_at,
        })
    }

    async fn recompute_object_references(
        tx: &mut Transaction<'_, Postgres>,
        workspace_id: Uuid,
        source_object_id: Uuid,
        source_version_id: Uuid,
    ) -> Result<ObjectReferenceUpdate> {
        Ok(maintain_object_references(tx, workspace_id, source_object_id, source_version_id, &[])
            .await?
            .reference_update)
    }

    type ReferenceProjection = (String, Option<String>, String, Option<Uuid>, i32, i32);

    async fn project_body(
        pool: &sqlx::PgPool,
        workspace_id: Uuid,
        body: &str,
    ) -> Result<Vec<ReferenceProjection>> {
        use serde_json::json;

        let source_id = insert_unversioned_object(pool, workspace_id).await?;
        let mut tx = pool.begin().await?;
        let version = insert_version(
            &mut tx,
            CreateObjectVersion {
                object_id: source_id,
                version_number: 1,
                title: "Source".to_owned(),
                body: body.to_owned(),
                metadata: json!({}),
                created_by: None,
            },
        )
        .await?;
        set_current_version(&mut tx, source_id, version.id).await?;
        recompute_object_references(&mut tx, workspace_id, source_id, version.id).await?;

        let rows = sqlx::query_as::<_, ReferenceProjection>(
            r#"
            SELECT raw_target, display_text, reference_kind, target_object_id, span_start, span_end
            FROM kival.object_references
            WHERE source_version_id = $1
            ORDER BY span_start
            "#,
        )
        .bind(version.id)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows)
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn parses_wikilink_and_display_text(pool: sqlx::PgPool) -> Result<()> {
        let workspace_id = insert_workspace(&pool, "reference_parser_display").await?;
        let rows =
            project_body(&pool, workspace_id, "See [[Object Title]] and [[ADR-014|the decision]].")
                .await?;

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "Object Title");
        assert!(rows[0].1.is_none());
        assert_eq!(rows[0].2, "wikilink");
        assert_eq!(rows[1].0, "ADR-014");
        assert_eq!(rows[1].1.as_deref(), Some("the decision"));
        assert_eq!(rows[1].2, "wikilink");
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn trims_wikilink_targets_and_display_text(pool: sqlx::PgPool) -> Result<()> {
        let workspace_id = insert_workspace(&pool, "reference_parser_trim").await?;
        let rows = project_body(&pool, workspace_id, "See [[  Target  |  display  ]]").await?;

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "Target");
        assert_eq!(rows[0].1.as_deref(), Some("display"));
        assert_eq!(rows[0].2, "wikilink");
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn parses_supported_kival_object_links(pool: sqlx::PgPool) -> Result<()> {
        let workspace_id = insert_workspace(&pool, "reference_parser_kival_links").await?;
        let target_id = insert_object(&pool, workspace_id, "Target").await?;
        let body = format!(
            "[short](kival://objects/{target_id}) \
             [full](kival://workspaces/{workspace_id}/objects/{target_id})"
        );
        let rows = project_body(&pool, workspace_id, &body).await?;

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, format!("kival://objects/{target_id}"));
        assert_eq!(rows[0].1.as_deref(), Some("short"));
        assert_eq!(rows[0].2, "kival_object_link");
        assert_eq!(rows[0].3, Some(target_id));
        assert_eq!(rows[1].0, format!("kival://workspaces/{workspace_id}/objects/{target_id}"));
        assert_eq!(rows[1].1.as_deref(), Some("full"));
        assert_eq!(rows[1].2, "kival_object_link");
        assert_eq!(rows[1].3, Some(target_id));
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn malformed_links_are_ignored_without_hiding_valid_references(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let workspace_id = insert_workspace(&pool, "reference_parser_malformed").await?;
        for (body, expected_targets) in [
            ("[[", Vec::<&str>::new()),
            ("[[]]", vec![]),
            ("[[target", vec![]),
            ("[](kival://objects/not-a-uuid)", vec![]),
            ("[label](kival://workspaces/nope/objects/nope)", vec![]),
            ("unicode 🚀 [[valid]] trailing [broken](", vec!["valid"]),
        ] {
            let rows = project_body(&pool, workspace_id, body).await?;
            let targets = rows.iter().map(|row| row.0.as_str()).collect::<Vec<_>>();
            assert_eq!(targets, expected_targets, "unexpected references parsed from {body:?}");
        }
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn records_utf8_byte_spans(pool: sqlx::PgPool) -> Result<()> {
        let workspace_id = insert_workspace(&pool, "reference_parser_utf8").await?;
        let body = "🚀 [[Target]]";
        let rows = project_body(&pool, workspace_id, body).await?;

        assert_eq!(rows.len(), 1);
        assert_eq!(&body[rows[0].4 as usize..rows[0].5 as usize], "[[Target]]");
        Ok(())
    }

    async fn insert_object_with_body(
        pool: &sqlx::PgPool,
        workspace_id: Uuid,
        title: &str,
        body: &str,
    ) -> Result<(Uuid, Uuid)> {
        use serde_json::json;

        let object_id = insert_unversioned_object(pool, workspace_id).await?;
        let mut tx = pool.begin().await?;
        let version = insert_version(
            &mut tx,
            CreateObjectVersion {
                object_id,
                version_number: 1,
                title: title.to_owned(),
                body: body.to_owned(),
                metadata: json!({}),
                created_by: None,
            },
        )
        .await?;
        set_current_version(&mut tx, object_id, version.id).await?;
        tx.commit().await?;

        Ok((object_id, version.id))
    }

    async fn set_current_version(
        tx: &mut Transaction<'_, Postgres>,
        object_id: Uuid,
        version_id: Uuid,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE kival.objects
            SET current_version_id = $2
            WHERE id = $1
            "#,
        )
        .bind(object_id)
        .bind(version_id)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn insert_object(pool: &sqlx::PgPool, workspace_id: Uuid, title: &str) -> Result<Uuid> {
        let mut tx = pool.begin().await?;
        let object_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO kival.objects (workspace_id)
            VALUES ($1)
            RETURNING id
            "#,
        )
        .bind(workspace_id)
        .fetch_one(&mut *tx)
        .await?;
        let version_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO kival.object_versions (object_id, version_number, title)
            VALUES ($1, 1, $2)
            RETURNING id
            "#,
        )
        .bind(object_id)
        .bind(title)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            UPDATE kival.objects
            SET current_version_id = $2
            WHERE id = $1
            "#,
        )
        .bind(object_id)
        .bind(version_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(object_id)
    }

    async fn rename_object(pool: &sqlx::PgPool, object_id: Uuid, title: &str) -> Result<()> {
        let mut tx = pool.begin().await?;
        let version_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO kival.object_versions (object_id, version_number, title)
            SELECT $1, COALESCE(MAX(version_number), 0) + 1, $2
            FROM kival.object_versions
            WHERE object_id = $1
            RETURNING id
            "#,
        )
        .bind(object_id)
        .bind(title)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            UPDATE kival.objects
            SET current_version_id = $2
            WHERE id = $1
            "#,
        )
        .bind(object_id)
        .bind(version_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn recompute_resolves_and_versions_references_without_creating_edges(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        use std::time::{SystemTime, UNIX_EPOCH};

        use serde_json::json;

        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after epoch")
            .as_nanos();
        let workspace_id =
            insert_workspace(&pool, &format!("reference_workspace_{suffix}")).await?;
        let other_workspace_id =
            insert_workspace(&pool, &format!("reference_other_workspace_{suffix}")).await?;
        let source_id = insert_unversioned_object(&pool, workspace_id).await?;
        let unique_id = insert_object(&pool, workspace_id, "Unique Target").await?;
        let _duplicate_one = insert_object(&pool, workspace_id, "Duplicate Target").await?;
        let _duplicate_two = insert_object(&pool, workspace_id, "Duplicate Target").await?;
        let other_id = insert_object(&pool, other_workspace_id, "Other Target").await?;

        let body = format!(
            "[[Unique Target]] [[Duplicate Target]] [[Missing Target]] \
             [same](kival://objects/{unique_id}) \
             [cross](kival://workspaces/{other_workspace_id}/objects/{other_id})"
        );
        let mut tx = pool.begin().await?;
        let version_one = insert_version(
            &mut tx,
            CreateObjectVersion {
                object_id: source_id,
                version_number: 1,
                title: "Source".to_owned(),
                body: body.clone(),
                metadata: json!({}),
                created_by: None,
            },
        )
        .await?;
        set_current_version(&mut tx, source_id, version_one.id).await?;
        let update =
            recompute_object_references(&mut tx, workspace_id, source_id, version_one.id).await?;
        assert_eq!(
            update,
            ObjectReferenceUpdate {
                resolved_count: 2,
                unresolved_count: 2,
                ambiguous_count: 1,
                stale_count: 0
            }
        );
        let rows = sqlx::query_as::<_, (String, Option<Uuid>, String)>(
            r#"
            SELECT raw_target, target_object_id, status
            FROM kival.object_references
            WHERE source_version_id = $1
            ORDER BY span_start
            "#,
        )
        .bind(version_one.id)
        .fetch_all(&mut *tx)
        .await?;
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0], ("Unique Target".to_owned(), Some(unique_id), "resolved".to_owned()));
        assert_eq!(rows[1].2, "ambiguous");
        assert_eq!(rows[2].2, "unresolved");
        assert_eq!(rows[3].1, Some(unique_id));
        assert_eq!(rows[4].1, None);

        let version_two = insert_version(
            &mut tx,
            CreateObjectVersion {
                object_id: source_id,
                version_number: 2,
                title: "Source v2".to_owned(),
                body: "[[Unique Target]]".to_owned(),
                metadata: json!({}),
                created_by: None,
            },
        )
        .await?;
        set_current_version(&mut tx, source_id, version_two.id).await?;
        let update =
            recompute_object_references(&mut tx, workspace_id, source_id, version_two.id).await?;
        assert_eq!(
            update,
            ObjectReferenceUpdate {
                resolved_count: 1,
                unresolved_count: 0,
                ambiguous_count: 0,
                stale_count: 5
            }
        );

        let old_active_count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM kival.object_references
            WHERE source_version_id = $1
                AND status <> 'stale'
            "#,
        )
        .bind(version_one.id)
        .fetch_one(&mut *tx)
        .await?;
        assert_eq!(old_active_count, 0);

        let current_count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM kival.object_references
            WHERE source_version_id = $1
            "#,
        )
        .bind(version_two.id)
        .fetch_one(&mut *tx)
        .await?;
        assert_eq!(current_count, 1);

        let edge_count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM kival.object_edges
            WHERE source_object_id = $1
            "#,
        )
        .bind(source_id)
        .fetch_one(&mut *tx)
        .await?;
        assert_eq!(edge_count, 0);

        tx.rollback().await?;
        Ok(())
    }

    async fn insert_user(pool: &sqlx::PgPool, suffix: u128) -> Result<Uuid> {
        Ok(sqlx::query_scalar(
            r#"
            INSERT INTO kival.users (username, display_name)
            VALUES ($1, 'Reference Test User')
            RETURNING id
            "#,
        )
        .bind(format!("reference-{suffix}"))
        .fetch_one(pool)
        .await?)
    }

    async fn reference_state(
        pool: &sqlx::PgPool,
        source_version_id: Uuid,
        raw_target: &str,
    ) -> Result<(Option<Uuid>, String)> {
        Ok(sqlx::query_as(
            r#"
            SELECT target_object_id, status
            FROM kival.object_references
            WHERE source_version_id = $1
                AND raw_target = $2
            "#,
        )
        .bind(source_version_id)
        .bind(raw_target)
        .fetch_one(pool)
        .await?)
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn wikilink_reresolution_tracks_namespace_lifecycle_and_preserves_stale_links(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        use std::time::{SystemTime, UNIX_EPOCH};

        use serde_json::json;

        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after epoch")
            .as_nanos();
        let actor_id = insert_user(&pool, suffix).await?;
        let workspace_id =
            insert_workspace(&pool, &format!("reresolution_workspace_{suffix}")).await?;
        let source_id = insert_unversioned_object(&pool, workspace_id).await?;
        let stable_target_id = insert_object(&pool, workspace_id, "Stable Target").await?;
        let initial_body =
            format!("[[Dynamic Target]] [stable](kival://objects/{stable_target_id})");

        let mut tx = pool.begin().await?;
        let version_one = insert_version(
            &mut tx,
            CreateObjectVersion {
                object_id: source_id,
                version_number: 1,
                title: "Source".to_owned(),
                body: initial_body.clone(),
                metadata: json!({}),
                created_by: None,
            },
        )
        .await?;
        set_current_version(&mut tx, source_id, version_one.id).await?;
        recompute_object_references(&mut tx, workspace_id, source_id, version_one.id).await?;
        tx.commit().await?;
        assert_eq!(
            reference_state(&pool, version_one.id, "Dynamic Target").await?,
            (None, "unresolved".to_owned())
        );

        let target_id = insert_object(&pool, workspace_id, "Dynamic Target").await?;
        let titles = vec!["Dynamic Target".to_owned()];
        let mut tx = pool.begin().await?;
        let summary =
            re_resolve_current_wikilinks_for_titles(&mut tx, workspace_id, &titles).await?;
        tx.commit().await?;
        assert_eq!(
            summary,
            ReferenceReresolutionSummary {
                updated_count: 1,
                resolved_count: 1,
                unresolved_count: 0,
                ambiguous_count: 0,
            }
        );
        assert_eq!(
            reference_state(&pool, version_one.id, "Dynamic Target").await?,
            (Some(target_id), "resolved".to_owned())
        );

        let duplicate_id = insert_object(&pool, workspace_id, "Dynamic Target").await?;
        let mut tx = pool.begin().await?;
        let summary =
            re_resolve_current_wikilinks_for_titles(&mut tx, workspace_id, &titles).await?;
        tx.commit().await?;
        assert_eq!(summary.ambiguous_count, 1);
        assert_eq!(
            reference_state(&pool, version_one.id, "Dynamic Target").await?,
            (None, "ambiguous".to_owned())
        );

        sqlx::query(
            r#"
            UPDATE kival.objects
            SET status = 'archived', archived_at = now(), archived_by = $2
            WHERE id = $1
            "#,
        )
        .bind(duplicate_id)
        .bind(actor_id)
        .execute(&pool)
        .await?;
        let mut tx = pool.begin().await?;
        let summary =
            re_resolve_current_wikilinks_for_titles(&mut tx, workspace_id, &titles).await?;
        tx.commit().await?;
        assert_eq!(summary.resolved_count, 1);
        assert_eq!(
            reference_state(&pool, version_one.id, "Dynamic Target").await?,
            (Some(target_id), "resolved".to_owned())
        );

        sqlx::query(
            r#"
            UPDATE kival.objects
            SET status = 'archived', archived_at = now(), archived_by = $2
            WHERE id = $1
            "#,
        )
        .bind(target_id)
        .bind(actor_id)
        .execute(&pool)
        .await?;
        let mut tx = pool.begin().await?;
        let summary =
            re_resolve_current_wikilinks_for_titles(&mut tx, workspace_id, &titles).await?;
        tx.commit().await?;
        assert_eq!(summary.unresolved_count, 1);
        assert_eq!(
            reference_state(&pool, version_one.id, "Dynamic Target").await?,
            (None, "unresolved".to_owned())
        );

        sqlx::query(
            r#"
            UPDATE kival.objects
            SET status = 'active', archived_at = NULL, archived_by = NULL
            WHERE id = $1
            "#,
        )
        .bind(target_id)
        .execute(&pool)
        .await?;
        let mut tx = pool.begin().await?;
        let summary =
            re_resolve_current_wikilinks_for_titles(&mut tx, workspace_id, &titles).await?;
        tx.commit().await?;
        assert_eq!(summary.resolved_count, 1);

        rename_object(&pool, target_id, "Renamed Away").await?;
        let rename_titles = vec!["Dynamic Target".to_owned(), "Renamed Away".to_owned()];
        let mut tx = pool.begin().await?;
        let summary =
            re_resolve_current_wikilinks_for_titles(&mut tx, workspace_id, &rename_titles).await?;
        tx.commit().await?;
        assert_eq!(summary.unresolved_count, 1);

        let replacement_id = insert_object(&pool, workspace_id, "Replacement").await?;
        rename_object(&pool, replacement_id, "Dynamic Target").await?;
        let replacement_titles = vec!["Replacement".to_owned(), "Dynamic Target".to_owned()];
        let mut tx = pool.begin().await?;
        let summary =
            re_resolve_current_wikilinks_for_titles(&mut tx, workspace_id, &replacement_titles)
                .await?;
        tx.commit().await?;
        assert_eq!(summary.resolved_count, 1);
        assert_eq!(
            reference_state(&pool, version_one.id, "Dynamic Target").await?,
            (Some(replacement_id), "resolved".to_owned())
        );

        let mut tx = pool.begin().await?;
        let version_two = insert_version(
            &mut tx,
            CreateObjectVersion {
                object_id: source_id,
                version_number: 2,
                title: "Source v2".to_owned(),
                body: format!("[stable](kival://objects/{stable_target_id})"),
                metadata: json!({}),
                created_by: None,
            },
        )
        .await?;
        set_current_version(&mut tx, source_id, version_two.id).await?;
        recompute_object_references(&mut tx, workspace_id, source_id, version_two.id).await?;
        tx.commit().await?;

        let stale_before = reference_state(&pool, version_one.id, "Dynamic Target").await?;
        let stable_before =
            reference_state(&pool, version_two.id, &format!("kival://objects/{stable_target_id}"))
                .await?;
        let mut tx = pool.begin().await?;
        let summary =
            re_resolve_current_wikilinks_for_titles(&mut tx, workspace_id, &titles).await?;
        tx.commit().await?;
        assert_eq!(summary.updated_count, 0);
        assert_eq!(reference_state(&pool, version_one.id, "Dynamic Target").await?, stale_before);
        assert_eq!(
            reference_state(&pool, version_two.id, &format!("kival://objects/{stable_target_id}"),)
                .await?,
            stable_before
        );

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn recompute_requires_scoped_source_version(pool: sqlx::PgPool) -> Result<()> {
        use serde_json::json;

        let workspace_id = insert_workspace(&pool, "recompute_scope_workspace").await?;
        let foreign_workspace_id = insert_workspace(&pool, "recompute_scope_foreign").await?;
        let source_id = insert_unversioned_object(&pool, workspace_id).await?;
        let target_id = insert_object(&pool, workspace_id, "Scoped Target").await?;

        let mut seed_tx = pool.begin().await?;
        let version = insert_version(
            &mut seed_tx,
            CreateObjectVersion {
                object_id: source_id,
                version_number: 1,
                title: "Scoped Source".to_owned(),
                body: "[[Scoped Target]]".to_owned(),
                metadata: json!({}),
                created_by: None,
            },
        )
        .await?;
        set_current_version(&mut seed_tx, source_id, version.id).await?;
        recompute_object_references(&mut seed_tx, workspace_id, source_id, version.id).await?;
        seed_tx.commit().await?;

        let foreign_source_id = insert_unversioned_object(&pool, workspace_id).await?;
        let mut foreign_tx = pool.begin().await?;
        let foreign_version = insert_version(
            &mut foreign_tx,
            CreateObjectVersion {
                object_id: foreign_source_id,
                version_number: 1,
                title: "Foreign Source".to_owned(),
                body: String::new(),
                metadata: json!({}),
                created_by: None,
            },
        )
        .await?;
        foreign_tx.commit().await?;

        let mut tx = pool.begin().await?;
        let error =
            recompute_object_references(&mut tx, foreign_workspace_id, source_id, version.id)
                .await
                .expect_err("foreign workspace must reject source projection recomputation");
        assert!(matches!(error, KernelError::ResourceNotFound));

        let error =
            recompute_object_references(&mut tx, workspace_id, source_id, foreign_version.id)
                .await
                .expect_err("foreign source version must reject projection recomputation");
        assert!(matches!(error, KernelError::ResourceNotFound));
        tx.commit().await?;

        assert_eq!(
            reference_state(&pool, version.id, "Scoped Target").await?,
            (Some(target_id), "resolved".to_owned())
        );

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn recompute_requires_current_version_and_projects_stored_body(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        use serde_json::json;

        let workspace_id = insert_workspace(&pool, "recompute_current_workspace").await?;
        let source_id = insert_unversioned_object(&pool, workspace_id).await?;
        let old_target = insert_object(&pool, workspace_id, "Old Target").await?;
        let new_target = insert_object(&pool, workspace_id, "New Target").await?;

        let mut tx = pool.begin().await?;
        let version_one = insert_version(
            &mut tx,
            CreateObjectVersion {
                object_id: source_id,
                version_number: 1,
                title: "Source".to_owned(),
                body: "[[Old Target]]".to_owned(),
                metadata: json!({}),
                created_by: None,
            },
        )
        .await?;
        set_current_version(&mut tx, source_id, version_one.id).await?;
        recompute_object_references(&mut tx, workspace_id, source_id, version_one.id).await?;

        let version_two = insert_version(
            &mut tx,
            CreateObjectVersion {
                object_id: source_id,
                version_number: 2,
                title: "Source v2".to_owned(),
                body: "[[New Target]]".to_owned(),
                metadata: json!({}),
                created_by: None,
            },
        )
        .await?;
        set_current_version(&mut tx, source_id, version_two.id).await?;
        recompute_object_references(&mut tx, workspace_id, source_id, version_two.id).await?;

        let error = recompute_object_references(&mut tx, workspace_id, source_id, version_one.id)
            .await
            .expect_err("historical source version must not replace the current projection");
        assert!(matches!(error, KernelError::ResourceNotFound));
        tx.commit().await?;

        assert_eq!(
            reference_state(&pool, version_one.id, "Old Target").await?,
            (Some(old_target), "stale".to_owned())
        );
        assert_eq!(
            reference_state(&pool, version_two.id, "New Target").await?,
            (Some(new_target), "resolved".to_owned())
        );

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn reference_maintenance_orders_overlapping_titles_but_keeps_disjoint_concurrency(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let workspace_id = insert_workspace(&pool, "reference_title_lock_workspace").await?;
        let (object_a, version_a) =
            insert_object_with_body(&pool, workspace_id, "A", "[[B]]").await?;
        let (object_b, version_b) =
            insert_object_with_body(&pool, workspace_id, "B", "[[A]]").await?;
        let (object_c, version_c) =
            insert_object_with_body(&pool, workspace_id, "C", "[[D]]").await?;
        let (object_e, version_e) =
            insert_object_with_body(&pool, workspace_id, "E", "[[F]]").await?;

        let mut first = pool.begin().await?;
        maintain_object_references(
            &mut first,
            workspace_id,
            object_a,
            version_a,
            &["A".to_owned()],
        )
        .await?;

        let mut overlapping = pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '100ms'").execute(&mut *overlapping).await?;
        let error = maintain_object_references(
            &mut overlapping,
            workspace_id,
            object_b,
            version_b,
            &["B".to_owned()],
        )
        .await
        .expect_err("cross-linked maintenance must wait on the shared ordered title set");
        let KernelError::Database(sqlx::Error::Database(database)) = error else {
            panic!("expected PostgreSQL lock timeout");
        };
        assert_eq!(database.code().as_deref(), Some("55P03"));
        overlapping.rollback().await?;
        first.commit().await?;

        let mut first_disjoint = pool.begin().await?;
        maintain_object_references(
            &mut first_disjoint,
            workspace_id,
            object_c,
            version_c,
            &["C".to_owned()],
        )
        .await?;

        let mut second_disjoint = pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '100ms'").execute(&mut *second_disjoint).await?;
        maintain_object_references(
            &mut second_disjoint,
            workspace_id,
            object_e,
            version_e,
            &["E".to_owned()],
        )
        .await?;
        second_disjoint.commit().await?;
        first_disjoint.commit().await?;

        assert_eq!(
            reference_state(&pool, version_a, "B").await?,
            (Some(object_b), "resolved".to_owned())
        );
        assert_eq!(reference_state(&pool, version_e, "F").await?, (None, "unresolved".to_owned()));

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn recompute_rolls_back_partial_projection_on_insert_failure(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        use serde_json::json;

        let workspace_id = insert_workspace(&pool, "recompute_savepoint_workspace").await?;
        let source_id = insert_unversioned_object(&pool, workspace_id).await?;
        let mut tx = pool.begin().await?;
        let version = insert_version(
            &mut tx,
            CreateObjectVersion {
                object_id: source_id,
                version_number: 1,
                title: "Source".to_owned(),
                body: "[[Good]] [[Fail]]".to_owned(),
                metadata: json!({}),
                created_by: None,
            },
        )
        .await?;
        set_current_version(&mut tx, source_id, version.id).await?;
        sqlx::query(
            r#"
            INSERT INTO kival.object_references (
                workspace_id, source_object_id, source_version_id, raw_target, reference_kind,
                span_start, span_end, status
            )
            VALUES ($1, $2, $3, 'Original', 'wikilink', 0, 12, 'unresolved')
            "#,
        )
        .bind(workspace_id)
        .bind(source_id)
        .bind(version.id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        // The first new reference starts at byte 0 and succeeds. The second starts at byte 9 and
        // fails this constraint, forcing an error after the projection has already been modified.
        sqlx::query(
            r#"
            ALTER TABLE kival.object_references
            ADD CONSTRAINT object_references_test_fail_second_insert
            CHECK (span_start < 9) NOT VALID
            "#,
        )
        .execute(&pool)
        .await?;

        let mut tx = pool.begin().await?;
        let error = recompute_object_references(&mut tx, workspace_id, source_id, version.id)
            .await
            .expect_err("second reference insert must fail");
        assert!(matches!(error, KernelError::Database(_)));

        // A caller that catches the error can still commit without preserving the partial rebuild.
        tx.commit().await?;

        let targets = sqlx::query_scalar::<_, String>(
            r#"
            SELECT raw_target
            FROM kival.object_references
            WHERE source_version_id = $1
            ORDER BY span_start
            "#,
        )
        .bind(version.id)
        .fetch_all(&pool)
        .await?;
        assert_eq!(targets, vec!["Original".to_owned()]);

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn reresolution_rolls_back_earlier_titles_when_later_title_fails(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        use serde_json::json;

        let workspace_id = insert_workspace(&pool, "reresolve_savepoint_workspace").await?;
        let source_id = insert_unversioned_object(&pool, workspace_id).await?;
        let mut tx = pool.begin().await?;
        let version = insert_version(
            &mut tx,
            CreateObjectVersion {
                object_id: source_id,
                version_number: 1,
                title: "Source".to_owned(),
                body: "[[Alpha]] [[Beta]]".to_owned(),
                metadata: json!({}),
                created_by: None,
            },
        )
        .await?;
        set_current_version(&mut tx, source_id, version.id).await?;
        recompute_object_references(&mut tx, workspace_id, source_id, version.id).await?;
        tx.commit().await?;

        let alpha_id = insert_object(&pool, workspace_id, "Alpha").await?;
        let beta_id = insert_object(&pool, workspace_id, "Beta").await?;
        assert_ne!(alpha_id, beta_id);

        // Existing unresolved Beta rows satisfy this check. Resolving Beta fails after Alpha has
        // already been updated, proving the whole multi-title transition rolls back together.
        sqlx::query(
            r#"
            ALTER TABLE kival.object_references
            ADD CONSTRAINT object_references_test_fail_beta_resolution
            CHECK (raw_target <> 'Beta' OR target_object_id IS NULL) NOT VALID
            "#,
        )
        .execute(&pool)
        .await?;

        let titles = vec!["Alpha".to_owned(), "Beta".to_owned()];
        let mut tx = pool.begin().await?;
        let error = re_resolve_current_wikilinks_for_titles(&mut tx, workspace_id, &titles)
            .await
            .expect_err("Beta resolution must fail after Alpha is processed");
        assert!(matches!(error, KernelError::Database(_)));
        tx.commit().await?;

        assert_eq!(
            reference_state(&pool, version.id, "Alpha").await?,
            (None, "unresolved".to_owned())
        );
        assert_eq!(
            reference_state(&pool, version.id, "Beta").await?,
            (None, "unresolved".to_owned())
        );

        Ok(())
    }
}
