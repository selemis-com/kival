//! Embedded web application serving.

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};

/// Revalidation policy for the embedded web application.
const REVALIDATE_CACHE_CONTROL: &str = "no-cache";

/// A web asset embedded into the server binary.
#[derive(Debug)]
struct EmbeddedAsset {
    /// URL path used to address the asset.
    path: &'static str,
    /// Raw asset contents.
    bytes: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));

/// Builds a router that serves the web application embedded in `kivald`.
pub(crate) fn router() -> Router {
    Router::new().fallback(handle_web_request)
}

/// Serves an embedded asset or falls back to the SPA entry point for HTML requests.
async fn handle_web_request(method: Method, uri: Uri, headers: HeaderMap) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }

    let path = if uri.path() == "/" { "/index.html" } else { uri.path() };
    let asset = find_asset(path)
        .or_else(|| accepts_html(&headers).then(|| find_asset("/index.html")).flatten());

    asset.map_or_else(
        || StatusCode::NOT_FOUND.into_response(),
        |asset| asset_response(asset, method == Method::HEAD),
    )
}

/// Finds an embedded asset by its absolute URL path.
fn find_asset(path: &str) -> Option<&'static EmbeddedAsset> {
    WEB_ASSETS.binary_search_by(|asset| asset.path.cmp(path)).ok().map(|index| &WEB_ASSETS[index])
}

/// Returns whether the request accepts an HTML response.
fn accepts_html(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|item| item.trim().starts_with("text/html")))
}

/// Builds an HTTP response for an embedded asset.
fn asset_response(asset: &'static EmbeddedAsset, head: bool) -> Response {
    let body = if head { Body::empty() } else { Body::from(asset.bytes) };
    let mut response = Response::new(body);
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type(asset.path)));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(REVALIDATE_CACHE_CONTROL));
    response
}

/// Returns the MIME type used for an embedded asset path.
fn content_type(path: &str) -> &'static str {
    let extension = path.rsplit_once('.').map(|(_, extension)| extension);
    match extension {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("wasm") => "application/wasm",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use axum::{body::to_bytes, http::Request};
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn serves_embedded_index_and_spa_routes() {
        let index = find_asset("/index.html").expect("web build should contain index.html");

        for (path, accept) in [("/", None), ("/workspaces/example", Some("text/html"))] {
            let mut request = Request::builder().uri(path);
            if let Some(accept) = accept {
                request = request.header(header::ACCEPT, accept);
            }
            let response = router().oneshot(request.body(Body::empty()).unwrap()).await.unwrap();

            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers().get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()),
                Some("text/html; charset=utf-8")
            );
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            assert_eq!(body.as_ref(), index.bytes);
        }
    }

    #[tokio::test]
    async fn missing_static_asset_does_not_fall_back_to_the_spa() {
        let response = router()
            .oneshot(
                Request::builder()
                    .uri("/missing.js")
                    .header(header::ACCEPT, "*/*")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn head_requests_return_headers_without_a_body() {
        let response = router()
            .oneshot(Request::builder().method(Method::HEAD).uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()),
            Some("text/html; charset=utf-8")
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(body.is_empty());
    }
}
