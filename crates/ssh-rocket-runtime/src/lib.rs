pub mod ipc;
mod helper_client;
mod ssh;

pub const DNS_ROUTER_PORT: u16 = 15353;

pub use helper_client::PrivilegedHelperSession;
pub use ipc::{HelperCommand, HelperEvent};
pub use ssh::{check_dns_health, check_socks_health, wait_for_tcp, SshSession};
