use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DesktopAppDto {
    pub name: String,
    pub executable: String,
    pub icon: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConnectionTypeDto {
    Proxy,
    Direct,
    Local,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActiveConnectionDto {
    pub proc_name: String,
    pub icon: String,
    pub local_addr: String,
    pub peer_addr: String,
    pub conn_type: ConnectionTypeDto,
    pub upload: u64,
    pub download: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppTrafficDto {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub upload: u64,
    pub download: u64,
    pub proxy_upload: u64,
    pub proxy_download: u64,
    pub direct_upload: u64,
    pub direct_download: u64,
    pub local_upload: u64,
    pub local_download: u64,
    pub primary_type: ConnectionTypeDto,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpeedDto {
    pub upload: u64,
    pub download: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeStatusDto {
    pub is_running: bool,
    pub status_text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuleImportSummaryDto {
    pub rule_count: usize,
    pub warnings: Vec<String>,
    pub ignored_count: usize,
}
