use crate::models::ProcessInfo;
use sysinfo::System;

pub fn collect(sys: &System) -> Vec<ProcessInfo> {
    let mut processes: Vec<ProcessInfo> = sys
        .processes()
        .iter()
        .map(|(pid, proc_)| {
            let status = format!("{:?}", proc_.status());
            let cmd: String = proc_
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ");
            let disk_usage = proc_.disk_usage();

            ProcessInfo {
                pid: pid.as_u32(),
                name: proc_.name().to_string_lossy().to_string(),
                status,
                cmd: if cmd.is_empty() {
                    proc_.name().to_string_lossy().to_string()
                } else {
                    cmd
                },
                cpu_usage: proc_.cpu_usage(),
                mem_usage: proc_.memory(),
                disk_read: disk_usage.read_bytes,
                disk_write: disk_usage.written_bytes,
                user: proc_
                    .user_id()
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "root".to_string()),
            }
        })
        .collect();

    processes.sort_by(|a, b| {
        b.cpu_usage
            .partial_cmp(&a.cpu_usage)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    processes
}
