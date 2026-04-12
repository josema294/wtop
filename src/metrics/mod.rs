pub mod cpu;
pub mod disk;
pub mod gpu;
pub mod memory;
pub mod network;
pub mod process;

use crate::models::SystemMetrics;
use gpu::GpuSource;
use sysinfo::{Networks, System};

pub fn collect_all(
    sys: &mut System,
    networks: &mut Networks,
    gpu_source: &GpuSource,
) -> SystemMetrics {
    sys.refresh_all();
    networks.refresh(true);

    let os_name = System::name().unwrap_or_else(|| "Unknown".to_owned());
    let os_version = System::os_version().unwrap_or_else(|| "Unknown".to_owned());
    let hostname = System::host_name().unwrap_or_else(|| "Unknown".to_owned());
    let uptime = System::uptime();

    SystemMetrics {
        os_name,
        os_version,
        hostname,
        uptime,
        cpu: cpu::collect(sys),
        mem: memory::collect(sys),
        net: network::collect(networks),
        gpu: gpu::collect(gpu_source),
        disk_io: disk::collect(),
        processes: process::collect(sys),
    }
}
