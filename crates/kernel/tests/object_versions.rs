//! Integration tests for kernel object version transitions.

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use kival_kernel::{
        CreateObjectVersion, KernelError, ObjectVersion, Result, UpdateObjectVersion,
        fetch_object_version, fetch_object_version_by_number,
        fetch_object_version_creator_for_mutation, list_object_version_creators,
        update_object_version,
    };
    use serde_json::json;
    use sqlx::PgPool;
    use uuid::Uuid;

    fn unique_name(prefix: &str) -> String {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after epoch")
            .as_nanos();
        format!("{prefix}_{suffix}")
    }

    async fn insert_version(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
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
                    chrono::DateTime<chrono::Utc>,
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

    async fn insert_object(pool: &PgPool) -> Result<Uuid> {
        let workspace_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO kival.workspaces (name)
            VALUES ($1)
            RETURNING id
            "#,
        )
        .bind(unique_name("workspace"))
        .fetch_one(pool)
        .await?;

        let object_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO kival.objects (workspace_id)
            VALUES ($1)
            RETURNING id
            "#,
        )
        .bind(workspace_id)
        .fetch_one(pool)
        .await?;

        Ok(object_id)
    }

    async fn insert_object_reader(pool: &PgPool, object_id: Uuid) -> Result<(Uuid, Uuid)> {
        let workspace_id =
            sqlx::query_scalar::<_, Uuid>("SELECT workspace_id FROM kival.objects WHERE id = $1")
                .bind(object_id)
                .fetch_one(pool)
                .await?;
        let actor_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO kival.users (username, display_name)
            VALUES ($1, 'Version Reader')
            RETURNING id
            "#,
        )
        .bind(format!("vread-{}", &Uuid::now_v7().simple().to_string()[..12]))
        .fetch_one(pool)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO kival.workspace_memberships (
                workspace_id, user_id, workspace_role, created_by
            )
            VALUES ($1, $2, 'admin', $2)
            "#,
        )
        .bind(workspace_id)
        .bind(actor_id)
        .execute(pool)
        .await?;

        Ok((actor_id, workspace_id))
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn creator_hydration_protects_reads_but_trusts_admitted_mutations(
        pool: PgPool,
    ) -> Result<()> {
        let object_id = insert_object(&pool).await?;
        let (actor_id, workspace_id) = insert_object_reader(&pool, object_id).await?;
        let mut tx = pool.begin().await?;
        let version = insert_version(
            &mut tx,
            CreateObjectVersion {
                object_id,
                version_number: 1,
                title: "Protected creator".to_owned(),
                body: "Body".to_owned(),
                metadata: json!({}),
                created_by: Some(actor_id),
            },
        )
        .await?;
        tx.commit().await?;

        let other_object_id = insert_object(&pool).await?;
        let mut tx = pool.begin().await?;
        let other_version = insert_version(
            &mut tx,
            CreateObjectVersion {
                object_id: other_object_id,
                version_number: 1,
                title: "Other creator".to_owned(),
                body: "Body".to_owned(),
                metadata: json!({}),
                created_by: Some(actor_id),
            },
        )
        .await?;
        tx.commit().await?;

        let creators = list_object_version_creators(
            &pool,
            actor_id,
            workspace_id,
            object_id,
            &[version.id, other_version.id],
        )
        .await?;
        assert_eq!(creators.len(), 1);
        assert_eq!(creators[0].version_id, version.id);

        sqlx::query(
            r#"
            UPDATE kival.workspace_memberships
            SET revoked_at = now(),
                revoked_by = $2
            WHERE workspace_id = $1
                AND user_id = $2
                AND revoked_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .bind(actor_id)
        .execute(&pool)
        .await?;

        let error =
            list_object_version_creators(&pool, actor_id, workspace_id, object_id, &[version.id])
                .await
                .expect_err("revoked actor must not hydrate version creators");
        assert!(matches!(error, KernelError::CapabilityRequired));

        let mut tx = pool.begin().await?;
        let creator =
            fetch_object_version_creator_for_mutation(&mut tx, workspace_id, object_id, version.id)
                .await?
                .expect("already-admitted mutation projection should remain readable");
        assert_eq!(creator.version_id, version.id);
        tx.rollback().await?;

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn version_insert_persists_body_text(pool: PgPool) -> Result<()> {
        let object_id = insert_object(&pool).await?;
        let mut tx = pool.begin().await?;

        let version = insert_version(
            &mut tx,
            CreateObjectVersion {
                object_id,
                version_number: 1,
                title: "v1".to_owned(),
                body: "hello".to_owned(),
                metadata: json!({"k":"v"}),
                created_by: None,
            },
        )
        .await?;
        tx.commit().await?;

        assert_eq!(version.body, "hello");
        let body_text = sqlx::query_scalar::<_, String>(
            r#"
            SELECT body_text
            FROM kival.object_versions
            WHERE id = $1
            "#,
        )
        .bind(version.id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(body_text, "hello");

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn object_version_metadata_rejects_nested_values(pool: PgPool) -> Result<()> {
        let object_id = insert_object(&pool).await?;
        let mut tx = pool.begin().await?;

        let err = insert_version(
            &mut tx,
            CreateObjectVersion {
                object_id,
                version_number: 1,
                title: "nested metadata".to_owned(),
                body: "body".to_owned(),
                metadata: json!({"config": {"enabled": true}}),
                created_by: None,
            },
        )
        .await
        .expect_err("flat metadata constraint should reject nested objects");
        tx.rollback().await?;

        assert!(matches!(err, KernelError::Database(sqlx::Error::Database(_))));

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn update_version_writes_body_text_row(pool: PgPool) -> Result<()> {
        let object_id = insert_object(&pool).await?;
        let (actor_id, workspace_id) = insert_object_reader(&pool, object_id).await?;
        let mut tx = pool.begin().await?;
        let initial = insert_version(
            &mut tx,
            CreateObjectVersion {
                object_id,
                version_number: 1,
                title: "v1".to_owned(),
                body: "one".to_owned(),
                metadata: json!({}),
                created_by: Some(actor_id),
            },
        )
        .await?;
        sqlx::query("UPDATE kival.objects SET current_version_id = $2 WHERE id = $1")
            .bind(object_id)
            .bind(initial.id)
            .execute(&mut *tx)
            .await?;

        let updated = update_object_version(
            &mut tx,
            UpdateObjectVersion {
                workspace_id,
                object_id,
                expected_current_version_id: initial.id,
                title: Some("v2".to_owned()),
                body: Some("two".to_owned()),
                metadata: Some(json!({})),
                created_by: actor_id,
            },
        )
        .await?;
        tx.commit().await?;

        assert!(updated.changed);
        assert_eq!(updated.version.version_number, 2);
        assert_eq!(updated.version.body, "two");

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn concurrent_updates_to_same_object_are_serialized(pool: PgPool) -> Result<()> {
        let object_id = insert_object(&pool).await?;
        let (actor_id, workspace_id) = insert_object_reader(&pool, object_id).await?;
        let mut tx = pool.begin().await?;
        let initial = insert_version(
            &mut tx,
            CreateObjectVersion {
                object_id,
                version_number: 1,
                title: "v1".to_owned(),
                body: "one".to_owned(),
                metadata: json!({}),
                created_by: Some(actor_id),
            },
        )
        .await?;
        sqlx::query("UPDATE kival.objects SET current_version_id = $2 WHERE id = $1")
            .bind(object_id)
            .bind(initial.id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;

        let update_left = async {
            let mut tx = pool.begin().await?;
            let updated = update_object_version(
                &mut tx,
                UpdateObjectVersion {
                    workspace_id,
                    object_id,
                    expected_current_version_id: initial.id,
                    title: Some("left".to_owned()),
                    body: Some("left".to_owned()),
                    metadata: Some(json!({})),
                    created_by: actor_id,
                },
            )
            .await?;
            tx.commit().await?;
            Result::Ok(updated.version.version_number)
        };
        let update_right = async {
            let mut tx = pool.begin().await?;
            let updated = update_object_version(
                &mut tx,
                UpdateObjectVersion {
                    workspace_id,
                    object_id,
                    expected_current_version_id: initial.id,
                    title: Some("right".to_owned()),
                    body: Some("right".to_owned()),
                    metadata: Some(json!({})),
                    created_by: actor_id,
                },
            )
            .await?;
            tx.commit().await?;
            Result::Ok(updated.version.version_number)
        };

        let outcomes: [_; 2] = tokio::join!(update_left, update_right).into();
        let successes = outcomes.iter().filter(|outcome| matches!(outcome, Ok(2))).count();
        let conflicts = outcomes
            .iter()
            .filter(|outcome| matches!(outcome, Err(KernelError::ObjectVersionConflict)))
            .count();

        assert_eq!(successes, 1);
        assert_eq!(conflicts, 1);

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn semantic_noop_update_keeps_current_version(pool: PgPool) -> Result<()> {
        let object_id = insert_object(&pool).await?;
        let (actor_id, workspace_id) = insert_object_reader(&pool, object_id).await?;
        let mut tx = pool.begin().await?;
        let initial = insert_version(
            &mut tx,
            CreateObjectVersion {
                object_id,
                version_number: 1,
                title: "v1".to_owned(),
                body: "one".to_owned(),
                metadata: json!({ "kind": "note" }),
                created_by: Some(actor_id),
            },
        )
        .await?;
        sqlx::query("UPDATE kival.objects SET current_version_id = $2 WHERE id = $1")
            .bind(object_id)
            .bind(initial.id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;

        let before_updated_at = sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>(
            "SELECT updated_at FROM kival.objects WHERE id = $1",
        )
        .bind(object_id)
        .fetch_one(&pool)
        .await?;

        let mut tx = pool.begin().await?;
        let updated = update_object_version(
            &mut tx,
            UpdateObjectVersion {
                workspace_id,
                object_id,
                expected_current_version_id: initial.id,
                title: Some(initial.title.clone()),
                body: Some(initial.body.clone()),
                metadata: Some(initial.metadata.clone()),
                created_by: actor_id,
            },
        )
        .await?;
        tx.commit().await?;

        assert!(!updated.changed);
        assert_eq!(updated.version.id, initial.id);
        assert_eq!(updated.version.version_number, 1);

        let version_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM kival.object_versions WHERE object_id = $1",
        )
        .bind(object_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(version_count, 1);

        let after_updated_at = sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>(
            "SELECT updated_at FROM kival.objects WHERE id = $1",
        )
        .bind(object_id)
        .fetch_one(&pool)
        .await?;
        assert_eq!(after_updated_at, before_updated_at);

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn read_returns_body_text_from_database(pool: PgPool) -> Result<()> {
        let object_id = insert_object(&pool).await?;
        let (actor_id, workspace_id) = insert_object_reader(&pool, object_id).await?;
        let mut tx = pool.begin().await?;
        let created = insert_version(
            &mut tx,
            CreateObjectVersion {
                object_id,
                version_number: 1,
                title: "v1".to_owned(),
                body: "body from postgres".to_owned(),
                metadata: json!({}),
                created_by: None,
            },
        )
        .await?;
        tx.commit().await?;

        let fetched =
            fetch_object_version(&pool, actor_id, workspace_id, object_id, created.id).await?;
        let fetched_by_number =
            fetch_object_version_by_number(&pool, actor_id, workspace_id, object_id, 1).await?;

        assert_eq!(fetched.body, "body from postgres");
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched_by_number, fetched);

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn version_insert_rejects_missing_object(pool: PgPool) -> Result<()> {
        let missing_object_id = Uuid::max();
        let mut tx = pool.begin().await?;

        let err = insert_version(
            &mut tx,
            CreateObjectVersion {
                object_id: missing_object_id,
                version_number: 1,
                title: "missing".to_owned(),
                body: "no orphan".to_owned(),
                metadata: json!({}),
                created_by: None,
            },
        )
        .await
        .expect_err("foreign key should fail");
        tx.rollback().await?;

        assert!(matches!(err, KernelError::Database(sqlx::Error::Database(_))));

        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn object_versions_cannot_be_updated(pool: PgPool) -> Result<()> {
        let object_id = insert_object(&pool).await?;
        let mut tx = pool.begin().await?;
        let created = insert_version(
            &mut tx,
            CreateObjectVersion {
                object_id,
                version_number: 1,
                title: "v1".to_owned(),
                body: "immutable".to_owned(),
                metadata: json!({}),
                created_by: None,
            },
        )
        .await?;
        tx.commit().await?;

        let err = sqlx::query(
            r#"
            UPDATE kival.object_versions
            SET title = 'changed'
            WHERE id = $1
            "#,
        )
        .bind(created.id)
        .execute(&pool)
        .await
        .expect_err("update should be prevented");

        assert!(matches!(err, sqlx::Error::Database(_)));
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn object_versions_cannot_be_deleted(pool: PgPool) -> Result<()> {
        let object_id = insert_object(&pool).await?;
        let mut tx = pool.begin().await?;
        let created = insert_version(
            &mut tx,
            CreateObjectVersion {
                object_id,
                version_number: 1,
                title: "v1".to_owned(),
                body: "immutable".to_owned(),
                metadata: json!({}),
                created_by: None,
            },
        )
        .await?;
        tx.commit().await?;

        let err = sqlx::query(
            r#"
            DELETE FROM kival.object_versions
            WHERE id = $1
            "#,
        )
        .bind(created.id)
        .execute(&pool)
        .await
        .expect_err("delete should be prevented");

        assert!(matches!(err, sqlx::Error::Database(_)));
        Ok(())
    }
}
