use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
pub struct SystemMetrics {
    pub os_name: String,
    pub os_version: String,
    pub hostname: String,
    pub uptime: u64,
    pub load_avg: LoadAvg,
    pub cpu: CpuInfo,
    pub mem: MemInfo,
    pub net: Vec<InterfaceInfo>,
    pub gpu: Option<GpuInfo>,
    pub disk_io: Vec<DiskIoEntry>,
    pub filesystems: Vec<FilesystemInfo>,
    pub processes: Vec<ProcessInfo>,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct LoadAvg {
    pub one: f32,
    pub five: f32,
    pub fifteen: f32,
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
pub struct InterfaceInfo {
    pub name: String,
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
pub struct DiskIoEntry {
    pub name: String,
    pub read_bytes: u64,
    pub write_bytes: u64,
}

#[derive(Serialize, Clone, Debug)]
pub struct FilesystemInfo {
    pub mount_point: String,
    pub fs_type: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Serialize, Clone, Debug)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub status: String,
    pub cmd: String,
    pub cpu_usage: f32,
    pub mem_usage: u64,
    pub disk_read: u64,
    pub disk_write: u64,
    pub user: String,
}
