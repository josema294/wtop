use anyhow::{Context, Result};
use axum::{
    Router,
    body::Body,
    extract::State,
    http::{StatusCode, Uri, header},
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
    routing::get,
};
use clap::Parser;
use futures::stream::Stream;
use nvml_wrapper::Nvml;
use serde::Serialize;
use std::{convert::Infallible, fs, net::IpAddr, sync::Arc, time::Duration};
use sysinfo::{Networks, System};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tracing::{debug, error, info, warn};

const INDEX_HTML: &[u8] = include_bytes!("../static/index.html");
const LOCALES_JSON: &[u8] = include_bytes!("../static/locales.json");

#[derive(Parser, Debug)]
#[command(name = "wtop", about = "Web-based system monitor", version)]
struct Args {
    /// Port to listen on
    #[arg(short, long, default_value_t = 3000)]
    port: u16,

    /// Address to bind to
    #[arg(short, long, default_value = "0.0.0.0")]
    bind: IpAddr,

    /// Only listen on localhost (overrides --bind)
    #[arg(long)]
    localhost_only: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct SystemMetrics {
    pub os_name: String,
    pub os_version: String,
    pub hostname: String,
    pub uptime: u64,
    pub cpu: CpuInfo,
    pub mem: MemInfo,
    pub net: NetInfo,
    pub gpu: Option<GpuInfo>,
    pub disk_io: DiskIoInfo,
    pub processes: Vec<ProcessInfo>,
}

#[derive(Serialize, Clone, Debug)]
pub struct CpuInfo {
    pub global_usage: f32,
    pub cores_usage: Vec<f32>,
    pub cores_freq: Vec<u64>,
    pub cores_temp: Vec<f32>,
    pub brand: String,
    pub physical_core_count: usize,
    pub global_temp: f32,
    pub global_freq: f32,
    pub power_w: f32,
}

#[derive(Serialize, Clone, Debug)]
pub struct MemInfo {
    pub total_mem: u64,
    pub used_mem: u64,
    pub total_swap: u64,
    pub used_swap: u64,
}

#[derive(Serialize, Clone, Debug)]
pub struct NetInfo {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[derive(Serialize, Clone, Debug)]
pub struct GpuInfo {
    pub name: String,
    pub load: u32,
    pub mem_load: u32,
    pub temp: u32,
    pub power_w: u32,
    pub vram_used: u64,
    pub vram_total: u64,
}

#[derive(Serialize, Clone, Debug)]
pub struct DiskIoInfo {
    pub read_bytes: u64,
    pub write_bytes: u64,
}

#[derive(Serialize, Clone, Debug)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cpu_usage: f32,
    pub mem_usage: u64,
    pub user: String,
}

/// Cached GPU source detected at startup
enum GpuSource {
    Nvml(Box<Nvml>),
    Sysfs(std::path::PathBuf),
    None,
}

fn detect_gpu_source() -> GpuSource {
    // Try NVIDIA NVML first
    if let Ok(nvml) = Nvml::init() {
        if nvml.device_by_index(0).is_ok() {
            info!("GPU detected: NVIDIA via NVML");
            return GpuSource::Nvml(Box::new(nvml));
        }
    }

    // Fallback: scan sysfs for AMD/Intel
    if let Ok(entries) = fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = path.file_name().unwrap_or_default().to_string_lossy();
            if file_name.starts_with("card") && !file_name.contains('-') {
                let device_path = path.join("device");
                if device_path.exists() {
                    if let Ok(vendor) = fs::read_to_string(device_path.join("vendor")) {
                        let vendor = vendor.trim();
                        if matches!(vendor, "0x1002" | "0x8086" | "0x8087" | "0x10de") {
                            info!("GPU detected: sysfs at {}", device_path.display());
                            return GpuSource::Sysfs(device_path);
                        }
                    }
                }
            }
        }
    }

    warn!("No GPU detected");
    GpuSource::None
}

