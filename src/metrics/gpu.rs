use std::fs;
use std::path::{Path, PathBuf};

use nvml_wrapper::Nvml;
use tracing::{info, warn};

use crate::models::GpuInfo;

/// PCI vendor IDs for known GPU manufacturers.
const VENDOR_AMD: &str = "0x1002";
const VENDOR_INTEL: &str = "0x8086";
const VENDOR_INTEL_ALT: &str = "0x8087";
const VENDOR_NVIDIA: &str = "0x10de";

/// Cached GPU source detected at startup.
pub enum GpuSource {
    Nvml(Box<Nvml>),
    Sysfs(PathBuf),
    None,
}

pub fn detect_source() -> GpuSource {
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
                        if matches!(
                            vendor,
                            VENDOR_AMD | VENDOR_INTEL | VENDOR_INTEL_ALT | VENDOR_NVIDIA
                        ) {
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

pub fn collect(source: &GpuSource) -> Option<GpuInfo> {
    match source {
        GpuSource::Nvml(nvml) => read_nvml(nvml),
        GpuSource::Sysfs(path) => read_sysfs(path),
        GpuSource::None => None,
    }
}

fn read_nvml(nvml: &Nvml) -> Option<GpuInfo> {
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

fn read_sysfs(device_path: &Path) -> Option<GpuInfo> {
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
            VENDOR_AMD => "AMD Radeon".to_string(),
            VENDOR_INTEL | VENDOR_INTEL_ALT => "Intel Graphics".to_string(),
            VENDOR_NVIDIA => "NVIDIA (nouveau)".to_string(),
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
