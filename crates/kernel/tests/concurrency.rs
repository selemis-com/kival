//! Adversarial integration tests for Kival domain concurrency and lock ordering.

#[cfg(test)]
mod tests {
    use kival_kernel::{
        CreateInitialObject, GrantPrincipal, KernelError, MembershipRole, ObjectRole, Result,
        active_object_role_pair, create_comment, create_comment_thread, create_group,
        create_group_membership, create_initial_object, create_object_edge, create_object_grant,
        create_user, create_workspace, create_workspace_group, create_workspace_membership,
        lock_comment, lock_thread_for_reply, revoke_workspace_membership,
    };
    use serde_json::json;
    use sqlx::{PgPool, Postgres, Transaction};
    use uuid::Uuid;

    /// Creates one active user with a unique username.
    async fn insert_user(pool: &PgPool, label: &str) -> Result<Uuid> {
        let suffix = Uuid::now_v7().simple().to_string();
        let username = format!("{}-{}", label, &suffix[..12]);
        let mut tx = pool.begin().await?;
        let user = create_user(&mut tx, &username, &username).await?;
        tx.commit().await?;
        Ok(user.id)
    }

    /// Creates a workspace and its creator admin membership.
    async fn insert_workspace(pool: &PgPool, actor_id: Uuid) -> Result<Uuid> {
        let suffix = Uuid::now_v7().simple().to_string();
        let mut tx = pool.begin().await?;
        let workspace = create_workspace(
            &mut tx,
            &format!("lock-order-workspace-{}", &suffix[..12]),
            None,
            actor_id,
        )
        .await?
        .workspace;
        tx.commit().await?;
        Ok(workspace.id)
    }

    /// Creates one active group.
    async fn insert_group(pool: &PgPool, actor_id: Uuid) -> Result<Uuid> {
        let suffix = Uuid::now_v7().simple().to_string();
        let mut tx = pool.begin().await?;
        let group =
            create_group(&mut tx, &format!("lock-order-group-{}", &suffix[..12]), None, actor_id)
                .await?;
        tx.commit().await?;
        Ok(group.id)
    }

    /// Creates one active object in an existing workspace.
    async fn insert_object(pool: &PgPool, workspace_id: Uuid, actor_id: Uuid) -> Result<Uuid> {
        let suffix = Uuid::now_v7().simple().to_string();
        let mut tx = pool.begin().await?;
        let created = create_initial_object(
            &mut tx,
            CreateInitialObject {
                workspace_id,
                title: format!("Lock order object {}", &suffix[..12]),
                body: "Body".to_owned(),
                metadata: json!({}),
                created_by: actor_id,
            },
        )
        .await?;
        tx.commit().await?;
        Ok(created.object_id)
    }

    /// Creates one live commentary thread with its root comment.
    async fn insert_comment_thread(
        pool: &PgPool,
        workspace_id: Uuid,
        object_id: Uuid,
        actor_id: Uuid,
    ) -> Result<(Uuid, Uuid)> {
        let mut tx = pool.begin().await?;
        let thread_id = create_comment_thread(&mut tx, workspace_id, object_id, actor_id).await?;
        let comment_id =
            create_comment(&mut tx, workspace_id, object_id, thread_id, None, actor_id, "root")
                .await?;
        tx.commit().await?;
        Ok((thread_id, comment_id))
    }

    /// Row kind used by lock-order tests.
    #[derive(Clone, Copy)]
    enum BlockRow {
        Workspace,
        Group,
        Object,
        CommentThread,
    }

