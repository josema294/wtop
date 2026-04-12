use std::convert::Infallible;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Query, State},
    http::{StatusCode, Uri, header},
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
};
use futures::stream::Stream;
use serde::Deserialize;
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

/// GET /api/metrics — JSON snapshot of current metrics
pub async fn api_metrics_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut rx = state.tx.subscribe();
    match rx.recv().await {
        Ok(metrics) => match serde_json::to_string(&metrics) {
            Ok(json) => Response::builder()
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json))
                .unwrap_or_else(|_| Response::new(Body::from("{}")))
                .into_response(),
            Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Serialization error").into_response(),
        },
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, "No metrics available").into_response(),
    }
}

#[derive(Deserialize)]
pub struct ExportParams {
    #[serde(default = "default_format")]
    format: String,
}

fn default_format() -> String {
    "json".to_string()
}

/// GET /api/export?format=json|csv — Download metrics as file
pub async fn api_export_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ExportParams>,
) -> impl IntoResponse {
    let mut rx = state.tx.subscribe();
    let metrics = match rx.recv().await {
        Ok(m) => m,
        Err(_) => return (StatusCode::SERVICE_UNAVAILABLE, "No metrics available").into_response(),
    };

    match params.format.as_str() {
        "csv" => {
            let mut csv = String::from("metric,value\n");
            csv.push_str(&format!("hostname,{}\n", metrics.hostname));
            csv.push_str(&format!("os,{} {}\n", metrics.os_name, metrics.os_version));
            csv.push_str(&format!("uptime_s,{}\n", metrics.uptime));
            csv.push_str(&format!("cpu_usage_pct,{:.1}\n", metrics.cpu.global_usage));
            csv.push_str(&format!("cpu_temp_c,{:.1}\n", metrics.cpu.global_temp));
            csv.push_str(&format!("cpu_power_w,{:.1}\n", metrics.cpu.power_w));
            csv.push_str(&format!("mem_used_bytes,{}\n", metrics.mem.used_mem));
            csv.push_str(&format!("mem_total_bytes,{}\n", metrics.mem.total_mem));
            csv.push_str(&format!(
                "load_avg,{:.2} {:.2} {:.2}\n",
                metrics.load_avg.one, metrics.load_avg.five, metrics.load_avg.fifteen
            ));
            for gpu in &metrics.gpu {
                csv.push_str(&format!("gpu_name,{}\n", gpu.name));
                csv.push_str(&format!("gpu_load_pct,{}\n", gpu.load));
                csv.push_str(&format!("gpu_temp_c,{}\n", gpu.temp));
                csv.push_str(&format!("gpu_power_w,{}\n", gpu.power_w));
            }
            Response::builder()
                .header(header::CONTENT_TYPE, "text/csv")
                .header(
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"wtop-metrics.csv\"",
                )
                .body(Body::from(csv))
                .unwrap_or_else(|_| Response::new(Body::from("")))
                .into_response()
        }
        _ => {
            // Default: JSON
            match serde_json::to_string_pretty(&metrics) {
                Ok(json) => Response::builder()
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(
                        header::CONTENT_DISPOSITION,
                        "attachment; filename=\"wtop-metrics.json\"",
                    )
                    .body(Body::from(json))
                    .unwrap_or_else(|_| Response::new(Body::from("{}")))
                    .into_response(),
                Err(_) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, "Serialization error").into_response()
                }
            }
        }
    }
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

/// Combined security headers + optional Bearer token auth
pub async fn security_and_auth(
    State(state): State<Arc<AppState>>,
    req: axum::http::Request<Body>,
    next: axum::middleware::Next,
) -> Response {
    // Check auth if token is configured
    if let Some(expected_token) = &state.auth_token {
        let authorized = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .map(|v| v == format!("Bearer {expected_token}"))
            .unwrap_or(false);

        if !authorized {
            return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
        }
    }

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
