use std::fs;

use crate::models::DiskIoEntry;

/// Sector size in bytes used by Linux /proc/diskstats.
const SECTOR_SIZE: u64 = 512;

pub fn collect() -> Vec<DiskIoEntry> {
    parse_diskstats()
}

fn parse_diskstats() -> Vec<DiskIoEntry> {
    let contents = match fs::read_to_string("/proc/diskstats") {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();

    for line in contents.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 14 {
            continue;
        }

        let name = parts[2];
        let is_physical_disk = (name.starts_with("sd") && name.len() == 3)
            || (name.starts_with("nvme") && name.len() == 7);

        if is_physical_disk {
            if let (Ok(r), Ok(w)) = (parts[5].parse::<u64>(), parts[9].parse::<u64>()) {
                entries.push(DiskIoEntry {
                    name: name.to_string(),
                    read_bytes: r * SECTOR_SIZE,
                    write_bytes: w * SECTOR_SIZE,
                });
            }
        }
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_diskstats_line() {
        let line = "   8       0 sda 1000 0 2000 0 500 0 3000 0 0 0 0 0 0 0";
        let parts: Vec<&str> = line.split_whitespace().collect();
        assert!(parts.len() >= 14);
        assert_eq!(parts[2], "sda");
        assert!(parts[2].starts_with("sd") && parts[2].len() == 3);
        assert_eq!(parts[5].parse::<u64>().unwrap(), 2000);
        assert_eq!(parts[9].parse::<u64>().unwrap(), 3000);
    }

    #[test]
    fn ignore_partitions() {
        let name = "sda1";
        assert!(!(name.starts_with("sd") && name.len() == 3));

        let name = "sda";
        assert!(name.starts_with("sd") && name.len() == 3);
    }

    #[test]
    fn nvme_detection() {
        let name = "nvme0n1";
        assert!(name.starts_with("nvme") && name.len() == 7);

        let name = "nvme0n1p1";
        assert!(!(name.starts_with("nvme") && name.len() == 7));
    }

    #[test]
    fn sector_size_conversion() {
        assert_eq!(100 * SECTOR_SIZE, 51200);
    }
}
