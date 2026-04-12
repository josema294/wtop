use crate::models::InterfaceInfo;
use sysinfo::Networks;

pub fn collect(networks: &Networks) -> Vec<InterfaceInfo> {
    networks
        .iter()
        .filter(|(name, _)| *name != "lo")
        .map(|(name, data)| InterfaceInfo {
            name: name.to_string(),
            rx_bytes: data.received(),
            tx_bytes: data.transmitted(),
        })
        .collect()
}
