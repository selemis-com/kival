//! Integration tests for kernel database authorization helpers.

#[cfg(test)]
mod tests {
    use kival_kernel::{KernelError, Result};
    use sqlx::PgPool;

    async fn require_capability(
        pool: &PgPool,
        exists: Option<bool>,
        allowed: Option<bool>,
    ) -> Result<bool> {
        Ok(sqlx::query_scalar(
            r#"
            SELECT kival.require_capability($1, $2)
            "#,
        )
        .bind(exists)
        .bind(allowed)
        .fetch_one(pool)
        .await?)
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn require_capability_fails_closed(pool: PgPool) -> Result<()> {
        let error = require_capability(&pool, Some(false), Some(true))
            .await
            .expect_err("missing resources must be rejected");
        assert!(matches!(error, KernelError::ResourceNotFound));

        let error = require_capability(&pool, None, Some(true))
            .await
            .expect_err("unknown resource existence must fail closed");
        assert!(matches!(error, KernelError::ResourceNotFound));

        let error = require_capability(&pool, Some(true), Some(false))
            .await
            .expect_err("missing capabilities must be rejected");
        assert!(matches!(error, KernelError::CapabilityRequired));

        let error = require_capability(&pool, Some(true), None)
            .await
            .expect_err("unknown capability state must fail closed");
        assert!(matches!(error, KernelError::CapabilityRequired));

        assert!(require_capability(&pool, Some(true), Some(true)).await?);

        Ok(())
    }
}
