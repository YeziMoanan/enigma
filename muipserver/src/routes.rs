use axum::{
    Json, Router,
    extract::{Request, State},
    http::{StatusCode, header::AUTHORIZATION},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};

use crate::{MuipOptions, account_api};

pub fn router(options: MuipOptions) -> Router {
    let state = account_api::ApiState::new(options.token, options.gm_addr, options.db);
    let protected = Router::new()
        .route(
            "/api/reverse1999/accounts/preview",
            post(account_api::preview_account),
        )
        .route(
            "/api/reverse1999/allowlist/import-preview",
            post(account_api::preview_import),
        )
        .route(
            "/api/reverse1999/allowlist/import",
            post(account_api::apply_import),
        )
        .route(
            "/api/reverse1999/allowlist/replace-preview",
            post(account_api::preview_replace),
        )
        .route(
            "/api/reverse1999/allowlist/replace",
            post(account_api::apply_replace),
        )
        .route(
            "/api/reverse1999/accounts/{account}/ban",
            post(account_api::ban),
        )
        .route(
            "/api/reverse1999/accounts/{account}/unban",
            post(account_api::unban),
        )
        .route(
            "/api/reverse1999/audit/{request_id}",
            get(account_api::request_status),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_bearer,
        ));

    Router::new()
        .route("/healthz", get(health))
        .merge(protected)
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
}

async fn require_bearer(
    State(state): State<account_api::ApiState>,
    request: Request,
    next: Next,
) -> Response {
    let expected = format!("Bearer {}", state.token());
    let authorized = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected);
    if !authorized {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::router;
    use crate::MuipOptions;
    use axum::{body::Body, http::Request};
    use sqlx::sqlite::SqlitePoolOptions;
    use tower::ServiceExt;

    async fn app() -> axum::Router {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        database::run_migrations(&db).await.unwrap();
        router(MuipOptions {
            host: "127.0.0.1".to_string(),
            port: 0,
            token: "test-token".to_string(),
            gm_addr: "127.0.0.1:9".to_string(),
            db,
        })
    }

    async fn status(method: &str, uri: &str) -> axum::http::StatusCode {
        app()
            .await
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn health_is_public_and_fixed_operations_require_bearer() {
        assert_eq!(status("GET", "/healthz").await, 200);
        for (method, uri) in [
            ("POST", "/api/reverse1999/accounts/preview"),
            ("POST", "/api/reverse1999/allowlist/import-preview"),
            ("POST", "/api/reverse1999/allowlist/import"),
            ("POST", "/api/reverse1999/accounts/player01/ban"),
            ("GET", "/api/reverse1999/audit/request-1"),
        ] {
            assert_eq!(status(method, uri).await, 401, "{method} {uri}");
        }
    }

    #[tokio::test]
    async fn legacy_panel_and_arbitrary_command_routes_are_absent() {
        assert_eq!(status("GET", "/muip/gm").await, 404);
        assert_eq!(status("POST", "/api/run_gm_cmd").await, 404);
    }
}
