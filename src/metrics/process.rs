use crate::models::ProcessInfo;
use sysinfo::System;

pub fn collect(sys: &System) -> Vec<ProcessInfo> {
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

    processes
}
