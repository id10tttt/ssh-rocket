use serde::{Deserialize, Serialize};
use std::{net::IpAddr, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum HelperCommand {
    Start {
        config_path: PathBuf,
        uid: u32,
        socks_port: u16,
        dns_port: u16,
        ssh_port: u16,
        ssh_addresses: Vec<IpAddr>,
    },
    Stop,
    SyncRules,
    Status,
    Quit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum HelperEvent {
    Ready,
    Active,
    Stopped,
    RulesSynced,
    Status { active: bool },
    Error { message: String },
}
