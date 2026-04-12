use std::fs;

use crate::models::DiskIoInfo;

/// Sector size in bytes used by Linux /proc/diskstats.
const SECTOR_SIZE: u64 = 512;

pub fn collect() -> DiskIoInfo {
    let (read_sectors, write_sectors) = parse_diskstats();
    DiskIoInfo {
        read_bytes: read_sectors * SECTOR_SIZE,
        write_bytes: write_sectors * SECTOR_SIZE,
    }
}

fn parse_diskstats() -> (u64, u64) {
    let mut read_sectors = 0u64;
    let mut write_sectors = 0u64;

    let contents = match fs::read_to_string("/proc/diskstats") {
        Ok(c) => c,
        Err(_) => return (0, 0),
    };

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
                read_sectors += r;
                write_sectors += w;
            }
        }
    }

    (read_sectors, write_sectors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_diskstats_line() {
        // Simulates parsing logic with known data
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
        let is_physical = name.starts_with("sd") && name.len() == 3;
        assert!(!is_physical);

        let name = "sda";
        let is_physical = name.starts_with("sd") && name.len() == 3;
        assert!(is_physical);
    }

    #[test]
    fn nvme_detection() {
        // nvme0n1 = 7 chars = physical, nvme0n1p1 = partition
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
