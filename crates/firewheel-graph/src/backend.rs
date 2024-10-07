#[derive(Debug, Clone, PartialEq)]
pub struct DeviceInfo {
    pub name: String,
    pub num_channels: u16,
    pub is_default: bool,
}