fn read_nvml_gpu(nvml: &Nvml) -> Option<GpuInfo> {
    let device = nvml.device_by_index(0).ok()?;
    let name = device.name().unwrap_or_else(|_| "NVIDIA GPU".to_string());
    let load = device.utilization_rates().map(|u| u.gpu).unwrap_or(0);
    let mem_load = device.utilization_rates().map(|u| u.memory).unwrap_or(0);
    let temp = device
        .temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu)
        .unwrap_or(0);
    let power = device.power_usage().unwrap_or(0) / 1000;
    let memory = device.memory_info().ok();

    Some(GpuInfo {
        name,
        load,
        mem_load,
        temp,
        power_w: power,
        vram_used: memory.as_ref().map(|m| m.used).unwrap_or(0),
        vram_total: memory.as_ref().map(|m| m.total).unwrap_or(0),
    })
}

fn read_sysfs_gpu(device_path: &std::path::Path) -> Option<GpuInfo> {
    let mut gpu = GpuInfo {
        name: "Unknown GPU".to_string(),
        load: 0,
        mem_load: 0,
        temp: 0,
        power_w: 0,
        vram_used: 0,
        vram_total: 0,
    };

    if let Ok(vendor) = fs::read_to_string(device_path.join("vendor")) {
        gpu.name = match vendor.trim() {
            "0x1002" => "AMD Radeon".to_string(),
            "0x8086" | "0x8087" => "Intel Graphics".to_string(),
            "0x10de" => "NVIDIA (nouveau)".to_string(),
            _ => return None,
        };
    }

    if let Ok(load) = fs::read_to_string(device_path.join("gpu_busy_percent")) {
        gpu.load = load.trim().parse().unwrap_or(0);
    }
    if let Ok(vram_used) = fs::read_to_string(device_path.join("mem_info_vram_used")) {
        gpu.vram_used = vram_used.trim().parse().unwrap_or(0);
    }
    if let Ok(vram_total) = fs::read_to_string(device_path.join("mem_info_vram_total")) {
        gpu.vram_total = vram_total.trim().parse().unwrap_or(0);
    }

    if gpu.vram_total > 0 {
        gpu.mem_load = ((gpu.vram_used as f64 / gpu.vram_total as f64) * 100.0) as u32;
    }

    if let Ok(hwmon_entries) = fs::read_dir(device_path.join("hwmon")) {
        for hwmon in hwmon_entries.flatten() {
            let hwmon_path = hwmon.path();
            if let Ok(temp1_input) = fs::read_to_string(hwmon_path.join("temp1_input")) {
                gpu.temp = (temp1_input.trim().parse::<u32>().unwrap_or(0)) / 1000;
            }
            if let Ok(power_input) = fs::read_to_string(hwmon_path.join("power1_average")) {
                gpu.power_w = (power_input.trim().parse::<u32>().unwrap_or(0)) / 1_000_000;
            } else if let Ok(power_input) = fs::read_to_string(hwmon_path.join("power1_input")) {
                gpu.power_w = (power_input.trim().parse::<u32>().unwrap_or(0)) / 1_000_000;
            }
        }
    }

    Some(gpu)
}

fn get_cpu_temp() -> (f32, Vec<f32>) {
    let mut global_temp = 0.0;
    let mut cores_temp = Vec::new();

    if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
        for entry in entries.flatten() {
            if let Ok(name) = fs::read_to_string(entry.path().join("name")) {
                let name = name.trim();
                if name == "k10temp" || name == "coretemp" {
                    if let Ok(temp) = fs::read_to_string(entry.path().join("temp1_input")) {
                        global_temp = temp.trim().parse::<f32>().unwrap_or(0.0) / 1000.0;
                    }

                    let mut i = 2;
                    while let Ok(temp) =
                        fs::read_to_string(entry.path().join(format!("temp{i}_input")))
                    {
                        cores_temp.push(temp.trim().parse::<f32>().unwrap_or(0.0) / 1000.0);
                        i += 1;
                    }
                    break;
                }
            }
        }
    }
    (global_temp, cores_temp)
}

