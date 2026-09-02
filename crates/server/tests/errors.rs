//! Error response API scenario tests.

#[cfg(test)]
mod tests {
    use axum::{
        body::to_bytes,
        http::{Method, StatusCode},
    };
    use eyre::Result;
    use kival_sdk::ApiErrorResponse;
    use kival_tests::{TestFixtureExt, TestKival};

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn group_list_invalid_status_returns_validation_error_body(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;

        let response =
            r.request(Some(&r.admin), Method::GET, "/groups?status=deleted", None).await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), usize::MAX).await?;
        let error: ApiErrorResponse = serde_json::from_slice(&body)?;
        assert_eq!(error.error.code, "bad_request");
        assert!(error.error.message.contains("status"));

        Ok(())
    }

    #[sqlx::test(migrations = "../kernel/migrations")]
    async fn object_list_invalid_status_returns_validation_error_body(
        pool: sqlx::PgPool,
    ) -> Result<()> {
        let r = TestKival::new(pool).await?;
        let workspace = r.create_workspace("object list invalid status body").await?;

        let response = r
            .request(
                Some(&r.admin),
                Method::GET,
                &format!("/workspaces/{}/objects?status=deleted", workspace.id),
                None,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), usize::MAX).await?;
        let error: ApiErrorResponse = serde_json::from_slice(&body)?;
        assert_eq!(error.error.code, "bad_request");
        assert!(error.error.message.contains("status"));

        Ok(())
    }
}
