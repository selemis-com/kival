//! Public API scenario tests.

#[cfg(test)]
mod tests {
    use axum::{
        body::to_bytes,
        http::{Method, StatusCode},
    };
    use eyre::Result;
    use kival_sdk::{ApiErrorResponse, Status, StatusResponse};
    use kival_tests::{TestKival, TestResponseExt};

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn readyz_returns_ok(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let response: StatusResponse =
            r.request_json(None, Method::GET, "/readyz", None).await?.into_success()?;

        assert_eq!(response.status, Status::Ok);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn healthz_returns_ok(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let response: StatusResponse =
            r.request_json(None, Method::GET, "/healthz", None).await?.into_success()?;

        assert_eq!(response.status, Status::Ok);

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn unknown_route_returns_json_404(pool: sqlx::PgPool) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let response = r.request(None, Method::GET, "/does-not-exist", None).await?;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = to_bytes(response.into_body(), usize::MAX).await?;
        let body: ApiErrorResponse = serde_json::from_slice(&body)?;
        assert_eq!(body.error.code, "not_found");

        Ok(())
    }
}
