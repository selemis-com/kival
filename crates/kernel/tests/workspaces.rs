//! Integration tests for kernel workspace lifecycle transitions.

#[cfg(test)]
mod tests {
    use kival_kernel::{
        KernelError, Result, archive_workspace, create_user, create_workspace, unarchive_workspace,
    };
    use sqlx::PgPool;

    #[sqlx::test(migrations = "./migrations")]
    async fn workspace_restore_enforces_lifecycle_state(pool: PgPool) -> Result<()> {
        let mut tx = pool.begin().await?;
        let admin = create_user(&mut tx, "admin", "Admin").await?;
        let workspace = create_workspace(&mut tx, "workspace", None, admin.id).await?.workspace;
        tx.commit().await?;

        let mut tx = pool.begin().await?;
        let error = unarchive_workspace(&mut tx, workspace.id)
            .await
            .expect_err("an active workspace is not a restore target");
        assert!(matches!(error, KernelError::Database(sqlx::Error::RowNotFound)));
        tx.rollback().await?;

        let mut tx = pool.begin().await?;
        archive_workspace(&mut tx, workspace.id, admin.id).await?;
        tx.commit().await?;

        let mut tx = pool.begin().await?;
        let restored = unarchive_workspace(&mut tx, workspace.id).await?;
        assert_eq!(restored.id, workspace.id);
        assert!(restored.archived_at.is_none());
        tx.commit().await?;

        Ok(())
    }
}