    /// Locks one row strongly enough to block a lifecycle dependency lock.
    async fn block_row(tx: &mut Transaction<'_, Postgres>, row: BlockRow, id: Uuid) -> Result<()> {
        let query = match row {
            BlockRow::Workspace => sqlx::query_scalar::<_, Uuid>(
                "SELECT id FROM kival.workspaces WHERE id = $1 FOR UPDATE",
            ),
            BlockRow::Group => sqlx::query_scalar::<_, Uuid>(
                "SELECT id FROM kival.groups WHERE id = $1 FOR UPDATE",
            ),
            BlockRow::Object => sqlx::query_scalar::<_, Uuid>(
                "SELECT id FROM kival.objects WHERE id = $1 FOR UPDATE",
            ),
            BlockRow::CommentThread => sqlx::query_scalar::<_, Uuid>(
                "SELECT id FROM kival.comment_threads WHERE id = $1 FOR UPDATE",
            ),
        };
        query.bind(id).fetch_one(&mut **tx).await?;
        Ok(())
    }

    /// Returns the backend PID used by a transaction.
    async fn backend_pid(tx: &mut Transaction<'_, Postgres>) -> Result<i32> {
        Ok(sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut **tx).await?)
    }

    /// Waits until a backend is actually blocked on a PostgreSQL lock.
    async fn wait_until_lock_wait(pool: &PgPool, pid: i32) -> Result<()> {
        for _ in 0..100 {
            let waiting = sqlx::query_scalar::<_, bool>(
                r#"
                SELECT wait_event_type = 'Lock'
                FROM pg_stat_activity
                WHERE pid = $1
                "#,
            )
            .bind(pid)
            .fetch_optional(pool)
            .await?;
            if waiting == Some(true) {
                return Ok(());
            }
            sqlx::query("SELECT pg_sleep(0.01)").execute(pool).await?;
        }
        panic!("transition backend did not enter a lock wait");
    }

    /// Executes a short update that must not be blocked by a prematurely acquired child lock.
    async fn assert_user_update_available(pool: &PgPool, user_id: Uuid) -> Result<()> {
        let mut tx = pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '100ms'").execute(&mut *tx).await?;
        sqlx::query("UPDATE kival.users SET display_name = display_name WHERE id = $1")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Executes a short group update that must not be blocked by a prematurely acquired group lock.
    async fn assert_group_update_available(pool: &PgPool, group_id: Uuid) -> Result<()> {
        let mut tx = pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '100ms'").execute(&mut *tx).await?;
        sqlx::query("UPDATE kival.groups SET description = description WHERE id = $1")
            .bind(group_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Executes a short object update that must not be blocked by a prematurely acquired object
    /// lock.
    async fn assert_object_update_available(pool: &PgPool, object_id: Uuid) -> Result<()> {
        let mut tx = pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '100ms'").execute(&mut *tx).await?;
        sqlx::query("UPDATE kival.objects SET updated_at = updated_at WHERE id = $1")
            .bind(object_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn workspace_membership_locks_workspace_before_user(pool: PgPool) -> Result<()> {
        let actor_id = insert_user(&pool, "wo-actor").await?;
        let member_id = insert_user(&pool, "wo-member").await?;
        let workspace_id = insert_workspace(&pool, actor_id).await?;

        let mut blocker = pool.begin().await?;
        block_row(&mut blocker, BlockRow::Workspace, workspace_id).await?;

        let mut transition_tx = pool.begin().await?;
        let pid = backend_pid(&mut transition_tx).await?;
        let transition = async {
            let result = create_workspace_membership(
                &mut transition_tx,
                workspace_id,
                Some(member_id),
                None,
                MembershipRole::Member,
                actor_id,
            )
            .await;
            transition_tx.rollback().await?;
            result.map(|_| ())
        };
        let probe = async {
            let result = async {
                wait_until_lock_wait(&pool, pid).await?;
                assert_user_update_available(&pool, member_id).await
            }
            .await;
            blocker.rollback().await?;
            result
        };

        let (transition_result, probe_result) = tokio::join!(transition, probe);
        probe_result?;
        transition_result?;
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn group_membership_locks_group_before_user(pool: PgPool) -> Result<()> {
        let actor_id = insert_user(&pool, "go-actor").await?;
        let member_id = insert_user(&pool, "go-member").await?;
        let group_id = insert_group(&pool, actor_id).await?;

        let mut blocker = pool.begin().await?;
        block_row(&mut blocker, BlockRow::Group, group_id).await?;

        let mut transition_tx = pool.begin().await?;
        let pid = backend_pid(&mut transition_tx).await?;
        let transition = async {
            let result = create_group_membership(
                &mut transition_tx,
                group_id,
                Some(member_id),
                None,
                MembershipRole::Member,
                actor_id,
            )
            .await;
            transition_tx.rollback().await?;
            result.map(|_| ())
        };
        let probe = async {
            let result = async {
                wait_until_lock_wait(&pool, pid).await?;
                assert_user_update_available(&pool, member_id).await
            }
            .await;
            blocker.rollback().await?;
            result
        };

        let (transition_result, probe_result) = tokio::join!(transition, probe);
        probe_result?;
        transition_result?;
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn workspace_group_locks_workspace_before_group(pool: PgPool) -> Result<()> {
        let actor_id = insert_user(&pool, "wg-actor").await?;
        let workspace_id = insert_workspace(&pool, actor_id).await?;
        let group_id = insert_group(&pool, actor_id).await?;

        let mut blocker = pool.begin().await?;
        block_row(&mut blocker, BlockRow::Workspace, workspace_id).await?;

        let mut transition_tx = pool.begin().await?;
        let pid = backend_pid(&mut transition_tx).await?;
        let transition = async {
            let result =
                create_workspace_group(&mut transition_tx, workspace_id, group_id, actor_id).await;
            transition_tx.rollback().await?;
            result.map(|_| ())
        };
        let probe = async {
            let result = async {
                wait_until_lock_wait(&pool, pid).await?;
                assert_group_update_available(&pool, group_id).await
            }
            .await;
            blocker.rollback().await?;
            result
        };

        let (transition_result, probe_result) = tokio::join!(transition, probe);
        probe_result?;
        transition_result?;
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn object_grant_locks_object_before_principal(pool: PgPool) -> Result<()> {
        let actor_id = insert_user(&pool, "og-actor").await?;
        let member_id = insert_user(&pool, "og-member").await?;
        let workspace_id = insert_workspace(&pool, actor_id).await?;

        let mut member_tx = pool.begin().await?;
        create_workspace_membership(
            &mut member_tx,
            workspace_id,
            Some(member_id),
            None,
            MembershipRole::Member,
            actor_id,
        )
        .await?;
        member_tx.commit().await?;

        let object_id = insert_object(&pool, workspace_id, actor_id).await?;
        let mut blocker = pool.begin().await?;
        block_row(&mut blocker, BlockRow::Object, object_id).await?;

        let mut transition_tx = pool.begin().await?;
        let pid = backend_pid(&mut transition_tx).await?;
        let transition = async {
            let result = create_object_grant(
                &mut transition_tx,
                workspace_id,
                object_id,
                GrantPrincipal::User(member_id),
                ObjectRole::Viewer,
                actor_id,
            )
            .await;
            transition_tx.rollback().await?;
            result.map(|_| ())
        };
        let probe = async {
            let result = async {
                wait_until_lock_wait(&pool, pid).await?;
                assert_user_update_available(&pool, member_id).await
            }
            .await;
            blocker.rollback().await?;
            result
        };

        let (transition_result, probe_result) = tokio::join!(transition, probe);
        probe_result?;
        transition_result?;
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn object_edge_locks_endpoints_in_uuid_order(pool: PgPool) -> Result<()> {
        let actor_id = insert_user(&pool, "oe-actor").await?;
        let workspace_id = insert_workspace(&pool, actor_id).await?;
        let first = insert_object(&pool, workspace_id, actor_id).await?;
        let second = insert_object(&pool, workspace_id, actor_id).await?;
        let (lower, higher) = if first < second { (first, second) } else { (second, first) };

        let mut blocker = pool.begin().await?;
        block_row(&mut blocker, BlockRow::Object, lower).await?;

        let mut transition_tx = pool.begin().await?;
        let pid = backend_pid(&mut transition_tx).await?;
        let transition = async {
            let result =
                create_object_edge(&mut transition_tx, workspace_id, higher, lower, actor_id).await;
            transition_tx.rollback().await?;
            result.map(|_| ())
        };
        let probe = async {
            let result = async {
                wait_until_lock_wait(&pool, pid).await?;
                assert_object_update_available(&pool, higher).await
            }
            .await;
            blocker.rollback().await?;
            result
        };

        let (transition_result, probe_result) = tokio::join!(transition, probe);
        probe_result?;
        transition_result?;
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn comment_replies_serialize_on_thread(pool: PgPool) -> Result<()> {
        let actor_id = insert_user(&pool, "reply-actor").await?;
        let workspace_id = insert_workspace(&pool, actor_id).await?;
        let object_id = insert_object(&pool, workspace_id, actor_id).await?;
        let (thread_id, _) =
            insert_comment_thread(&pool, workspace_id, object_id, actor_id).await?;

        let mut first = pool.begin().await?;
        let locked = lock_thread_for_reply(&mut first, workspace_id, object_id, thread_id).await?;
        assert!(locked.is_some());

        let mut second = pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '100ms'").execute(&mut *second).await?;
        let kernel_error = lock_thread_for_reply(&mut second, workspace_id, object_id, thread_id)
            .await
            .expect_err("concurrent reply must serialize on the thread row");
        let KernelError::Database(error) = kernel_error else {
            panic!("expected PostgreSQL lock timeout, got {kernel_error:?}");
        };
        assert_lock_timeout(error);

        second.rollback().await?;
        first.rollback().await?;
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn comment_mutation_locks_thread_before_comment(pool: PgPool) -> Result<()> {
        let actor_id = insert_user(&pool, "order-actor").await?;
        let workspace_id = insert_workspace(&pool, actor_id).await?;
        let object_id = insert_object(&pool, workspace_id, actor_id).await?;
        let (thread_id, comment_id) =
            insert_comment_thread(&pool, workspace_id, object_id, actor_id).await?;

        let mut blocker = pool.begin().await?;
        block_row(&mut blocker, BlockRow::CommentThread, thread_id).await?;

        let mut transition_tx = pool.begin().await?;
        let pid = backend_pid(&mut transition_tx).await?;
        let transition = async {
            let result =
                lock_comment(&mut transition_tx, workspace_id, object_id, comment_id).await;
            transition_tx.rollback().await?;
            result.map(|_| ())
        };
        let probe = async {
            let result = async {
                wait_until_lock_wait(&pool, pid).await?;
                let mut tx = pool.begin().await?;
                sqlx::query("SET LOCAL lock_timeout = '100ms'").execute(&mut *tx).await?;
                sqlx::query("UPDATE kival.comments SET body = body WHERE id = $1")
                    .bind(comment_id)
                    .execute(&mut *tx)
                    .await?;
                tx.commit().await?;
                Ok::<(), KernelError>(())
            }
            .await;
            blocker.rollback().await?;
            result
        };

        let (transition_result, probe_result) = tokio::join!(transition, probe);
        probe_result?;
        transition_result?;
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn protected_read_assertions_are_statement_stable(pool: PgPool) -> Result<()> {
        let stable = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT count(*) = 6
                AND bool_and(provolatile = 's'::"char")
            FROM pg_proc
            WHERE pronamespace = 'kival'::regnamespace
                AND proname = ANY (ARRAY[
                    'require_capability',
                    'require_read_workspace',
                    'require_read_group',
                    'require_read_object',
                    'require_access_active_object',
                    'require_admin_active_group'
                ])
            "#,
        )
        .fetch_one(&pool)
        .await?;
        assert!(stable, "protected-read capability assertions must be STABLE");
        Ok(())
    }

    /// Asserts that a competing user disable waits for a held lifecycle dependency.
    async fn assert_user_disable_blocked(
        pool: &PgPool,
        user_id: Uuid,
        actor_id: Uuid,
    ) -> Result<()> {
        let mut tx = pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '100ms'").execute(&mut *tx).await?;
        let error = sqlx::query(
            r#"
            UPDATE kival.users
            SET status = 'disabled',
                disabled_at = now(),
                disabled_by = $2,
                disabled_by_operator = false
            WHERE id = $1
                AND disabled_at IS NULL
            "#,
        )
        .bind(user_id)
        .bind(actor_id)
        .execute(&mut *tx)
        .await
        .expect_err("user disable must wait for lifecycle dependency");
        assert_lock_timeout(error);
        tx.rollback().await?;
        Ok(())
    }

    /// Asserts that a competing workspace archive waits for a held lifecycle dependency.
    async fn assert_workspace_archive_blocked(
        pool: &PgPool,
        workspace_id: Uuid,
        actor_id: Uuid,
    ) -> Result<()> {
        let mut tx = pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '100ms'").execute(&mut *tx).await?;
        let error = sqlx::query(
            r#"
            UPDATE kival.workspaces
            SET status = 'archived',
                archived_at = now(),
                archived_by = $2
            WHERE id = $1
                AND archived_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .bind(actor_id)
        .execute(&mut *tx)
        .await
        .expect_err("workspace archive must wait for lifecycle dependency");
        assert_lock_timeout(error);
        tx.rollback().await?;
        Ok(())
    }

    /// Asserts that a competing group archive waits for a held lifecycle dependency.
    async fn assert_group_archive_blocked(
        pool: &PgPool,
        group_id: Uuid,
        actor_id: Uuid,
    ) -> Result<()> {
        let mut tx = pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '100ms'").execute(&mut *tx).await?;
        let error = sqlx::query(
            r#"
            UPDATE kival.groups
            SET status = 'archived',
                archived_at = now(),
                archived_by = $2
            WHERE id = $1
                AND archived_at IS NULL
            "#,
        )
        .bind(group_id)
        .bind(actor_id)
        .execute(&mut *tx)
        .await
        .expect_err("group archive must wait for lifecycle dependency");
        assert_lock_timeout(error);
        tx.rollback().await?;
        Ok(())
    }

    /// Asserts that a competing object archive waits for a held lifecycle dependency.
    async fn assert_object_archive_blocked(
        pool: &PgPool,
        workspace_id: Uuid,
        object_id: Uuid,
        actor_id: Uuid,
    ) -> Result<()> {
        let mut tx = pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '100ms'").execute(&mut *tx).await?;
        let error = sqlx::query(
            r#"
            UPDATE kival.objects
            SET status = 'archived',
                archived_at = now(),
                archived_by = $3
            WHERE workspace_id = $1
                AND id = $2
                AND archived_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .bind(object_id)
        .bind(actor_id)
        .execute(&mut *tx)
        .await
        .expect_err("object archive must wait for lifecycle dependency");
        assert_lock_timeout(error);
        tx.rollback().await?;
        Ok(())
    }

    /// Asserts that revoking a workspace membership waits for a held grant dependency.
    async fn assert_workspace_membership_revoke_blocked(
        pool: &PgPool,
        workspace_id: Uuid,
        user_id: Uuid,
        actor_id: Uuid,
    ) -> Result<()> {
        let mut tx = pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '100ms'").execute(&mut *tx).await?;
        let error = sqlx::query(
            r#"
            UPDATE kival.workspace_memberships
            SET revoked_at = now(),
                revoked_by = $3
            WHERE workspace_id = $1
                AND user_id = $2
                AND revoked_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .bind(user_id)
        .bind(actor_id)
        .execute(&mut *tx)
        .await
        .expect_err("membership revocation must wait for grant dependency");
        assert_lock_timeout(error);
        tx.rollback().await?;
        Ok(())
    }

    /// Asserts that archiving a workspace-group link waits for a held grant dependency.
    async fn assert_workspace_group_archive_blocked(
        pool: &PgPool,
        workspace_id: Uuid,
        group_id: Uuid,
        actor_id: Uuid,
    ) -> Result<()> {
        let mut tx = pool.begin().await?;
        sqlx::query("SET LOCAL lock_timeout = '100ms'").execute(&mut *tx).await?;
        let error = sqlx::query(
            r#"
            UPDATE kival.workspace_groups
            SET status = 'archived',
                archived_at = now(),
                archived_by = $3
            WHERE workspace_id = $1
                AND group_id = $2
                AND archived_at IS NULL
            "#,
        )
        .bind(workspace_id)
        .bind(group_id)
        .bind(actor_id)
        .execute(&mut *tx)
        .await
        .expect_err("workspace-group archive must wait for grant dependency");
        assert_lock_timeout(error);
        tx.rollback().await?;
        Ok(())
    }

    /// Verifies PostgreSQL reported a lock timeout rather than an unrelated database error.
    fn assert_lock_timeout(error: sqlx::Error) {
        let sqlx::Error::Database(database) = error else {
            panic!("expected PostgreSQL lock timeout, got {error:?}");
        };
        assert_eq!(database.code().as_deref(), Some("55P03"));
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn workspace_membership_pins_workspace_and_user_lifecycle(pool: PgPool) -> Result<()> {
        let actor_id = insert_user(&pool, "wm-pin-actor").await?;
        let member_id = insert_user(&pool, "wm-pin-user").await?;
        let workspace_id = insert_workspace(&pool, actor_id).await?;

        let mut tx = pool.begin().await?;
        create_workspace_membership(
            &mut tx,
            workspace_id,
            Some(member_id),
            None,
            MembershipRole::Member,
            actor_id,
        )
        .await?;

        assert_workspace_archive_blocked(&pool, workspace_id, actor_id).await?;
        assert_user_disable_blocked(&pool, member_id, actor_id).await?;
        tx.rollback().await?;
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn workspace_group_pins_workspace_and_group_lifecycle(pool: PgPool) -> Result<()> {
        let actor_id = insert_user(&pool, "wg-pin-actor").await?;
        let workspace_id = insert_workspace(&pool, actor_id).await?;
        let group_id = insert_group(&pool, actor_id).await?;

        let mut tx = pool.begin().await?;
        create_workspace_group(&mut tx, workspace_id, group_id, actor_id).await?;

        assert_workspace_archive_blocked(&pool, workspace_id, actor_id).await?;
        assert_group_archive_blocked(&pool, group_id, actor_id).await?;
        tx.rollback().await?;
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn user_object_grant_pins_object_user_and_membership_lifecycle(
        pool: PgPool,
    ) -> Result<()> {
        let actor_id = insert_user(&pool, "ug-pin-actor").await?;
        let member_id = insert_user(&pool, "ug-pin-user").await?;
        let workspace_id = insert_workspace(&pool, actor_id).await?;

        let mut member_tx = pool.begin().await?;
        create_workspace_membership(
            &mut member_tx,
            workspace_id,
            Some(member_id),
            None,
            MembershipRole::Member,
            actor_id,
        )
        .await?;
        member_tx.commit().await?;

        let object_id = insert_object(&pool, workspace_id, actor_id).await?;
        let mut tx = pool.begin().await?;
        create_object_grant(
            &mut tx,
            workspace_id,
            object_id,
            GrantPrincipal::User(member_id),
            ObjectRole::Viewer,
            actor_id,
        )
        .await?;

        assert_object_archive_blocked(&pool, workspace_id, object_id, actor_id).await?;
        assert_workspace_membership_revoke_blocked(&pool, workspace_id, member_id, actor_id)
            .await?;
        assert_user_disable_blocked(&pool, member_id, actor_id).await?;
        tx.rollback().await?;
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn group_object_grant_pins_object_group_and_link_lifecycle(pool: PgPool) -> Result<()> {
        let actor_id = insert_user(&pool, "gg-pin-actor").await?;
        let workspace_id = insert_workspace(&pool, actor_id).await?;
        let group_id = insert_group(&pool, actor_id).await?;

        let mut link_tx = pool.begin().await?;
        create_workspace_group(&mut link_tx, workspace_id, group_id, actor_id).await?;
        link_tx.commit().await?;

        let object_id = insert_object(&pool, workspace_id, actor_id).await?;
        let mut tx = pool.begin().await?;
        create_object_grant(
            &mut tx,
            workspace_id,
            object_id,
            GrantPrincipal::Group(group_id),
            ObjectRole::Viewer,
            actor_id,
        )
        .await?;

        assert_object_archive_blocked(&pool, workspace_id, object_id, actor_id).await?;
        assert_group_archive_blocked(&pool, group_id, actor_id).await?;
        assert_workspace_group_archive_blocked(&pool, workspace_id, group_id, actor_id).await?;
        tx.rollback().await?;
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn object_edge_pins_workspace_and_both_endpoint_lifecycles(pool: PgPool) -> Result<()> {
        let actor_id = insert_user(&pool, "edge-pin-actor").await?;
        let workspace_id = insert_workspace(&pool, actor_id).await?;
        let source_id = insert_object(&pool, workspace_id, actor_id).await?;
        let target_id = insert_object(&pool, workspace_id, actor_id).await?;

        let mut tx = pool.begin().await?;
        create_object_edge(&mut tx, workspace_id, source_id, target_id, actor_id).await?;

        assert_workspace_archive_blocked(&pool, workspace_id, actor_id).await?;
        assert_object_archive_blocked(&pool, workspace_id, source_id, actor_id).await?;
        assert_object_archive_blocked(&pool, workspace_id, target_id, actor_id).await?;
        tx.rollback().await?;
        Ok(())
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn admitted_object_pair_mutation_survives_later_membership_revocation(
        pool: PgPool,
    ) -> Result<()> {
        let owner_id = insert_user(&pool, "pair-admit-owner").await?;
        let actor_id = insert_user(&pool, "pair-admit-actor").await?;
        let workspace_id = insert_workspace(&pool, owner_id).await?;

        let mut membership_tx = pool.begin().await?;
        let membership = create_workspace_membership(
            &mut membership_tx,
            workspace_id,
            Some(actor_id),
            None,
            MembershipRole::Member,
            owner_id,
        )
        .await?;
        membership_tx.commit().await?;

        let source_id = insert_object(&pool, workspace_id, owner_id).await?;
        let target_id = insert_object(&pool, workspace_id, owner_id).await?;

        let mut grants_tx = pool.begin().await?;
        create_object_grant(
            &mut grants_tx,
            workspace_id,
            source_id,
            GrantPrincipal::User(actor_id),
            ObjectRole::Editor,
            owner_id,
        )
        .await?;
        create_object_grant(
            &mut grants_tx,
            workspace_id,
            target_id,
            GrantPrincipal::User(actor_id),
            ObjectRole::Viewer,
            owner_id,
        )
        .await?;
        grants_tx.commit().await?;

        let admitted = active_object_role_pair(
            &pool,
            actor_id,
            workspace_id,
            source_id,
            ObjectRole::Editor,
            target_id,
            ObjectRole::Viewer,
        )
        .await?;
        assert_eq!(admitted.0.role, Some(ObjectRole::Editor));
        assert_eq!(admitted.1.role, Some(ObjectRole::Viewer));

        let mut revoke_tx = pool.begin().await?;
        revoke_workspace_membership(&mut revoke_tx, workspace_id, membership.id, owner_id).await?;
        revoke_tx.commit().await?;

        let later = active_object_role_pair(
            &pool,
            actor_id,
            workspace_id,
            source_id,
            ObjectRole::Editor,
            target_id,
            ObjectRole::Viewer,
        )
        .await?;
        assert!(later.0.exists);
        assert!(later.1.exists);
        assert!(later.0.role.is_none());
        assert!(later.1.role.is_none());

        let mut transition_tx = pool.begin().await?;
        create_object_edge(&mut transition_tx, workspace_id, source_id, target_id, actor_id)
            .await?;
        transition_tx.commit().await?;

        Ok(())
    }
}
