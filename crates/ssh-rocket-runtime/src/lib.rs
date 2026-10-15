pub mod dns_router;
pub mod forward;
pub mod ipc;
pub mod proxy_connector;
pub mod system;
mod helper_client;
mod ssh;

pub const DNS_ROUTER_PORT: u16 = 15353;

pub use forward::{
    ForwardEvent, ForwardManager, ForwardSession, ForwardStart, ForwardState,
};
pub use helper_client::PrivilegedHelperSession;
pub use ipc::{HelperCommand, HelperEvent};
pub use proxy_connector::connect_stdio as proxy_connect_stdio;
pub use ssh::{check_dns_health, check_socks_health, wait_for_tcp, SshSession};
