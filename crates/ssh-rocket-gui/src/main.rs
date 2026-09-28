pub mod app;
pub mod app_scanner;
pub mod controller;
pub mod i18n;
pub mod rule_manager;
mod sys_monitor;
pub mod traffic_tracker;
mod tray;
pub mod ui;

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
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(app::build_ui);
    app.run();
}
