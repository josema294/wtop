use std::fs;
use std::time::{Duration, Instant};

use crate::models::CpuInfo;
use sysinfo::System;

/// Base power draw in watts when CPU is idle (estimation fallback).
const CPU_POWER_IDLE_W: f32 = 30.0;
/// Additional power draw in watts at 100% CPU load (estimation fallback).
const CPU_POWER_LOAD_W: f32 = 75.0;

/// Tracks RAPL energy readings between samples to compute real power.
pub struct RaplState {
    last_energy_uj: Option<u64>,
    last_read: Instant,
    rapl_path: Option<String>,
}

impl RaplState {
    pub fn new() -> Self {
        // Detect RAPL path at init
        let rapl_path = find_rapl_path();
        if rapl_path.is_some() {
            tracing::info!("RAPL power monitoring available");
        } else {
            tracing::info!("RAPL not available, using power estimation");
        }
        Self {
            last_energy_uj: None,
            last_read: Instant::now(),
            rapl_path,
        }
    }

    fn read_power(&mut self) -> Option<f32> {
        let path = self.rapl_path.as_ref()?;
        let energy_uj: u64 = fs::read_to_string(path).ok()?.trim().parse().ok()?;
        let now = Instant::now();

        let power = if let Some(last) = self.last_energy_uj {
            let elapsed = now.duration_since(self.last_read);
            if elapsed > Duration::from_millis(100) {
                let delta_uj = if energy_uj >= last {
                    energy_uj - last
                } else {
                    // Counter wrapped around
                    energy_uj
                };
                Some(delta_uj as f32 / elapsed.as_micros() as f32) // µJ / µs = W
            } else {
                None
            }
        } else {
            None
        };

        self.last_energy_uj = Some(energy_uj);
        self.last_read = now;
        power
    }
}

fn find_rapl_path() -> Option<String> {
    // Try intel-rapl and amd-rapl
    for base in &[
        "/sys/class/powercap/intel-rapl:0/energy_uj",
        "/sys/class/powercap/amd-rapl:0/energy_uj",
    ] {
        if fs::metadata(base).is_ok() {
            return Some(base.to_string());
        }
    }
    None
}

pub fn collect(sys: &System, rapl: &mut RaplState) -> CpuInfo {
    let cpus = sys.cpus();
    let global_usage = sys.global_cpu_usage();
    let cores_usage: Vec<f32> = cpus.iter().map(|cpu| cpu.cpu_usage()).collect();
    let brand = cpus
        .first()
        .map(|c| c.brand().to_string())
        .unwrap_or_else(|| "Unknown".to_string());
    let physical_core_count = System::physical_core_count().unwrap_or(0);

    let (global_temp, cores_temp) = read_temperatures();
    let cores_freq = read_frequencies();
    let avg_freq = if !cores_freq.is_empty() {
        (cores_freq.iter().sum::<u64>() as f32 / cores_freq.len() as f32) / 1000.0
    } else {
        0.0
    };

    // Use RAPL if available, otherwise estimate
    let power_w = rapl
        .read_power()
        .unwrap_or_else(|| CPU_POWER_IDLE_W + (global_usage / 100.0 * CPU_POWER_LOAD_W));

    CpuInfo {
        global_usage,
        cores_usage,
        cores_freq,
        cores_temp,
        brand,
        physical_core_count,
        global_temp,
        global_freq: avg_freq,
        power_w,
    }
}

fn read_temperatures() -> (f32, Vec<f32>) {
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

fn read_frequencies() -> Vec<u64> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_estimation_idle() {
        let power = CPU_POWER_IDLE_W + (0.0 / 100.0 * CPU_POWER_LOAD_W);
        assert!((power - 30.0).abs() < f32::EPSILON);
    }

    #[test]
    fn power_estimation_full_load() {
        let power = CPU_POWER_IDLE_W + (100.0 / 100.0 * CPU_POWER_LOAD_W);
        assert!((power - 105.0).abs() < f32::EPSILON);
    }

    #[test]
    fn rapl_path_detection() {
        // Just verify the function doesn't panic
        let _ = find_rapl_path();
    }
}
