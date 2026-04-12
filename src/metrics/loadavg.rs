use std::fs;

use crate::models::LoadAvg;

pub fn collect() -> LoadAvg {
    if let Ok(contents) = fs::read_to_string("/proc/loadavg") {
        let parts: Vec<&str> = contents.split_whitespace().collect();
        if parts.len() >= 3 {
            return LoadAvg {
                one: parts[0].parse().unwrap_or(0.0),
                five: parts[1].parse().unwrap_or(0.0),
                fifteen: parts[2].parse().unwrap_or(0.0),
            };
        }
    }
    LoadAvg {
        one: 0.0,
        five: 0.0,
        fifteen: 0.0,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_loadavg_format() {
        // Simulate /proc/loadavg content
        let content = "0.52 0.34 0.28 1/423 12345";
        let parts: Vec<&str> = content.split_whitespace().collect();
        assert!(parts.len() >= 3);
        assert!((parts[0].parse::<f32>().unwrap() - 0.52).abs() < 0.001);
        assert!((parts[1].parse::<f32>().unwrap() - 0.34).abs() < 0.001);
        assert!((parts[2].parse::<f32>().unwrap() - 0.28).abs() < 0.001);
    }
}
