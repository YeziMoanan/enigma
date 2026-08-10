use axum::{
    body::{Body, Bytes, to_bytes},
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use std::time::Instant;

pub async fn full_logger(req: Request<Body>, next: Next) -> Response<Body> {
    let req_method = req.method().clone();
    let req_uri = req.uri().clone();
    let req_headers = req.headers().clone();
    let start_time = Instant::now();

    let req_body_bytes = to_bytes(req.into_body(), usize::MAX)
        .await
        .unwrap_or_default();

    let req_body = redact_payload(&String::from_utf8_lossy(&req_body_bytes));
    let log_headers = redact_headers(&req_headers);

    let mut req_builder = Request::builder()
        .method(req_method.clone())
        .uri(req_uri.clone());

    req_builder = req_headers
        .iter()
        .fold(req_builder, |b, (key, value)| b.header(key, value));

    let new_request = req_builder.body(Body::from(req_body_bytes)).unwrap();

    let response = next.run(new_request).await;
    let res_status = response.status();
    let res_headers = response.headers().clone();

    let res_body_bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_else(|_| {
            tracing::error!("Failed to read response body");
            Bytes::new()
        });

    let res_body = redact_payload(&String::from_utf8_lossy(&res_body_bytes));
    let duration = start_time.elapsed();

    if res_status == StatusCode::INTERNAL_SERVER_ERROR {
        tracing::error!(
            "\n>> {} - {} {}\n>> Duration: {:.2?}\n>> Request Headers: {:?}\n>> Request Body: {}\n>> Response Headers: {:?}\n>> Response Body: {}",
            res_status,
            req_method,
            req_uri,
            duration,
            log_headers,
            req_body,
            res_headers,
            res_body
        );
    } else {
        tracing::info!(
            "\n>> {} - {} {}\n>> Duration: {:.2?}\n>> Request Headers: {:?}\n>> Request Body: {}\n>> Response Headers: {:?}\n>> Response Body: {}",
            res_status,
            req_method,
            req_uri,
            duration,
            log_headers,
            req_body,
            res_headers,
            res_body
        );
    }

    let mut res_builder = Response::builder().status(res_status);

    res_builder = res_headers
        .iter()
        .fold(res_builder, |b, (key, value)| b.header(key, value));

    res_builder.body(Body::from(res_body_bytes)).unwrap()
}

fn redact_headers(headers: &axum::http::HeaderMap) -> axum::http::HeaderMap {
    let mut redacted = headers.clone();
    for name in ["authorization", "cookie", "set-cookie", "x-api-key"] {
        if redacted.contains_key(name) {
            redacted.insert(name, axum::http::HeaderValue::from_static("[REDACTED]"));
        }
    }
    redacted
}

fn redact_payload(payload: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(payload) else {
        return if payload.len() > 4096 {
            format!("{}…[truncated]", &payload[..4096])
        } else {
            payload.to_owned()
        };
    };
    redact_json(&mut value);
    serde_json::to_string(&value).unwrap_or_else(|_| "[REDACTED]".to_owned())
}

fn redact_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                let normalized = key.to_ascii_lowercase();
                if normalized.contains("token")
                    || normalized.contains("password")
                    || normalized.contains("secret")
                    || normalized.contains("signature")
                    || normalized == "deviceid"
                    || normalized == "device_id"
                    || normalized == "sessionid"
                    || normalized == "session_id"
                {
                    *child = serde_json::Value::String("[REDACTED]".to_owned());
                } else {
                    redact_json(child);
                }
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(redact_json),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::redact_payload;

    #[test]
    fn credentials_and_device_ids_are_not_logged() {
        let logged = redact_payload(
            r#"{"token":"secret-token","pwd":"kept-by-schema","password":"secret-password","deviceId":"device-1","nested":{"refreshToken":"refresh"}}"#,
        );
        assert!(!logged.contains("secret-token"));
        assert!(!logged.contains("secret-password"));
        assert!(!logged.contains("device-1"));
        assert!(!logged.contains("\"refresh\":\""));
    }
}
