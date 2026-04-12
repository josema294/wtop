use crate::models::NetInfo;
use sysinfo::Networks;

pub fn collect(networks: &Networks) -> NetInfo {
    let mut rx_bytes = 0;
    let mut tx_bytes = 0;
    for (_interface_name, data) in networks {
        rx_bytes += data.received();
        tx_bytes += data.transmitted();
    }
    NetInfo { rx_bytes, tx_bytes }
}
