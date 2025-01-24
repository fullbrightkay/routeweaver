use serde::Deserialize;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::BluetoothTransport;

#[derive(Default, Clone, Deserialize, Debug)]
pub struct BluetoothTransportConfig {
    pub listen_psm: Option<u16>,
}
