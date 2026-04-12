use crate::models::FilesystemInfo;
use sysinfo::Disks;

pub fn collect(disks: &mut Disks) -> Vec<FilesystemInfo> {
    disks.refresh(true);
    disks
        .iter()
        .map(|disk| {
            let total = disk.total_space();
            let available = disk.available_space();
            FilesystemInfo {
                mount_point: disk.mount_point().to_string_lossy().to_string(),
                fs_type: disk.file_system().to_string_lossy().to_string(),
                total_bytes: total,
                used_bytes: total.saturating_sub(available),
                available_bytes: available,
            }
        })
        .collect()
}
