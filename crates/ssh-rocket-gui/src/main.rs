pub mod app;
pub mod app_scanner;
pub mod controller;
pub mod i18n;
pub mod rule_manager;
mod sys_monitor;
pub mod traffic_tracker;
mod tray;
pub mod ui;
pub mod ssh_key;

use adw::prelude::*;
use libadwaita as adw;

pub use app_scanner::{scan_desktop_apps, DesktopApp};
pub use controller::{RuntimeController, RuntimeEvent};
pub use rule_manager::{
    custom_rules, domain_kind_label, imported_rules, remove_listed_rule, rule_source_name,
    ListedRule, RuleListState, MAX_RULE_SOURCE_SIZE,
};
pub use traffic_tracker::{ActiveConnectionStat, AppTrafficStat, ConnectionType};

pub const APP_ID: &str = "io.github.idi0t.SshRocket";
pub const SOCKS_PORT: u16 = 17880;
pub const DEFAULT_RULE_SOURCE: &str =
    "https://johnshall.github.io/Shadowrocket-ADBlock-Rules-Forever/sr_top500_banlist_ad.conf";
pub const RULE_BATCH_SIZE: usize = 20;

fn main() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("proxy-connect") {
        let host = args.next().unwrap_or_default();
        let port = args.next().and_then(|value| value.parse::<u16>().ok());
        let result = port
            .ok_or_else(|| anyhow::anyhow!("缺少有效的目标端口"))
            .and_then(|port| ssh_rocket_runtime::proxy_connect_stdio(SOCKS_PORT, &host, port));
        if let Err(error) = result {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
        return;
    }

    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(app::build_ui);
    app.run();
}
