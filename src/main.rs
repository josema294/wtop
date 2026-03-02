use axum::{
    Router,
    extract::State,
    response::sse::{Event, Sse},
    routing::get,
};
use futures::stream::Stream;
use nvml_wrapper::Nvml;
use serde::Serialize;
use std::{convert::Infallible, fs, sync::Arc, time::Duration};
use sysinfo::{Networks, System};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tower_http::services::ServeDir;

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
    pub global_freq: f32, // Average GHz
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

fn get_sysfs_gpu_info() -> Option<GpuInfo> {
    if let Ok(entries) = fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = path.file_name().unwrap_or_default().to_string_lossy();
            if file_name.starts_with("card") && !file_name.contains('-') {
                let device_path = path.join("device");
                if device_path.exists() {
                    let mut gpu = GpuInfo {
                        name: "Unknown GPU".to_string(),
                        load: 0,
                        mem_load: 0,
                        temp: 0,
                        power_w: 0,
                        vram_used: 0,
                        vram_total: 0,
                    };

                    let mut is_gpu = false;
                    if let Ok(vendor) = fs::read_to_string(device_path.join("vendor")) {
                        let vendor = vendor.trim();
                        if vendor == "0x1002" {
                            gpu.name = "AMD Radeon".to_string();
                            is_gpu = true;
                        } else if vendor == "0x8086" || vendor == "0x8087" {
                            gpu.name = "Intel Graphics".to_string();
                            is_gpu = true;
                        } else if vendor == "0x10de" {
                            gpu.name = "NVIDIA (nouveau)".to_string();
                            is_gpu = true;
                        }
                    }

                    if !is_gpu {
                        continue;
                    }

                    if let Ok(load) = fs::read_to_string(device_path.join("gpu_busy_percent")) {
                        gpu.load = load.trim().parse().unwrap_or(0);
                    }
                    if let Ok(vram_used) =
                        fs::read_to_string(device_path.join("mem_info_vram_used"))
                    {
                        gpu.vram_used = vram_used.trim().parse().unwrap_or(0);
                    }
                    if let Ok(vram_total) =
                        fs::read_to_string(device_path.join("mem_info_vram_total"))
                    {
                        gpu.vram_total = vram_total.trim().parse().unwrap_or(0);
                    }

                    if gpu.vram_total > 0 {
                        gpu.mem_load =
                            ((gpu.vram_used as f64 / gpu.vram_total as f64) * 100.0) as u32;
                    }

                    if let Ok(hwmon_entries) = fs::read_dir(device_path.join("hwmon")) {
                        for hwmon in hwmon_entries.flatten() {
                            let hwmon_path = hwmon.path();
                            if let Ok(temp1_input) =
                                fs::read_to_string(hwmon_path.join("temp1_input"))
                            {
                                gpu.temp = (temp1_input.trim().parse::<u32>().unwrap_or(0)) / 1000;
                            }
                            if let Ok(power_input) =
                                fs::read_to_string(hwmon_path.join("power1_average"))
                            {
                                gpu.power_w =
                                    (power_input.trim().parse::<u32>().unwrap_or(0)) / 1_000_000;
                            } else if let Ok(power_input) =
                                fs::read_to_string(hwmon_path.join("power1_input"))
                            {
                                gpu.power_w =
                                    (power_input.trim().parse::<u32>().unwrap_or(0)) / 1_000_000;
                            }
                        }
                    }

                    return Some(gpu);
                }
            }
        }
    }
    None
}