fn get_cpu_frequencies() -> Vec<u64> {
    let mut freqs = Vec::new();
    if let Ok(contents) = fs::read_to_string("/proc/cpuinfo") {
        for line in contents.lines() {
            if line.starts_with("cpu MHz") {
                if let Some(mhz_str) = line.split(':').nth(1) {
                    if let Ok(mhz) = mhz_str.trim().parse::<f32>() {
                        freqs.push(mhz as u64);
                    }
                }
            }
        }
    }
    freqs
}

fn collect_metrics(sys: &mut System, networks: &mut Networks, gpu_source: &GpuSource) -> SystemMetrics {
    sys.refresh_all();
    networks.refresh(true);

    // OS
    let os_name = System::name().unwrap_or_else(|| "Unknown".to_owned());
    let os_version = System::os_version().unwrap_or_else(|| "Unknown".to_owned());
    let hostname = System::host_name().unwrap_or_else(|| "Unknown".to_owned());
    let uptime = System::uptime();

    // CPU
    let cpus = sys.cpus();
    let global_usage = sys.global_cpu_usage();
    let cores_usage: Vec<f32> = cpus.iter().map(|cpu| cpu.cpu_usage()).collect();
    let brand = cpus
        .first()
        .map(|c| c.brand().to_string())
        .unwrap_or_else(|| "Unknown".to_string());
    let physical_core_count = System::physical_core_count().unwrap_or(0);

    let (global_temp, cores_temp) = get_cpu_temp();
    let cores_freq = get_cpu_frequencies();
    let avg_freq = if !cores_freq.is_empty() {
        (cores_freq.iter().sum::<u64>() as f32 / cores_freq.len() as f32) / 1000.0
    } else {
        0.0
    };

    // Power estimation (basic: base + load-dependent)
    let power_w = 30.0 + (global_usage / 100.0 * 75.0);

    let cpu_info = CpuInfo {
        global_usage,
        cores_usage,
        cores_freq,
        cores_temp,
        brand,
        physical_core_count,
        global_temp,
        global_freq: avg_freq,
        power_w,
    };

    // Memory
    let mem = MemInfo {
        total_mem: sys.total_memory(),
        used_mem: sys.used_memory(),
        total_swap: sys.total_swap(),
        used_swap: sys.used_swap(),
    };

    // Network (aggregate all interfaces)
    let mut rx_bytes = 0;
    let mut tx_bytes = 0;
    for (_interface_name, data) in &*networks {
        rx_bytes += data.received();
        tx_bytes += data.transmitted();
    }

    // GPU (using cached source)
    let gpu_info = match gpu_source {
        GpuSource::Nvml(nvml) => read_nvml_gpu(nvml),
        GpuSource::Sysfs(path) => read_sysfs_gpu(path),
        GpuSource::None => None,
    };

    // Disk I/O
    let mut disk_read_sectors = 0u64;
    let mut disk_write_sectors = 0u64;

    if let Ok(contents) = fs::read_to_string("/proc/diskstats") {
        for line in contents.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 14 {
                let is_physical_disk = (parts[2].starts_with("sd") && parts[2].len() == 3)
                    || (parts[2].starts_with("nvme") && parts[2].len() == 7);

                if is_physical_disk {
                    if let (Ok(r), Ok(w)) = (parts[5].parse::<u64>(), parts[9].parse::<u64>()) {
                        disk_read_sectors += r;
                        disk_write_sectors += w;
                    }
                }
            }
        }
    }

    let disk_io = DiskIoInfo {
        read_bytes: disk_read_sectors * 512,
        write_bytes: disk_write_sectors * 512,
    };

    // Processes
    let mut processes: Vec<ProcessInfo> = sys
        .processes()
        .iter()
        .map(|(pid, proc_)| ProcessInfo {
            pid: pid.as_u32(),
            name: proc_.name().to_string_lossy().to_string(),
            cpu_usage: proc_.cpu_usage(),
            mem_usage: proc_.memory(),
            user: proc_
                .user_id()
                .map(|id| id.to_string())
                .unwrap_or_else(|| "root".to_string()),
        })
        .collect();

    processes.sort_by(|a, b| {
        b.cpu_usage
            .partial_cmp(&a.cpu_usage)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    SystemMetrics {
        os_name,
        os_version,
        hostname,
        uptime,
        cpu: cpu_info,
        mem,
        net: NetInfo { rx_bytes, tx_bytes },
        gpu: gpu_info,
        disk_io,
        processes,
    }
}

struct AppState {
    tx: broadcast::Sender<SystemMetrics>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    let bind_addr = if args.localhost_only {
        "127.0.0.1".parse().unwrap()
    } else {
        args.bind
    };

    let (tx, _) = broadcast::channel::<SystemMetrics>(16);
    let app_state = Arc::new(AppState { tx: tx.clone() });

    // Detect GPU once at startup
    let gpu_source = detect_gpu_source();

    // Spawn the metrics collector on a dedicated OS thread to avoid
    // blocking the async runtime with synchronous sysfs/procfs reads
    std::thread::Builder::new()
        .name("wtop-metrics".into())
        .spawn(move || {
            let mut sys = System::new_all();
            let mut networks = Networks::new_with_refreshed_list();

            loop {
                std::thread::sleep(Duration::from_millis(1500));

                let metrics = collect_metrics(&mut sys, &mut networks, &gpu_source);

                if let Err(e) = tx.send(metrics) {
                    debug!("No SSE subscribers connected: {}", e);
                }
            }
        })
        .context("Failed to spawn metrics collector thread")?;

    // Setup Axum router with security headers
    let app = Router::new()
        .route("/events", get(sse_handler))
        .route("/version", get(version_handler))
        .fallback(static_handler)
        .with_state(app_state)
        .layer(axum::middleware::from_fn(security_headers));

    let addr = format!("{}:{}", bind_addr, args.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind to {addr}"))?;

    info!("wtop v{} listening on http://{}", env!("CARGO_PKG_VERSION"), addr);
    if !args.localhost_only && bind_addr.to_string() == "0.0.0.0" {
        warn!("Listening on all interfaces. Use --localhost-only for local access only.");
    }

    axum::serve(listener, app)
        .await
        .context("Server error")?;

    Ok(())
}

async fn security_headers(
    req: axum::http::Request<Body>,
    next: axum::middleware::Next,
) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        "nosniff".parse().unwrap(),
    );
    headers.insert(
        header::X_FRAME_OPTIONS,
        "DENY".parse().unwrap(),
    );
    headers.insert(
        header::HeaderName::from_static("x-xss-protection"),
        "1; mode=block".parse().unwrap(),
    );
    response
}

async fn sse_handler(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.tx.subscribe();
    let stream = BroadcastStream::new(rx);

    let event_stream = futures::stream::StreamExt::filter_map(stream, |result| async move {
        match result {
            Ok(metrics) => {
                match serde_json::to_string(&metrics) {
                    Ok(json) => Some(Ok(Event::default().data(json))),
                    Err(e) => {
                        error!("Failed to serialize metrics: {}", e);
                        None
                    }
                }
            }
            Err(_) => None,
        }
    });

    Sse::new(event_stream).keep_alive(axum::response::sse::KeepAlive::new())
}

async fn version_handler() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

async fn static_handler(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');

    if path.is_empty() || path == "index.html" {
        return Response::builder()
            .header(header::CONTENT_TYPE, "text/html")
            .body(Body::from(INDEX_HTML))
            .unwrap_or_else(|_| {
                Response::new(Body::from("Internal Server Error"))
            })
            .into_response();
    }

    if path == "locales.json" {
        return Response::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(LOCALES_JSON))
            .unwrap_or_else(|_| {
                Response::new(Body::from("Internal Server Error"))
            })
            .into_response();
    }

    (StatusCode::NOT_FOUND, "404 Not Found").into_response()
}
