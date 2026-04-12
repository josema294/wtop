pub mod cpu;
pub mod disk;
pub mod filesystem;
pub mod gpu;
pub mod loadavg;
pub mod memory;
pub mod network;
pub mod process;

use crate::models::SystemMetrics;
use cpu::RaplState;
use gpu::GpuSources;
use sysinfo::{Disks, Networks, System};

pub struct MetricsCollector {
    pub sys: System,
    pub networks: Networks,
    pub disks: Disks,
    pub gpu_sources: GpuSources,
    pub rapl: RaplState,
    pub is_container: bool,
}

impl MetricsCollector {
    pub fn new(gpu_sources: GpuSources) -> Self {
        let is_container = detect_container();
        if is_container {
            tracing::info!("Running inside a container");
        }
        Self {
            sys: System::new_all(),
            networks: Networks::new_with_refreshed_list(),
            disks: Disks::new_with_refreshed_list(),
            gpu_sources,
            rapl: RaplState::new(),
            is_container,
        }
    }

    pub fn collect(&mut self) -> SystemMetrics {
        self.sys.refresh_all();
        self.networks.refresh(true);

        let os_name = System::name().unwrap_or_else(|| "Unknown".to_owned());
        let os_version = System::os_version().unwrap_or_else(|| "Unknown".to_owned());
        let hostname = System::host_name().unwrap_or_else(|| "Unknown".to_owned());
        let uptime = System::uptime();

        SystemMetrics {
            os_name,
            os_version,
            hostname,
            uptime,
            is_container: self.is_container,
            load_avg: loadavg::collect(),
            cpu: cpu::collect(&self.sys, &mut self.rapl),
            mem: memory::collect(&self.sys),
            net: network::collect(&self.networks),
            gpu: gpu::collect(&self.gpu_sources),
            disk_io: disk::collect(),
            filesystems: filesystem::collect(&mut self.disks),
            processes: process::collect(&self.sys),
        }
    }
}

fn detect_container() -> bool {
    // Check common container indicators
    std::path::Path::new("/.dockerenv").exists()
        || std::fs::read_to_string("/proc/1/cgroup")
            .map(|c| c.contains("docker") || c.contains("kubepods") || c.contains("containerd"))
            .unwrap_or(false)
}