fn get_cpu_temp() -> (f32, Vec<f32>) {
    let mut global_temp = 0.0;
    let mut cores_temp = Vec::new();

    // Look for k10temp (AMD) or coretemp (Intel)
    if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
        for entry in entries.flatten() {
            if let Ok(name) = fs::read_to_string(entry.path().join("name")) {
                let name = name.trim();
                if name == "k10temp" || name == "coretemp" {
                    // Try to get Tctl or package temp as global
                    if let Ok(temp) = fs::read_to_string(entry.path().join("temp1_input")) {
                        global_temp = temp.trim().parse::<f32>().unwrap_or(0.0) / 1000.0;
                    }

                    // Try to get individual core temps if available (temp2, temp3...)
                    let mut i = 2;
                    while let Ok(temp) =
                        fs::read_to_string(entry.path().join(format!("temp{}_input", i)))
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

struct AppState {
    tx: broadcast::Sender<SystemMetrics>,
}

#[tokio::main]
async fn main() {
    let (tx, _) = broadcast::channel::<SystemMetrics>(16);

    let app_state = Arc::new(AppState { tx: tx.clone() });

    // Spawn the background metrics collector
    tokio::spawn(async move {
        let mut sys = System::new_all();
        let mut networks = Networks::new_with_refreshed_list();

        loop {
            tokio::time::sleep(Duration::from_millis(1500)).await;

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
            let mut cores_usage = Vec::new();
            for cpu in cpus {
                cores_usage.push(cpu.cpu_usage());
            }
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

            // Power estimation (Very basic: base + load-dependant)
            // Typical 3800X TDP is 105W, idle ~30W
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

            // MEM
            let mem = MemInfo {
                total_mem: sys.total_memory(),
                used_mem: sys.used_memory(),
                total_swap: sys.total_swap(),
                used_swap: sys.used_swap(),
            };

            // NET (Aggregate all interfaces)
            let mut rx_bytes = 0;
            let mut tx_bytes = 0;
            for (_interface_name, data) in &networks {
                rx_bytes += data.received(); // Bytes received since last refresh
                tx_bytes += data.transmitted(); // Bytes transmistted since last refresh
            }

            // GPU
            let mut gpu_info = None;
            let mut nvml_success = false;
            if let Ok(nvml) = Nvml::init() {
                if let Ok(device) = nvml.device_by_index(0) {
                    nvml_success = true;
                    let name = device.name().unwrap_or_else(|_| "NVIDIA GPU".to_string());
                    let load = device.utilization_rates().map(|u| u.gpu).unwrap_or(0);
                    let mem_load = device.utilization_rates().map(|u| u.memory).unwrap_or(0);
                    // the newer nvml-wrapper doesn't need enum_wrappers for temperature in some cases or its simpler, let's keep it clean
                    let temp = device
                        .temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu)
                        .unwrap_or(0);

                    let power = device.power_usage().unwrap_or(0) / 1000; // milliwatts to watts
                    let memory = device.memory_info().ok();

                    gpu_info = Some(GpuInfo {
                        name,
                        load,
                        mem_load,
                        temp,
                        power_w: power as u32,
                        vram_used: memory.as_ref().map(|m| m.used).unwrap_or(0),
                        vram_total: memory.as_ref().map(|m| m.total).unwrap_or(0),
                    });
                }
            }

            if !nvml_success {
                // Fallback to sysfs for AMD/Intel
                gpu_info = get_sysfs_gpu_info();
            }

            // DISK I/O (Basic implementation reading /proc/diskstats for Linux)
            // Note: This reads cumulative sectors read/written since boot.
            let mut disk_read_sectors = 0;
            let mut disk_write_sectors = 0;

            if let Ok(contents) = fs::read_to_string("/proc/diskstats") {
                for line in contents.lines() {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 14 {
                        // Looking for nvme* or sd* partitions that are the parent disk not partitions
                        // To simplify, we sum all physical disks
                        let is_physical_disk = (parts[2].starts_with("sd") && parts[2].len() == 3)
                            || (parts[2].starts_with("nvme") && parts[2].len() == 7);

                        if is_physical_disk {
                            // field 5: sectors read, field 9: sectors written
                            if let (Ok(r), Ok(w)) =
                                (parts[5].parse::<u64>(), parts[9].parse::<u64>())
                            {
                                disk_read_sectors += r;
                                disk_write_sectors += w;
                            }
                        }
                    }
                }
            }

            // Assuming standard 512 byte sector size for Linux diskstats
            let disk_io = DiskIoInfo {
                read_bytes: disk_read_sectors * 512,
                write_bytes: disk_write_sectors * 512,
            };

            // PROC
            let mut processes: Vec<ProcessInfo> = sys
                .processes()
                .iter()
                .map(|(pid, proc)| ProcessInfo {
                    pid: pid.as_u32(),
                    name: proc.name().to_string_lossy().to_string(),
                    cpu_usage: proc.cpu_usage(),
                    mem_usage: proc.memory(),
                    user: proc
                        .user_id()
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| "root".to_string()),
                })
                .collect();

            // Sort by CPU usage as default
            processes.sort_by(|a, b| b.cpu_usage.partial_cmp(&a.cpu_usage).unwrap());

            let metrics = SystemMetrics {
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
            };

            // Send to all listeners
            let _ = tx.send(metrics);
        }
    });

    // Setup Axum Router
    let app = Router::new()
        .route("/events", get(sse_handler))
        .route("/version", get(version_handler))
        .fallback_service(ServeDir::new("static"))
        .with_state(app_state);

    let port = 3000;
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    println!("Wtop Backend running on http://{}", addr);

    axum::serve(listener, app).await.unwrap();
}

async fn sse_handler(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.tx.subscribe();
    let stream = BroadcastStream::new(rx);

    let event_stream = futures::stream::StreamExt::filter_map(stream, |result| async move {
        match result {
            Ok(metrics) => {
                let json = serde_json::to_string(&metrics).unwrap();
                Some(Ok(Event::default().data(json)))
            }
            Err(_) => None, // receiver lagged
        }
    });

    Sse::new(event_stream).keep_alive(axum::response::sse::KeepAlive::new())
}

async fn version_handler() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
