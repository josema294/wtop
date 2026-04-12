use std::convert::Infallible;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::State,
    http::{StatusCode, Uri, header},
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
};
use futures::stream::Stream;
use tokio_stream::wrappers::BroadcastStream;
use tracing::error;

use crate::AppState;

const INDEX_HTML: &[u8] = include_bytes!("../static/index.html");
const LOCALES_JSON: &[u8] = include_bytes!("../static/locales.json");
const MANIFEST_JSON: &[u8] = include_bytes!("../static/manifest.json");
const SW_JS: &[u8] = include_bytes!("../static/sw.js");

pub async fn sse_handler(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.tx.subscribe();
    let stream = BroadcastStream::new(rx);

    let event_stream = futures::stream::StreamExt::filter_map(stream, |result| async move {
        match result {
            Ok(metrics) => match serde_json::to_string(&metrics) {
                Ok(json) => Some(Ok(Event::default().data(json))),
                Err(e) => {
                    error!("Failed to serialize metrics: {}", e);
                    None
                }
            },
            Err(_) => None,
        }
    });

    Sse::new(event_stream).keep_alive(axum::response::sse::KeepAlive::new())
}

pub async fn version_handler() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub async fn static_handler(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');

    if path.is_empty() || path == "index.html" {
        return Response::builder()
            .header(header::CONTENT_TYPE, "text/html")
            .body(Body::from(INDEX_HTML))
            .unwrap_or_else(|_| Response::new(Body::from("Internal Server Error")))
            .into_response();
    }

    if path == "locales.json" {
        return Response::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(LOCALES_JSON))
            .unwrap_or_else(|_| Response::new(Body::from("Internal Server Error")))
            .into_response();
    }

    if path == "manifest.json" {
        return Response::builder()
            .header(header::CONTENT_TYPE, "application/manifest+json")
            .body(Body::from(MANIFEST_JSON))
            .unwrap_or_else(|_| Response::new(Body::from("Internal Server Error")))
            .into_response();
    }

    if path == "sw.js" {
        return Response::builder()
            .header(header::CONTENT_TYPE, "application/javascript")
            .header("service-worker-allowed", "/")
            .body(Body::from(SW_JS))
            .unwrap_or_else(|_| Response::new(Body::from("Internal Server Error")))
            .into_response();
    }

    (StatusCode::NOT_FOUND, "404 Not Found").into_response()
}

pub async fn security_headers(
    req: axum::http::Request<Body>,
    next: axum::middleware::Next,
) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    headers.insert(header::X_FRAME_OPTIONS, "DENY".parse().unwrap());
    headers.insert(
        header::HeaderName::from_static("x-xss-protection"),
        "1; mode=block".parse().unwrap(),
    );
    response
}
