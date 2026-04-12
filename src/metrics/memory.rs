use crate::models::MemInfo;
use sysinfo::System;

pub fn collect(sys: &System) -> MemInfo {
    MemInfo {
        total_mem: sys.total_memory(),
        used_mem: sys.used_memory(),
        total_swap: sys.total_swap(),
        used_swap: sys.used_swap(),
    }
}
