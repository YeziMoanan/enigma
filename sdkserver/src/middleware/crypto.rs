use axum::{
    body::{Body, Bytes, to_bytes},
    extract::Request,
    http::{HeaderValue, StatusCode, header},
    middleware::Next,
    response::Response,
};
use flate2::{
    Compression,
    read::{GzDecoder, GzEncoder},
};
use std::io::Read;

fn swap_each_two_bytes(bytes: &mut [u8]) {
    let length = bytes.len();
    let hlength = length / 2;
    let mut v7 = 0;
    let mut v4 = 0;

    while v4 < hlength {
        if v7 >= length {
            break;
        }

        bytes[v7] ^= 237;

        let v5 = v7 + 1;
        if v5 >= length {
            break;
        }

        bytes[v5] ^= 237;
        bytes.swap(v7, v5);

        v7 += 2;
        v4 += 1;
    }
}

pub async fn sdk_encryption(req: Request<Body>, next: Next) -> Response<Body> {
    let (mut parts, body) = req.into_parts();

    let req_body_bytes = to_bytes(body, usize::MAX)
        .await
        .unwrap_or_else(|_| Bytes::new());

    let mut decoder = GzDecoder::new(&req_body_bytes[..]);
    let mut decompressed = Vec::with_capacity(req_body_bytes.len());

    if let Err(e) = decoder.read_to_end(&mut decompressed) {
        tracing::error!("Failed to decompress request: {}", e);
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::from("Invalid request body"))
            .unwrap();
    }

    swap_each_two_bytes(&mut decompressed);

    parts.headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    let req = Request::from_parts(parts, Body::from(decompressed));

    let response = next.run(req).await;

    let res_status = response.status();
    let res_headers = response.headers().clone();
    let res_body_bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_else(|_| Bytes::new());

    let mut raw = res_body_bytes.to_vec();
    swap_each_two_bytes(&mut raw);

    let mut encoder = GzEncoder::new(&*raw, Compression::default());
    let mut encrypted = Vec::with_capacity(raw.len());

    if let Err(e) = encoder.read_to_end(&mut encrypted) {
        tracing::error!("Failed to compress response: {}", e);
        return Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from("Failed to encode response"))
            .unwrap();
    }

    let mut res_builder = Response::builder().status(res_status);
    res_builder = res_headers
        .iter()
        .fold(res_builder, |b, (k, v)| b.header(k, v));

    res_builder.body(Body::from(encrypted)).unwrap()
}
