mod tray;

use adw::prelude::*;
use gtk4::{self as gtk, gio};
use libadwaita as adw;
use ssh_rocket_core::{
    AppConfig, AppRule, DomainRule, DomainRuleKind, IpRule, Profile, RuleAction, RuleImportResult,
    parse_omega_rules, parse_rule_set, parse_shadowrocket_rules,
};
use ssh_rocket_runtime::{PrivilegedHelperSession, SshSession};
use tray::{TrayConnectionState, TrayManager};
use std::{
    cell::RefCell,
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    process::Command as StdCommand,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::{oneshot, Mutex},
};

const APP_ID: &str = "io.github.idi0t.SshRocket";
const SOCKS_PORT: u16 = 17880;
const DEFAULT_RULE_SOURCE: &str = "https://johnshall.github.io/Shadowrocket-ADBlock-Rules-Forever/sr_top500_banlist_ad.conf";
const MAX_RULE_SOURCE_SIZE: usize = 16 * 1024 * 1024;
const RULE_BATCH_SIZE: usize = 20;

#[derive(Clone, Debug, Default)]
struct AppTrafficStat {
    id: String,
    name: String,
    icon: String,
    upload: u64,
    download: u64,
}

enum RuntimeEvent {
    Connected,
    Disconnected,
    Status(String),
    Error(String),
    Log(String),
    Speed { upload: u64, download: u64 },
    AppTraffic(Vec<AppTrafficStat>),
    RuleImportFailed(String),
    RulesImported {
        result: RuleImportResult,
        source_url: String,
    },
}

#[derive(Clone, Default)]
struct RuntimeController {
    stop: Rc<RefCell<Option<oneshot::Sender<()>>>>,
    helper: Arc<Mutex<Option<PrivilegedHelperSession>>>,
    is_running: Arc<AtomicBool>,
}

#[derive(Clone)]
struct DesktopApp {
    name: String,
    executable: String,
    icon: String,
}

type RefreshConnections = Rc<RefCell<Option<Rc<dyn Fn()>>>>;
type RefreshRules = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

#[derive(Clone)]
enum ListedRule {
    Domain(DomainRule),
    Ip(IpRule),
}

impl ListedRule {
    fn value(&self) -> String {
        match self {
            Self::Domain(rule) => rule.pattern.clone(),
            Self::Ip(rule) => rule.network.to_string(),
        }
    }

    fn kind_label(&self) -> &'static str {
        match self {
            Self::Domain(rule) => domain_kind_label(rule.kind),
            Self::Ip(_) => "IP-CIDR",
        }
    }

    fn action(&self) -> RuleAction {
        match self {
            Self::Domain(rule) => rule.action,
            Self::Ip(rule) => rule.action,
        }
    }

    fn matches(&self, query: &str) -> bool {
        query.is_empty()
            || self.value().to_lowercase().contains(query)
            || self.kind_label().to_lowercase().contains(query)
            || action_label(self.action()).to_lowercase().contains(query)
    }
}

#[derive(Default)]
struct RuleListState {
    filtered: Vec<ListedRule>,
    rendered_rows: Vec<adw::ActionRow>,
    loaded: usize,
}

fn action_label(action: RuleAction) -> &'static str {
    match action {
        RuleAction::Direct => "DIRECT",
        RuleAction::Proxy => "PROXY",
        RuleAction::Block => "REJECT",
    }
}

fn domain_kind_label(kind: DomainRuleKind) -> &'static str {
    match kind {
        DomainRuleKind::Domain => "DOMAIN",
        DomainRuleKind::DomainSuffix => "DOMAIN-SUFFIX",
        DomainRuleKind::DomainKeyword => "DOMAIN-KEYWORD",
        DomainRuleKind::Legacy => "LEGACY",
    }
}

fn custom_rules(config: &AppConfig) -> Vec<ListedRule> {
    config
        .settings
        .domain_rules
        .iter()
        .cloned()
        .map(ListedRule::Domain)
        .chain(config.settings.ip_rules.iter().cloned().map(ListedRule::Ip))
        .collect()
}

fn imported_rules(config: &AppConfig) -> Vec<ListedRule> {
    config
        .settings
        .imported_domain_rules
        .iter()
        .cloned()
        .map(ListedRule::Domain)
        .chain(config.settings.imported_ip_rules.iter().cloned().map(ListedRule::Ip))
        .collect()
}

fn rule_source_name(source_url: &str) -> String {
    source_url
        .split(['?', '#'])
        .next()
        .and_then(|url| url.rsplit('/').find(|part| !part.is_empty()))
        .filter(|name| !name.is_empty())
        .unwrap_or("Imported Configuration")
        .to_string()
}

fn remove_listed_rule(config: &mut AppConfig, rule: &ListedRule) {
    match rule {
        ListedRule::Domain(rule) => config
            .settings
            .domain_rules
            .retain(|item| !(item.pattern == rule.pattern && item.kind == rule.kind)),
        ListedRule::Ip(rule) => config.settings.ip_rules.retain(|item| item.network != rule.network),
    }
}

fn show_rule_dialog(
    parent: &adw::ApplicationWindow,
    config: Rc<RefCell<AppConfig>>,
    existing: Option<ListedRule>,
    refresh_rules: RefreshRules,
) {
    let dialog = adw::AlertDialog::new(
        Some(if existing.is_some() { "Edit Rule" } else { "Add Rule" }),
        None,
    );
    let group = adw::PreferencesGroup::new();
    let pattern = adw::EntryRow::builder()
        .title("Domain, IP, or CIDR")
        .text(existing.as_ref().map(ListedRule::value).unwrap_or_default())
        .build();
    let rule_type = adw::ComboRow::builder()
        .title("Rule Type")
        .model(&gtk::StringList::new(&[
            "DOMAIN-SUFFIX",
            "DOMAIN",
            "DOMAIN-KEYWORD",
            "IP-CIDR",
        ]))
        .selected(match existing.as_ref().map(ListedRule::kind_label) {
            Some("DOMAIN") => 1,
            Some("DOMAIN-KEYWORD") => 2,
            Some("IP-CIDR") => 3,
            _ => 0,
        })
        .build();
    let action = adw::ComboRow::builder()
        .title("Action")
        .model(&gtk::StringList::new(&["DIRECT", "PROXY", "REJECT"]))
        .selected(match existing.as_ref().map(ListedRule::action) {
            Some(RuleAction::Direct) => 0,
            Some(RuleAction::Block) => 2,
            _ => 1,
        })
        .build();
    group.add(&pattern);
    group.add(&rule_type);
    group.add(&action);
    dialog.set_extra_child(Some(&group));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("save", "Save");
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.connect_response(None, move |dialog, response| {
        if response != "save" {
            return;
        }
        let value = pattern.text().trim().to_string();
        if value.is_empty() {
            dialog.set_body("Enter a domain, IP, or CIDR.");
            return;
        }
        let rule_type_text = match rule_type.selected() {
            1 => "DOMAIN",
            2 => "DOMAIN-KEYWORD",
            3 => "IP-CIDR",
            _ => "DOMAIN-SUFFIX",
        };
        let selected_action = match action.selected() {
            0 => RuleAction::Direct,
            2 => RuleAction::Block,
            _ => RuleAction::Proxy,
        };
        let mut parsed = parse_rule_set(&format!("{rule_type_text},{value}"), selected_action);
        if parsed.rule_count() != 1 {
            dialog.set_body("The rule is invalid.");
            return;
        }

        let mut current = config.borrow_mut();
        if let Some(existing) = &existing {
            remove_listed_rule(&mut current, existing);
        }
        if let Some(rule) = parsed.domain_rules.pop() {
            current.settings.domain_rules.retain(|item| {
                !(item.pattern == rule.pattern && item.kind == rule.kind)
            });
            current.settings.domain_rules.push(rule);
        } else if let Some(rule) = parsed.ip_rules.pop() {
            current.settings.ip_rules.retain(|item| item.network != rule.network);
            current.settings.ip_rules.push(rule);
        }
        if current.save().is_ok() {
            drop(current);
            if let Some(refresh) = refresh_rules.borrow().as_ref() {
                refresh();
            }
        }
    });
    dialog.present(Some(parent));
}

fn append_rule_batch(
    group: &adw::PreferencesGroup,
    state: &Rc<RefCell<RuleListState>>,
    load_more: &gtk::Button,
    editable: bool,
    parent: &adw::ApplicationWindow,
    config: &Rc<RefCell<AppConfig>>,
    refresh_rules: &RefreshRules,
) {
    let items = {
        let mut state = state.borrow_mut();
        let end = (state.loaded + RULE_BATCH_SIZE).min(state.filtered.len());
        let items = state.filtered[state.loaded..end].to_vec();
        state.loaded = end;
        items
    };
    for item in items {
        let row = adw::ActionRow::builder()
            .title(item.value())
            .subtitle(item.kind_label())
            .build();
        row.set_use_markup(false);
        let icon_name = match item.action() {
            RuleAction::Proxy => "ssh-rocket-symbolic",
            RuleAction::Direct => "network-wired-symbolic",
            RuleAction::Block => "network-offline-symbolic",
        };
        row.add_prefix(&gtk::Image::from_icon_name(icon_name));
        let action = gtk::Label::new(Some(action_label(item.action())));
        action.add_css_class("dim-label");
        row.add_suffix(&action);
        if editable {
            let edit = gtk::Button::from_icon_name("document-edit-symbolic");
            edit.add_css_class("flat");
            edit.set_tooltip_text(Some("Edit Rule"));
            let edit_parent = parent.clone();
            let edit_config = config.clone();
            let edit_rule = item.clone();
            let edit_refresh = refresh_rules.clone();
            edit.connect_clicked(move |_| {
                show_rule_dialog(
                    &edit_parent,
                    edit_config.clone(),
                    Some(edit_rule.clone()),
                    edit_refresh.clone(),
                );
            });
            row.add_suffix(&edit);
            let remove = gtk::Button::from_icon_name("user-trash-symbolic");
            remove.add_css_class("flat");
            remove.set_tooltip_text(Some("Delete Rule"));
            let remove_config = config.clone();
            let remove_rule = item.clone();
            let remove_refresh = refresh_rules.clone();
            remove.connect_clicked(move |_| {
                let mut current = remove_config.borrow_mut();
                remove_listed_rule(&mut current, &remove_rule);
                if current.save().is_ok() {
                    drop(current);
                    if let Some(refresh) = remove_refresh.borrow().as_ref() {
                        refresh();
                    }
                }
            });
            row.add_suffix(&remove);
        }
        group.add(&row);
        state.borrow_mut().rendered_rows.push(row);
    }
    let state = state.borrow();
    load_more.set_visible(state.loaded < state.filtered.len());
}

fn refresh_rule_list(
    group: &adw::PreferencesGroup,
    state: &Rc<RefCell<RuleListState>>,
    load_more: &gtk::Button,
    rules: Vec<ListedRule>,
    query: &str,
    editable: bool,
    parent: &adw::ApplicationWindow,
    config: &Rc<RefCell<AppConfig>>,
    refresh_rules: &RefreshRules,
) {
    {
        let mut state = state.borrow_mut();
        for row in state.rendered_rows.drain(..) {
            group.remove(&row);
        }
        let query = query.trim().to_lowercase();
        state.filtered = rules.into_iter().filter(|rule| rule.matches(&query)).collect();
        state.loaded = 0;
    }
    append_rule_batch(
        group,
        state,
        load_more,
        editable,
        parent,
        config,
        refresh_rules,
    );
}

fn show_profile_dialog(
    parent: &adw::ApplicationWindow,
    config: Rc<RefCell<AppConfig>>,
    profile: Option<Profile>,
    refresh: RefreshConnections,
) {
    let editing = profile.is_some();
    let source = profile.unwrap_or_default();
    let dialog = adw::AlertDialog::new(Some(if editing { "Edit Connection" } else { "New Connection" }), None);
    let group = adw::PreferencesGroup::new();
    let name = adw::EntryRow::builder().title("Name").text(&source.name).build();
    let host = adw::EntryRow::builder().title("Host").text(&source.host).build();
    let port = adw::EntryRow::builder().title("Port").text(source.port.to_string()).build();
    let username = adw::EntryRow::builder().title("Username").text(&source.username).build();
    let identity = adw::EntryRow::builder()
        .title("Identity File")
        .text(source.identity_file.as_ref().map(|path| path.to_string_lossy()).unwrap_or_default())
        .build();
    group.add(&name);
    group.add(&host);
    group.add(&port);
    group.add(&username);
    group.add(&identity);
    dialog.set_extra_child(Some(&group));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("save", "Save");
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.connect_response(None, move |dialog, response| {
        if response != "save" {
            return;
        }
        let host_text = host.text().trim().to_string();
        if host_text.is_empty() {
            dialog.set_body("Host is required.");
            return;
        }
        let mut saved = source.clone();
        saved.name = if name.text().trim().is_empty() { "Unnamed".into() } else { name.text().trim().to_string() };
        saved.host = host_text;
        saved.port = port.text().parse::<u16>().unwrap_or(22);
        saved.username = username.text().trim().to_string();
        let identity_text = identity.text();
        let identity_text = identity_text.trim();
        saved.identity_file = (!identity_text.is_empty()).then(|| PathBuf::from(identity_text));
        let mut current = config.borrow_mut();
        if let Some(existing) = current.profiles.iter_mut().find(|item| item.id == saved.id) {
            *existing = saved.clone();
        } else {
            current.profiles.push(saved.clone());
        }
        if current.active_profile.is_none() {
            current.active_profile = Some(saved.id);
        }
        if current.save().is_ok() {
            drop(current);
            if let Some(refresh) = refresh.borrow().as_ref() {
                refresh();
            }
        }
    });
    dialog.present(Some(parent));
}

fn scan_desktop_apps() -> Vec<DesktopApp> {
    let mut dirs = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
    ];
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        dirs.push(home.join(".local/share/applications"));
        dirs.push(home.join(".local/share/flatpak/exports/share/applications"));
    }

    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else { continue; };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else { continue; };
            if let Some(app) = parse_desktop_app(&text) {
                if seen.insert(app.executable.clone()) {
                    apps.push(app);
                }
            }
        }
    }
    apps.sort_by_key(|app| app.name.to_lowercase());
    apps
}

fn parse_desktop_app(text: &str) -> Option<DesktopApp> {
    let mut in_entry = false;
    let mut name = String::new();
    let mut exec = String::new();
    let mut icon = String::new();
    let mut no_display = false;
    let mut app_type = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        if let Some(value) = line.strip_prefix("Name=") {
            if name.is_empty() { name = value.trim().to_string(); }
        } else if let Some(value) = line.strip_prefix("Exec=") {
            exec = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("Icon=") {
            icon = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("NoDisplay=") {
            no_display = value.eq_ignore_ascii_case("true");
        } else if let Some(value) = line.strip_prefix("Type=") {
            app_type = value.trim().to_string();
        }
    }
    if no_display || (!app_type.is_empty() && app_type != "Application") || exec.is_empty() {
        return None;
    }
    let executable = extract_exec_name(&exec)?;
    Some(DesktopApp {
        name: if name.is_empty() { executable.clone() } else { name },
        executable,
        icon,
    })
}

fn extract_exec_name(exec: &str) -> Option<String> {
    let mut parts = exec.split_whitespace().filter(|part| !part.starts_with('%'));
    let first = parts.next()?.trim_matches(['\'', '"']);
    let command = if first.ends_with("/env") || first == "env" {
        parts.find(|part| !part.starts_with('-') && !part.contains('='))?
    } else {
        first
    };
    if command.ends_with("flatpak") || command == "flatpak" {
        let args: Vec<_> = exec.split_whitespace().collect();
        if let Some(value) = args.iter().find_map(|arg| arg.strip_prefix("--command=")) {
            return Path::new(value).file_name().map(|value| value.to_string_lossy().to_lowercase());
        }
        if let Some(id) = args.iter().rev().find(|arg| !arg.starts_with('-') && **arg != "run") {
            return id.rsplit('.').find(|part| !matches!(*part, "desktop" | "client" | "app"))
                .map(|value| value.to_lowercase());
        }
    }
    Path::new(command).file_name().map(|value| value.to_string_lossy().to_lowercase())
}

fn current_app_action(config: &AppConfig, executable: &str) -> RuleAction {
    config.settings.app_rules.iter()
        .find(|rule| rule.executable.file_name().is_some_and(|name| name == executable))
        .map(|rule| rule.action)
        .unwrap_or(RuleAction::Direct)
}

fn set_app_action(config: &Rc<RefCell<AppConfig>>, executable: &str, action: RuleAction) {
    let mut current = config.borrow_mut();
    current.settings.app_rules.retain(|rule| {
        rule.executable.file_name().is_none_or(|name| name != executable)
    });
    current.settings.app_rules.push(AppRule {
        executable: PathBuf::from(executable),
        action,
    });
    let _ = current.save();
}

fn format_bytes(bytes: u64) -> String {
    let value = bytes as f64;
    if value < 1024.0 {
        format!("{bytes} B")
    } else if value < 1024.0 * 1024.0 {
        format!("{:.1} KB", value / 1024.0)
    } else if value < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", value / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", value / (1024.0 * 1024.0 * 1024.0))
    }
}

impl RuntimeController {
    fn is_running(&self) -> bool {
        self.is_running.load(Ordering::SeqCst)
    }

    fn stop(&self) {
        self.is_running.store(false, Ordering::SeqCst);
        if let Some(stop) = self.stop.borrow_mut().take() {
            let _ = stop.send(());
        }
    }

    fn shutdown(&self) {
        self.stop();
        let helper = self.helper.clone();
        thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build();
            if let Ok(rt) = runtime {
                rt.block_on(async {
                    let mut guard = helper.lock().await;
                    if let Some(mut h) = guard.take() {
                        h.shutdown().await;
                    }
                });
            }
        });
    }

    fn sync_rules(&self) {
        let helper = self.helper.clone();
        thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build();
            if let Ok(rt) = runtime {
                rt.block_on(async {
                    let mut guard = helper.lock().await;
                    if let Some(h) = guard.as_mut() {
                        let _ = h.sync_rules().await;
                    }
                });
            }
        });
    }

    fn start(
        &self,
        profile: Profile,
        config: AppConfig,
        config_path: PathBuf,
        events: mpsc::Sender<RuntimeEvent>,
    ) {
        self.stop();
        self.is_running.store(true, Ordering::SeqCst);
        let (stop_tx, mut stop_rx) = oneshot::channel();
        *self.stop.borrow_mut() = Some(stop_tx);

        let helper = self.helper.clone();
        let is_running_flag = self.is_running.clone();

        thread::spawn(move || {
            let desktop_apps = scan_desktop_apps();
            let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build();
            let Ok(runtime) = runtime else {
                let _ = events.send(RuntimeEvent::Error("Failed to create runtime".into()));
                is_running_flag.store(false, Ordering::SeqCst);
                return;
            };
            runtime.block_on(async move {
                let user_cancelled = Arc::new(AtomicBool::new(false));
                let mut retry_attempt = 0;
                let mut app_tracker = AppTrafficTracker::default();

                loop {
                    if user_cancelled.load(Ordering::SeqCst) {
                        break;
                    }

                    if retry_attempt > 0 {
                        let _ = events.send(RuntimeEvent::Status(format!("Reconnecting ({retry_attempt})…")));
                        let _ = events.send(RuntimeEvent::Log(format!("[reconnect] Reconnecting SSH tunnel (attempt {retry_attempt})...")));
                    }

                    // 1. 验证 helper 可响应，并在建立 SSH 前清理遗留的特权网络状态。
                    let mut helper_guard = helper.lock().await;
                    let helper_usable = match helper_guard.as_mut() {
                        Some(h) => {
                            if !h.is_alive() {
                                false
                            } else {
                                match h.status().await {
                                    Ok(false) => true,
                                    Ok(true) => match h.stop().await {
                                        Ok(()) => true,
                                        Err(error) => {
                                            let _ = events.send(RuntimeEvent::Log(format!(
                                                "[helper] Failed to reset active helper session: {error}"
                                            )));
                                            false
                                        }
                                    },
                                    Err(error) => {
                                        let _ = events.send(RuntimeEvent::Log(format!(
                                            "[helper] Existing helper is unresponsive: {error}"
                                        )));
                                        false
                                    }
                                }
                            }
                        }
                        None => false,
                    };
                    if !helper_usable {
                        if let Some(mut stale_helper) = helper_guard.take() {
                            stale_helper.terminate().await;
                        }
                        let _ = events.send(RuntimeEvent::Log("[helper] Requesting privileged helper authorization...".into()));
                        let _ = events.send(RuntimeEvent::Status("Authorizing helper…".into()));
                        match PrivilegedHelperSession::ensure_started(&helper_path()).await {
                            Ok((h, stderr)) => {
                                if let Some(stderr) = stderr {
                                    spawn_log_reader(stderr, "helper", events.clone());
                                }
                                *helper_guard = Some(h);
                                let _ = events.send(RuntimeEvent::Log("[helper] Privileged helper authenticated and ready".into()));
                            }
                            Err(err) => {
                                if user_cancelled.load(Ordering::SeqCst) {
                                    break;
                                }
                                let _ = events.send(RuntimeEvent::Error(format!("Helper authorization failed: {err}")));
                                let _ = events.send(RuntimeEvent::Log(format!("[helper] Authorization failed: {err}")));
                                break;
                            }
                        }
                    }

                    // 2. Establish OpenSSH session
                    let mut ssh = match SshSession::start(&profile, SOCKS_PORT, config.settings.dns_server).await {
                        Ok(session) => session,
                        Err(error) => {
                            drop(helper_guard);
                            if user_cancelled.load(Ordering::SeqCst) {
                                break;
                            }
                            retry_attempt += 1;
                            let _ = events.send(RuntimeEvent::Log(format!(
                                "[reconnect] SSH failed: {error}. Retrying in 1s (attempt {retry_attempt})..."
                            )));
                            let _ = events.send(RuntimeEvent::Status("Reconnecting in 1s…".into()));
                            tokio::select! {
                                _ = &mut stop_rx => {
                                    user_cancelled.store(true, Ordering::SeqCst);
                                    break;
                                }
                                _ = tokio::time::sleep(Duration::from_secs(1)) => continue,
                            }
                        }
                    };

                    if let Some(stderr) = ssh.take_stderr() {
                        spawn_log_reader(stderr, "ssh", events.clone());
                    }

                    let uid = unsafe { libc::getuid() };
                    let helper_session = helper_guard.as_mut().unwrap();
                    let start_res = helper_session.start(
                        config_path.clone(),
                        uid,
                        ssh.socks_port,
                        ssh.dns_port,
                        ssh.server_port,
                        ssh.server_addresses.clone(),
                    ).await;

                    if let Err(err) = start_res {
                        let _ = ssh.stop().await;
                        if let Some(mut failed_helper) = helper_guard.take() {
                            failed_helper.terminate().await;
                        }
                        if user_cancelled.load(Ordering::SeqCst) {
                            break;
                        }
                        retry_attempt += 1;
                        let _ = events.send(RuntimeEvent::Log(format!(
                            "[reconnect] Transparent proxy startup failed: {err}. Retrying in 1s (attempt {retry_attempt})..."
                        )));
                        let _ = events.send(RuntimeEvent::Status("Reconnecting in 1s…".into()));
                        drop(helper_guard);
                        tokio::select! {
                            _ = &mut stop_rx => {
                                user_cancelled.store(true, Ordering::SeqCst);
                                break;
                            }
                            _ = tokio::time::sleep(Duration::from_secs(1)) => continue,
                        }
                    }

                    // Connected successfully!
                    if retry_attempt > 0 {
                        let _ = events.send(RuntimeEvent::Log("[reconnect] Connection successfully restored".into()));
                    }
                    retry_attempt = 0;
                    let _ = events.send(RuntimeEvent::Connected);

                    // 3. Monitor active connection
                    let mut traffic_interval = tokio::time::interval(Duration::from_secs(1));
                    let mut previous_traffic = None;
                    let mut disconnect_reason = String::new();

                    loop {
                        tokio::select! {
                            _ = &mut stop_rx => {
                                user_cancelled.store(true, Ordering::SeqCst);
                                break;
                            }
                            status = ssh.wait() => {
                                disconnect_reason = match status {
                                    Ok(status) => format!("SSH connection exited with {status}"),
                                    Err(error) => error.to_string(),
                                };
                                let _ = events.send(RuntimeEvent::Log(format!("[reconnect] {disconnect_reason}")));
                                break;
                            }
                            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                                let helper_health = match helper_guard.as_mut() {
                                    Some(helper) => helper.check_active().await,
                                    None => Err(anyhow::anyhow!("privileged helper session is unavailable")),
                                };
                                if let Err(error) = helper_health {
                                    disconnect_reason = format!("Transparent proxy health check failed: {error}");
                                    let _ = events.send(RuntimeEvent::Log(format!("[helper] {disconnect_reason}")));
                                    break;
                                }
                            }
                            _ = traffic_interval.tick() => {
                                if let Some((sent, received)) = read_ssh_traffic(&profile.host).await {
                                    let (upload, download) = previous_traffic
                                        .map(|(old_sent, old_received)| {
                                            (sent.saturating_sub(old_sent), received.saturating_sub(old_received))
                                        })
                                        .unwrap_or((0, 0));
                                    previous_traffic = Some((sent, received));
                                    let _ = events.send(RuntimeEvent::Speed { upload, download });
                                }
                                if let Some(stats) = app_tracker.sample(&desktop_apps).await {
                                    let _ = events.send(RuntimeEvent::AppTraffic(stats));
                                }
                            }
                        }
                    }

                    // Stop transparent routing and ssh
                    let helper_stop_error = match helper_guard.as_mut() {
                        Some(h) => h.stop().await.err(),
                        None => None,
                    };
                    if let Some(error) = helper_stop_error {
                        let _ = events.send(RuntimeEvent::Log(format!(
                            "[helper] Failed to stop helper session cleanly: {error}"
                        )));
                        if let Some(mut failed_helper) = helper_guard.take() {
                            failed_helper.terminate().await;
                        }
                    }
                    drop(helper_guard);
                    let _ = ssh.stop().await;
                    let _ = events.send(RuntimeEvent::Speed { upload: 0, download: 0 });
                    app_tracker.clear();

                    if user_cancelled.load(Ordering::SeqCst) {
                        let _ = events.send(RuntimeEvent::Disconnected);
                        break;
                    }

                    // Auto-reconnect triggered!
                    retry_attempt += 1;
                    let _ = events.send(RuntimeEvent::Status("Reconnecting in 1s…".into()));
                    let _ = events.send(RuntimeEvent::Log(format!(
                        "[reconnect] Connection lost: {disconnect_reason}. Reconnecting in 1s (attempt {retry_attempt})..."
                    )));

                    tokio::select! {
                        _ = &mut stop_rx => {
                            user_cancelled.store(true, Ordering::SeqCst);
                            let _ = events.send(RuntimeEvent::Disconnected);
                            break;
                        }
                        _ = tokio::time::sleep(Duration::from_secs(1)) => {
                            // Loop back to reconnect
                        }
                    }
                }
                is_running_flag.store(false, Ordering::SeqCst);
            });
        });
    }
}

async fn read_ssh_traffic(host: &str) -> Option<(u64, u64)> {
    if host.trim().is_empty() {
        return None;
    }
    let output = Command::new("ss").args(["-tin", "dst", host]).output().await.ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let sent = sum_ss_counter(&text, "bytes_sent:");
    let received = sum_ss_counter(&text, "bytes_received:");
    (sent > 0 || received > 0).then_some((sent, received))
}

fn sum_ss_counter(text: &str, key: &str) -> u64 {
    text.split_whitespace()
        .filter_map(|field| field.strip_prefix(key))
        .filter_map(|value| value.parse::<u64>().ok())
        .sum()
}

fn format_speed(bytes_per_second: u64) -> String {
    let value = bytes_per_second as f64;
    if value < 1024.0 {
        format!("{value:.0} B/s")
    } else if value < 1024.0 * 1024.0 {
        format!("{:.1} KB/s", value / 1024.0)
    } else if value < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB/s", value / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB/s", value / (1024.0 * 1024.0 * 1024.0))
    }
}

fn create_app_icon(icon_name: &str) -> gtk::Image {
    let icon = if !icon_name.is_empty() {
        if icon_name.starts_with('/') {
            gtk::Image::from_file(icon_name)
        } else {
            gtk::Image::from_icon_name(icon_name)
        }
    } else {
        gtk::Image::from_icon_name("application-x-executable-symbolic")
    };
    icon.set_pixel_size(28);
    icon
}

fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    format!("{hours:02}:{minutes:02}:{secs:02}")
}

fn create_traffic_stat_column(
    title: &str,
    up_label: &gtk::Label,
    down_label: &gtk::Label,
) -> gtk::Box {
    let col = gtk::Box::new(gtk::Orientation::Vertical, 6);
    col.set_margin_start(16);
    col.set_margin_end(16);
    col.set_margin_top(14);
    col.set_margin_bottom(14);

    let title_lbl = gtk::Label::builder()
        .label(title)
        .halign(gtk::Align::Start)
        .css_classes(["dim-label", "heading"])
        .build();
    col.append(&title_lbl);

    let up_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let up_arrow = gtk::Label::builder().label("↑").css_classes(["stat-arrow-up", "heading"]).build();
    up_box.append(&up_arrow);
    up_label.set_halign(gtk::Align::Start);
    up_label.add_css_class("numeric");
    up_label.add_css_class("heading");
    up_box.append(up_label);
    col.append(&up_box);

    let down_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let down_arrow = gtk::Label::builder().label("↓").css_classes(["stat-arrow-down", "heading"]).build();
    down_box.append(&down_arrow);
    down_label.set_halign(gtk::Align::Start);
    down_label.add_css_class("numeric");
    down_label.add_css_class("heading");
    down_box.append(down_label);
    col.append(&down_box);

    col
}

fn create_chart_column(
    title: &str,
    count_label: &gtk::Label,
    fill_box: &gtk::Box,
    fill_class: &str,
) -> gtk::Box {
    let col = gtk::Box::new(gtk::Orientation::Vertical, 6);
    col.set_halign(gtk::Align::Center);
    col.set_margin_top(16);
    col.set_margin_bottom(16);
    col.set_margin_start(12);
    col.set_margin_end(12);

    count_label.add_css_class("title-3");
    count_label.add_css_class("numeric");
    count_label.set_halign(gtk::Align::Center);
    col.append(count_label);

    let track = gtk::Box::new(gtk::Orientation::Vertical, 0);
    track.add_css_class("chart-track");
    track.set_width_request(42);
    track.set_height_request(100);
    track.set_halign(gtk::Align::Center);

    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    track.append(&spacer);

    fill_box.set_valign(gtk::Align::End);
    fill_box.add_css_class(fill_class);
    fill_box.set_height_request(0);
    track.append(fill_box);
    col.append(&track);

    let title_lbl = gtk::Label::builder()
        .label(title)
        .halign(gtk::Align::Center)
        .css_classes(["heading", "dim-label"])
        .build();
    col.append(&title_lbl);

    col
}

fn init_dashboard_styles() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        ".chart-track {
            background-color: alpha(currentColor, 0.12);
            border-radius: 6px;
        }
        .chart-fill-direct {
            background-color: #2ec27e;
            border-radius: 6px;
        }
        .chart-fill-proxy {
            background-color: #2ec27e;
            border-radius: 6px;
        }
        .chart-fill-reject {
            background-color: #2ec27e;
            border-radius: 6px;
        }
        .stat-arrow-up {
            color: #e01b24;
            font-weight: bold;
        }
        .stat-arrow-down {
            color: #2ec27e;
            font-weight: bold;
        }"
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

#[derive(Default)]
struct AppTrafficTracker {
    active_sockets: std::collections::HashMap<String, (u64, u64)>,
    app_traffic: std::collections::HashMap<String, (u64, u64)>,
}

impl AppTrafficTracker {
    fn clear(&mut self) {
        self.active_sockets.clear();
        self.app_traffic.clear();
    }

    async fn sample(&mut self, desktop_apps: &[DesktopApp]) -> Option<Vec<AppTrafficStat>> {
        let output = Command::new("ss").args(["-tinp", "-H"]).output().await.ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let mut seen_sockets = std::collections::HashSet::new();
        let mut current_sock_key = String::new();
        let mut current_proc_name = String::new();

        for line in text.lines() {
            if !line.starts_with([' ', '\t']) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    let local = parts[3];
                    let peer = parts[4];
                    current_sock_key = format!("{local}->{peer}");
                    seen_sockets.insert(current_sock_key.clone());

                    current_proc_name = if let Some(idx) = line.find("users:((\"") {
                        let start = idx + 9;
                        if let Some(end) = line[start..].find('"') {
                            line[start..start + end].to_string()
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    };
                } else {
                    current_sock_key.clear();
                    current_proc_name.clear();
                }
            } else if !current_sock_key.is_empty() && !current_proc_name.is_empty() {
                let cur_sent = sum_ss_counter(line, "bytes_sent:");
                let cur_received = sum_ss_counter(line, "bytes_received:");
                if cur_sent > 0 || cur_received > 0 {
                    let (delta_up, delta_down) = if let Some((prev_sent, prev_rcv)) = self.active_sockets.get(&current_sock_key) {
                        (cur_sent.saturating_sub(*prev_sent), cur_received.saturating_sub(*prev_rcv))
                    } else {
                        (cur_sent, cur_received)
                    };
                    self.active_sockets.insert(current_sock_key.clone(), (cur_sent, cur_received));
                    if delta_up > 0 || delta_down > 0 {
                        let entry = self.app_traffic.entry(current_proc_name.clone()).or_insert((0, 0));
                        entry.0 += delta_up;
                        entry.1 += delta_down;
                    }
                }
            }
        }

        self.active_sockets.retain(|k, _| seen_sockets.contains(k));

        let mut stats: Vec<AppTrafficStat> = self
            .app_traffic
            .iter()
            .map(|(proc, (up, down))| {
                let matching_app = desktop_apps.iter().find(|app| {
                    app.executable.eq_ignore_ascii_case(proc)
                        || app.name.eq_ignore_ascii_case(proc)
                        || Path::new(&app.executable)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.eq_ignore_ascii_case(proc))
                });
                let (name, icon) = if let Some(app) = matching_app {
                    (app.name.clone(), app.icon.clone())
                } else {
                    (proc.clone(), String::new())
                };
                AppTrafficStat {
                    id: proc.clone(),
                    name,
                    icon,
                    upload: *up,
                    download: *down,
                }
            })
            .collect();

        stats.sort_by(|a, b| (b.upload + b.download).cmp(&(a.upload + a.download)));
        Some(stats)
    }
}

fn navigation_row(icon_name: &str, title: &str) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    row.set_height_request(48);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(20);
    content.append(&icon);
    let label = gtk::Label::new(Some(title));
    label.set_halign(gtk::Align::Start);
    label.set_hexpand(true);
    content.append(&label);
    row.set_child(Some(&content));
    row
}

fn page_scroller(page: &adw::PreferencesPage) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder().child(page).vexpand(true).build()
}

fn spawn_log_reader<R>(reader: R, source: &'static str, events: mpsc::Sender<RuntimeEvent>)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = events.send(RuntimeEvent::Log(format!("[{source}] {line}")));
        }
    });
}

fn import_rule_source(url: &str) -> Result<RuleImportResult, String> {
    if !url.starts_with("https://") {
        return Err("Only HTTPS rule URLs are supported".into());
    }
    let content = download_rule_text(url)?;
    let mut result = parse_shadowrocket_rules(&content);
    let rule_sets = result.rule_sets.clone();
    for reference in rule_sets.into_iter().take(8) {
        match download_rule_text(&reference.url) {
            Ok(content) => result.merge(parse_rule_set(&content, reference.action)),
            Err(error) => {
                result.ignored_count += 1;
                result.warnings.push(format!("Rule set skipped: {error}"));
            }
        }
    }
    if result.rule_sets.len() > 8 {
        result.ignored_count += result.rule_sets.len() - 8;
        result.warnings.push("Additional rule sets were skipped".into());
    }
    if result.rule_count() == 0 {
        return Err("The source contains no supported rules".into());
    }
    Ok(result)
}

fn download_rule_text(url: &str) -> Result<String, String> {
    if !url.starts_with("https://") {
        return Err("Only HTTPS rule URLs are supported".into());
    }
    let output = StdCommand::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            "120",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-filesize",
            &MAX_RULE_SOURCE_SIZE.to_string(),
            url,
        ])
        .output()
        .map_err(|error| format!("Failed to start curl: {error}"))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if error.is_empty() { format!("curl exited with {}", output.status) } else { error });
    }
    if output.stdout.len() > MAX_RULE_SOURCE_SIZE {
        return Err("Rule source exceeds 16 MB".into());
    }
    String::from_utf8(output.stdout).map_err(|_| "Rule source is not UTF-8".into())
}

fn helper_path() -> PathBuf {
    if let Some(path) = std::env::var_os("SSH_ROCKET_HELPER") {
        return PathBuf::from(path);
    }
    for path in ["/usr/local/libexec/ssh-rocket-helper", "/usr/libexec/ssh-rocket-helper"] {
        let path = PathBuf::from(path);
        if path.exists() {
            return path;
        }
    }
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("ssh-rocket-helper")))
        .unwrap_or_else(|| PathBuf::from("ssh-rocket-helper"))
}

fn main() {
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run();
}

fn build_ui(app: &adw::Application) {
    init_dashboard_styles();
    let config = Rc::new(RefCell::new(AppConfig::load().unwrap_or_default()));
    let controller = Rc::new(RefCell::new(RuntimeController::default()));
    let (event_tx, event_rx) = mpsc::channel::<RuntimeEvent>();
    let is_connected = Rc::new(RefCell::new(false));
    let connect_start_time = Rc::new(RefCell::new(None::<std::time::Instant>));
    let connection_buttons = Rc::new(RefCell::new(Vec::<(String, gtk::Button)>::new()));
    let refresh_connections: RefreshConnections = Rc::new(RefCell::new(None));
    let tray_manager = Rc::new(RefCell::new(None::<Rc<TrayManager>>));
    let quitting = Rc::new(RefCell::new(false));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("SSH Rocket")
        .default_width(880)
        .default_height(600)
        .build();

    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sidebar.set_width_request(200);
    sidebar.add_css_class("sidebar");
    let app_title = gtk::Label::new(Some("SSH Rocket"));
    app_title.add_css_class("title-2");
    app_title.set_halign(gtk::Align::Start);
    app_title.set_margin_start(18);
    app_title.set_margin_end(18);
    app_title.set_margin_top(18);
    app_title.set_margin_bottom(12);
    sidebar.append(&app_title);

    let navigation = gtk::ListBox::new();
    navigation.add_css_class("navigation-sidebar");
    navigation.set_selection_mode(gtk::SelectionMode::Single);
    navigation.set_activate_on_single_click(true);
    navigation.set_vexpand(true);
    let connect_nav = navigation_row("ssh-rocket-connect-symbolic", "Connect");
    let rules_nav = navigation_row("ssh-rocket-rules-symbolic", "Rules");
    let traffic_nav = navigation_row("ssh-rocket-traffic-symbolic", "Traffic");
    let logs_nav = navigation_row("ssh-rocket-logs-symbolic", "Logs");
    navigation.append(&connect_nav);
    navigation.append(&rules_nav);
    navigation.append(&traffic_nav);
    navigation.append(&logs_nav);
    sidebar.append(&navigation);
    root.append(&sidebar);
    root.append(&gtk::Separator::new(gtk::Orientation::Vertical));

    let header = adw::HeaderBar::new();
    let page_title = gtk::Label::new(Some("Connect"));
    page_title.add_css_class("title-3");
    header.set_title_widget(Some(&page_title));
    let add_connection = gtk::Button::from_icon_name("list-add-symbolic");
    add_connection.add_css_class("flat");
    add_connection.set_tooltip_text(Some("Add Connection"));
    header.pack_end(&add_connection);
    let toolbar = adw::ToolbarView::new();
    toolbar.set_hexpand(true);
    toolbar.add_top_bar(&header);

    let view_stack = gtk::Stack::new();
    view_stack.set_hexpand(true);
    view_stack.set_vexpand(true);

    let connect_stack = gtk::Stack::new();
    connect_stack.set_vexpand(true);
    let empty_connections = adw::StatusPage::builder()
        .icon_name("network-server-symbolic")
        .title("No Connections")
        .description("Add a server to get started.")
        .build();
    let empty_add = gtk::Button::with_label("Add Connection");
    empty_add.add_css_class("suggested-action");
    empty_add.add_css_class("pill");
    empty_add.set_halign(gtk::Align::Center);
    empty_connections.set_child(Some(&empty_add));
    connect_stack.add_named(&empty_connections, Some("empty"));

    let connection_flow = gtk::FlowBox::new();
    connection_flow.set_selection_mode(gtk::SelectionMode::None);
    connection_flow.set_column_spacing(16);
    connection_flow.set_row_spacing(16);
    connection_flow.set_min_children_per_line(1);
    connection_flow.set_max_children_per_line(3);
    connection_flow.set_homogeneous(false);
    connection_flow.set_valign(gtk::Align::Start);
    connection_flow.set_margin_start(18);
    connection_flow.set_margin_end(18);
    connection_flow.set_margin_top(18);
    connection_flow.set_margin_bottom(18);
    let connection_scroller = gtk::ScrolledWindow::builder().child(&connection_flow).vexpand(true).build();
    connect_stack.add_named(&connection_scroller, Some("cards"));
    view_stack.add_named(&connect_stack, Some("connect"));

    let rules_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let rules_stack = gtk::Stack::new();
    rules_stack.set_vexpand(true);
    let rules_switcher = gtk::StackSwitcher::new();
    rules_switcher.set_stack(Some(&rules_stack));
    rules_switcher.set_halign(gtk::Align::Center);
    rules_switcher.set_margin_top(12);
    rules_switcher.set_margin_bottom(12);
    rules_page.append(&rules_switcher);
    rules_page.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    rules_page.append(&rules_stack);

    let refresh_rule_views: RefreshRules = Rc::new(RefCell::new(None));
    let refresh_blocked_views: RefreshRules = Rc::new(RefCell::new(None));
    let refresh_traffic_rule_counts: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));

    let applications_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let app_toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    app_toolbar.set_margin_start(18);
    app_toolbar.set_margin_end(18);
    app_toolbar.set_margin_top(12);
    app_toolbar.set_margin_bottom(12);
    let app_search = gtk::SearchEntry::builder().placeholder_text("Search").hexpand(true).build();
    app_toolbar.append(&app_search);
    let sort_label = gtk::Label::new(Some("Sort"));
    sort_label.add_css_class("dim-label");
    app_toolbar.append(&sort_label);
    let app_sort = gtk::DropDown::from_strings(&["Name", "Rule"]);
    app_toolbar.append(&app_sort);
    applications_page.append(&app_toolbar);
    applications_page.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let applications_preferences = adw::PreferencesPage::new();
    let applications_group = adw::PreferencesGroup::builder().title("Applications").build();
    let app_rows = Rc::new(RefCell::new(Vec::<(String, String, adw::ComboRow)>::new()));
    for app_info in scan_desktop_apps() {
        let action = current_app_action(&config.borrow(), &app_info.executable);
        let row = adw::ComboRow::builder()
            .title(&app_info.name)
            .subtitle(&app_info.executable)
            .model(&gtk::StringList::new(&["Direct", "Proxy", "Block"]))
            .selected(match action {
                RuleAction::Direct => 0,
                RuleAction::Proxy => 1,
                RuleAction::Block => 2,
            })
            .build();
        row.set_use_markup(false);
        row.add_prefix(&create_app_icon(&app_info.icon));
        let executable = app_info.executable.clone();
        let config_ref = config.clone();
        let controller_ref = controller.clone();
        let refresh_blocked_ref = refresh_blocked_views.clone();
        let refresh_traffic_counts_ref = refresh_traffic_rule_counts.clone();
        row.connect_selected_notify(move |row| {
            let action = match row.selected() {
                1 => RuleAction::Proxy,
                2 => RuleAction::Block,
                _ => RuleAction::Direct,
            };
            set_app_action(&config_ref, &executable, action);
            controller_ref.borrow().sync_rules();
            if let Some(refresh) = refresh_blocked_ref.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_traffic_counts_ref.borrow().as_ref() {
                refresh();
            }
        });
        applications_group.add(&row);
        app_rows.borrow_mut().push((
            format!("{} {}", app_info.name, app_info.executable).to_lowercase(),
            app_info.name.to_lowercase(),
            row,
        ));
    }
    {
        let app_rows = app_rows.clone();
        app_search.connect_search_changed(move |entry| {
            let query = entry.text().to_lowercase();
            for (search_text, _, row) in app_rows.borrow().iter() {
                row.set_visible(query.is_empty() || search_text.contains(&query));
            }
        });
    }
    {
        let app_rows = app_rows.clone();
        let applications_group = applications_group.clone();
        app_sort.connect_selected_notify(move |sort| {
            let mut rows = app_rows.borrow_mut();
            rows.sort_by(|left, right| {
                if sort.selected() == 1 {
                    left.2.selected().cmp(&right.2.selected()).then_with(|| left.1.cmp(&right.1))
                } else {
                    left.1.cmp(&right.1)
                }
            });
            for (_, _, row) in rows.iter() {
                applications_group.remove(row);
            }
            for (_, _, row) in rows.iter() {
                applications_group.add(row);
            }
        });
    }
    applications_preferences.add(&applications_group);
    applications_page.append(&page_scroller(&applications_preferences));
    rules_stack.add_titled(&applications_page, Some("applications"), "Applications");

    let routing_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let domain_stack = gtk::Stack::new();
    domain_stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
    domain_stack.set_vexpand(true);
    routing_page.append(&domain_stack);

    let overview_page = adw::PreferencesPage::new();
    let routing_group = adw::PreferencesGroup::builder().title("Default Policy").build();
    let policy = adw::ComboRow::builder()
        .title("Unmatched Traffic")
        .model(&gtk::StringList::new(&["Proxy", "Direct", "Block"]))
        .selected(match config.borrow().settings.default_policy {
            RuleAction::Proxy => 0,
            RuleAction::Direct => 1,
            RuleAction::Block => 2,
        })
        .build();
    let ipv6 = adw::SwitchRow::builder().title("IPv6").active(config.borrow().settings.ipv6).build();
    routing_group.add(&policy);
    routing_group.add(&ipv6);
    overview_page.add(&routing_group);

    let initial_rule_source = {
        let current = config.borrow();
        if current.settings.rule_source_url.is_empty() {
            DEFAULT_RULE_SOURCE.to_string()
        } else {
            current.settings.rule_source_url.clone()
        }
    };
    let rule_source = adw::EntryRow::builder()
        .title("Shadowrocket Rule Source")
        .text(&initial_rule_source)
        .build();
    let import_rules = gtk::Button::new();
    import_rules.set_visible(false);
    let configurations_group = adw::PreferencesGroup::builder().title("Configurations").build();
    let import_button = gtk::Button::with_label("Import…");
    import_button.set_valign(gtk::Align::Center);
    import_button.add_css_class("suggested-action");
    configurations_group.set_header_suffix(Some(&import_button));
    let rule_status = adw::ActionRow::new();
    rule_status.set_activatable(true);
    rule_status.add_prefix(&gtk::Image::from_icon_name("object-select-symbolic"));
    rule_status.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    configurations_group.add(&rule_status);
    overview_page.add(&configurations_group);

    let custom_summary_group = adw::PreferencesGroup::builder().title("Custom Rules").build();
    let custom_summary = adw::ActionRow::builder()
        .title("Custom Overrides")
        .activatable(true)
        .build();
    custom_summary.add_prefix(&gtk::Image::from_icon_name("document-edit-symbolic"));
    custom_summary.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    custom_summary_group.add(&custom_summary);
    overview_page.add(&custom_summary_group);
    domain_stack.add_named(&page_scroller(&overview_page), Some("overview"));

    let detail_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let detail_header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    detail_header.set_margin_start(12);
    detail_header.set_margin_end(12);
    detail_header.set_margin_top(8);
    detail_header.set_margin_bottom(8);
    let detail_back = gtk::Button::from_icon_name("go-previous-symbolic");
    detail_back.add_css_class("flat");
    detail_back.set_tooltip_text(Some("Back"));
    detail_header.append(&detail_back);
    let detail_title = gtk::Label::new(Some("Configuration"));
    detail_title.add_css_class("title-4");
    detail_title.set_halign(gtk::Align::Start);
    detail_header.append(&detail_title);
    detail_page.append(&detail_header);
    detail_page.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let detail_preferences = adw::PreferencesPage::new();
    let source_group = adw::PreferencesGroup::builder().title("Source").build();
    let source_detail = adw::ActionRow::new();
    source_detail.add_prefix(&gtk::Image::from_icon_name("folder-download-symbolic"));
    let update_source = gtk::Button::from_icon_name("view-refresh-symbolic");
    update_source.add_css_class("flat");
    update_source.set_tooltip_text(Some("Update Configuration"));
    source_detail.add_suffix(&update_source);
    let remove_source = gtk::Button::from_icon_name("user-trash-symbolic");
    remove_source.add_css_class("flat");
    remove_source.set_tooltip_text(Some("Remove Configuration"));
    source_detail.add_suffix(&remove_source);
    source_group.add(&source_detail);
    detail_preferences.add(&source_group);
    let contents_group = adw::PreferencesGroup::builder().title("Contents").build();
    let imported_summary = adw::ActionRow::builder().title("Rules").activatable(true).build();
    imported_summary.add_prefix(&gtk::Image::from_icon_name("view-list-symbolic"));
    imported_summary.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    contents_group.add(&imported_summary);
    detail_preferences.add(&contents_group);
    detail_page.append(&page_scroller(&detail_preferences));
    domain_stack.add_named(&detail_page, Some("detail"));

    let imported_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let imported_header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    imported_header.set_margin_start(12);
    imported_header.set_margin_end(12);
    imported_header.set_margin_top(8);
    imported_header.set_margin_bottom(8);
    let imported_back = gtk::Button::from_icon_name("go-previous-symbolic");
    imported_back.add_css_class("flat");
    imported_back.set_tooltip_text(Some("Back"));
    imported_header.append(&imported_back);
    let imported_title = gtk::Label::new(Some("Rules"));
    imported_title.add_css_class("title-4");
    imported_header.append(&imported_title);
    imported_page.append(&imported_header);
    imported_page.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let imported_search = gtk::SearchEntry::builder().placeholder_text("Search Rules").build();
    imported_search.set_margin_start(18);
    imported_search.set_margin_end(18);
    imported_search.set_margin_top(12);
    imported_search.set_margin_bottom(12);
    imported_page.append(&imported_search);
    let imported_body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    imported_body.set_margin_start(18);
    imported_body.set_margin_end(18);
    imported_body.set_margin_bottom(18);
    let imported_rules_group = adw::PreferencesGroup::builder().title("Imported Rules").build();
    imported_body.append(&imported_rules_group);
    let imported_load_more = gtk::Button::with_label("Load More");
    imported_load_more.set_halign(gtk::Align::Center);
    imported_body.append(&imported_load_more);
    let imported_scroller = gtk::ScrolledWindow::builder()
        .child(&imported_body)
        .vexpand(true)
        .build();
    imported_page.append(&imported_scroller);
    domain_stack.add_named(&imported_page, Some("imported"));

    let custom_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let custom_header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    custom_header.set_margin_start(12);
    custom_header.set_margin_end(12);
    custom_header.set_margin_top(8);
    custom_header.set_margin_bottom(8);
    let custom_back = gtk::Button::from_icon_name("go-previous-symbolic");
    custom_back.add_css_class("flat");
    custom_back.set_tooltip_text(Some("Back"));
    custom_header.append(&custom_back);
    let custom_title = gtk::Label::new(Some("Custom Rules"));
    custom_title.add_css_class("title-4");
    custom_header.append(&custom_title);
    custom_page.append(&custom_header);
    custom_page.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    let custom_search = gtk::SearchEntry::builder().placeholder_text("Search Rules").build();
    custom_search.set_margin_start(18);
    custom_search.set_margin_end(18);
    custom_search.set_margin_top(12);
    custom_search.set_margin_bottom(12);
    custom_page.append(&custom_search);
    let custom_body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    custom_body.set_margin_start(18);
    custom_body.set_margin_end(18);
    custom_body.set_margin_bottom(18);
    let custom_rules_group = adw::PreferencesGroup::builder().title("Custom Rules").build();
    let custom_actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let import_rules = gtk::Button::with_label("Import");
    custom_actions.append(&import_rules);
    let clear_rules = gtk::Button::with_label("Clear");
    clear_rules.add_css_class("destructive-action");
    custom_actions.append(&clear_rules);
    let add_rule = gtk::Button::with_label("Add Rule");
    add_rule.add_css_class("suggested-action");
    custom_actions.append(&add_rule);
    custom_rules_group.set_header_suffix(Some(&custom_actions));
    custom_body.append(&custom_rules_group);
    let custom_load_more = gtk::Button::with_label("Load More");
    custom_load_more.set_halign(gtk::Align::Center);
    custom_body.append(&custom_load_more);
    let custom_scroller = gtk::ScrolledWindow::builder().child(&custom_body).vexpand(true).build();
    custom_page.append(&custom_scroller);
    domain_stack.add_named(&custom_page, Some("custom"));

    domain_stack.set_visible_child_name("overview");
    {
        let parent = window.clone();
        let config = config.clone();
        let refresh_rule_views = refresh_rule_views.clone();
        add_rule.connect_clicked(move |_| {
            show_rule_dialog(
                &parent,
                config.clone(),
                None,
                refresh_rule_views.clone(),
            );
        });
    }

    let imported_state = Rc::new(RefCell::new(RuleListState::default()));
    let custom_state = Rc::new(RefCell::new(RuleListState::default()));
    {
        let group = imported_rules_group.clone();
        let state = imported_state.clone();
        let load_more = imported_load_more.clone();
        let parent = window.clone();
        let config = config.clone();
        let refresh = refresh_rule_views.clone();
        imported_load_more.connect_clicked(move |_| {
            append_rule_batch(&group, &state, &load_more, false, &parent, &config, &refresh);
        });
    }
    {
        let group = custom_rules_group.clone();
        let state = custom_state.clone();
        let load_more = custom_load_more.clone();
        let parent = window.clone();
        let config = config.clone();
        let refresh = refresh_rule_views.clone();
        custom_load_more.connect_clicked(move |_| {
            append_rule_batch(&group, &state, &load_more, true, &parent, &config, &refresh);
        });
    }
    {
        let group = imported_rules_group.clone();
        let state = imported_state.clone();
        let load_more = imported_load_more.clone();
        let parent = window.clone();
        let config = config.clone();
        let refresh = refresh_rule_views.clone();
        imported_scroller.vadjustment().connect_value_changed(move |adjustment| {
            if adjustment.value() + adjustment.page_size() >= adjustment.upper() - 160.0 {
                append_rule_batch(&group, &state, &load_more, false, &parent, &config, &refresh);
            }
        });
    }
    {
        let group = custom_rules_group.clone();
        let state = custom_state.clone();
        let load_more = custom_load_more.clone();
        let parent = window.clone();
        let config = config.clone();
        let refresh = refresh_rule_views.clone();
        custom_scroller.vadjustment().connect_value_changed(move |adjustment| {
            if adjustment.value() + adjustment.page_size() >= adjustment.upper() - 160.0 {
                append_rule_batch(&group, &state, &load_more, true, &parent, &config, &refresh);
            }
        });
    }

    let refresh_rule_views_impl: Rc<dyn Fn()> = {
        let config = config.clone();
        let rule_status = rule_status.clone();
        let custom_summary = custom_summary.clone();
        let detail_title = detail_title.clone();
        let source_detail = source_detail.clone();
        let imported_summary = imported_summary.clone();
        let imported_search = imported_search.clone();
        let imported_rules_group = imported_rules_group.clone();
        let imported_state = imported_state.clone();
        let imported_load_more = imported_load_more.clone();
        let custom_search = custom_search.clone();
        let custom_rules_group = custom_rules_group.clone();
        let custom_state = custom_state.clone();
        let custom_load_more = custom_load_more.clone();
        let clear_rules = clear_rules.clone();
        let parent = window.clone();
        let refresh_rule_views = refresh_rule_views.clone();
        let refresh_blocked_views = refresh_blocked_views.clone();
        let refresh_traffic_rule_counts = refresh_traffic_rule_counts.clone();
        Rc::new(move || {
            let current = config.borrow();
            let imported = imported_rules(&current);
            let custom = custom_rules(&current);
            let source_name = if current.settings.rule_source_name.is_empty() {
                "Imported Configuration"
            } else {
                &current.settings.rule_source_name
            };
            if imported.is_empty() {
                rule_status.set_title("No Configurations");
                rule_status.set_subtitle("");
                rule_status.set_activatable(false);
            } else {
                rule_status.set_title(source_name);
                rule_status.set_subtitle(&format!("{} rules", imported.len()));
                rule_status.set_activatable(true);
            }
            custom_summary.set_subtitle(&format!("{} rules", custom.len()));
            detail_title.set_text(source_name);
            source_detail.set_title(source_name);
            source_detail.set_subtitle(&current.settings.rule_source_url);
            let (direct, proxy, reject) = imported.iter().fold((0, 0, 0), |counts, rule| {
                match rule.action() {
                    RuleAction::Direct => (counts.0 + 1, counts.1, counts.2),
                    RuleAction::Proxy => (counts.0, counts.1 + 1, counts.2),
                    RuleAction::Block => (counts.0, counts.1, counts.2 + 1),
                }
            });
            imported_summary.set_subtitle(&format!(
                "{} rules · {direct} direct · {proxy} proxy · {reject} reject",
                imported.len()
            ));
            clear_rules.set_visible(!custom.is_empty());
            let imported_query = imported_search.text().to_string();
            let custom_query = custom_search.text().to_string();
            drop(current);
            refresh_rule_list(
                &imported_rules_group,
                &imported_state,
                &imported_load_more,
                imported,
                &imported_query,
                false,
                &parent,
                &config,
                &refresh_rule_views,
            );
            refresh_rule_list(
                &custom_rules_group,
                &custom_state,
                &custom_load_more,
                custom,
                &custom_query,
                true,
                &parent,
                &config,
                &refresh_rule_views,
            );
            if let Some(refresh) = refresh_blocked_views.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_traffic_rule_counts.borrow().as_ref() {
                refresh();
            }
        })
    };
    *refresh_rule_views.borrow_mut() = Some(refresh_rule_views_impl.clone());
    refresh_rule_views_impl();

    {
        let refresh = refresh_rule_views_impl.clone();
        imported_search.connect_search_changed(move |_| refresh());
    }
    {
        let refresh = refresh_rule_views_impl.clone();
        custom_search.connect_search_changed(move |_| refresh());
    }
    {
        let stack = domain_stack.clone();
        rule_status.connect_activated(move |_| stack.set_visible_child_name("detail"));
    }
    {
        let stack = domain_stack.clone();
        custom_summary.connect_activated(move |_| stack.set_visible_child_name("custom"));
    }
    {
        let stack = domain_stack.clone();
        detail_back.connect_clicked(move |_| stack.set_visible_child_name("overview"));
    }
    {
        let stack = domain_stack.clone();
        imported_summary.connect_activated(move |_| stack.set_visible_child_name("imported"));
    }
    {
        let stack = domain_stack.clone();
        imported_back.connect_clicked(move |_| stack.set_visible_child_name("detail"));
    }
    {
        let stack = domain_stack.clone();
        custom_back.connect_clicked(move |_| stack.set_visible_child_name("overview"));
    }
    {
        let parent = window.clone();
        let trigger = import_rules.clone();
        let rule_source = rule_source.clone();
        import_button.connect_clicked(move |_| {
            let dialog = adw::AlertDialog::new(Some("Import Configuration"), None);
            let group = adw::PreferencesGroup::new();
            let url = adw::EntryRow::builder()
                .title("HTTPS URL")
                .text(rule_source.text())
                .build();
            group.add(&url);
            dialog.set_extra_child(Some(&group));
            dialog.add_response("cancel", "Cancel");
            dialog.add_response("import", "Import");
            dialog.set_response_appearance("import", adw::ResponseAppearance::Suggested);
            let trigger = trigger.clone();
            let rule_source = rule_source.clone();
            dialog.connect_response(None, move |_, response| {
                if response == "import" {
                    rule_source.set_text(url.text().trim());
                    trigger.emit_clicked();
                }
            });
            dialog.present(Some(&parent));
        });
    }
    {
        let trigger = import_rules.clone();
        let rule_source = rule_source.clone();
        let config = config.clone();
        update_source.connect_clicked(move |_| {
            rule_source.set_text(&config.borrow().settings.rule_source_url);
            trigger.emit_clicked();
        });
    }
    {
        let parent = window.clone();
        let config = config.clone();
        let stack = domain_stack.clone();
        let refresh = refresh_rule_views.clone();
        remove_source.connect_clicked(move |_| {
            let dialog = adw::AlertDialog::new(Some("Remove Configuration?"), None);
            dialog.add_response("cancel", "Cancel");
            dialog.add_response("remove", "Remove");
            dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
            let config = config.clone();
            let stack = stack.clone();
            let refresh = refresh.clone();
            dialog.connect_response(None, move |_, response| {
                if response == "remove" {
                    let mut current = config.borrow_mut();
                    current.settings.imported_domain_rules.clear();
                    current.settings.imported_ip_rules.clear();
                    current.settings.rule_source_url.clear();
                    current.settings.rule_source_name.clear();
                    current.settings.rule_source_updated_at = 0;
                    if current.save().is_ok() {
                        drop(current);
                        stack.set_visible_child_name("overview");
                        if let Some(refresh) = refresh.borrow().as_ref() {
                            refresh();
                        }
                    }
                }
            });
            dialog.present(Some(&parent));
        });
    }
    {
        let parent = window.clone();
        let config = config.clone();
        let refresh = refresh_rule_views.clone();
        clear_rules.connect_clicked(move |_| {
            let dialog = adw::AlertDialog::new(Some("Clear Custom Rules?"), None);
            dialog.add_response("cancel", "Cancel");
            dialog.add_response("clear", "Clear");
            dialog.set_response_appearance("clear", adw::ResponseAppearance::Destructive);
            let config = config.clone();
            let refresh = refresh.clone();
            dialog.connect_response(None, move |_, response| {
                if response == "clear" {
                    let mut current = config.borrow_mut();
                    current.settings.domain_rules.clear();
                    current.settings.ip_rules.clear();
                    if current.save().is_ok() {
                        drop(current);
                        if let Some(refresh) = refresh.borrow().as_ref() {
                            refresh();
                        }
                    }
                }
            });
            dialog.present(Some(&parent));
        });
    }
    {
        let parent = window.clone();
        let config = config.clone();
        let refresh = refresh_rule_views.clone();
        import_rules.connect_clicked(move |_| {
            let file_dialog = gtk::FileDialog::builder()
                .title("Import Omega Configuration")
                .accept_label("Open")
                .build();

            let filter = gtk::FileFilter::new();
            filter.add_pattern("*.bak");
            filter.add_pattern("*.json");
            filter.set_name(Some("Omega Backup (*.bak, *.json)"));

            let all_filter = gtk::FileFilter::new();
            all_filter.add_pattern("*");
            all_filter.set_name(Some("All Files"));

            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            filters.append(&all_filter);
            file_dialog.set_filters(Some(&filters));

            let dialog_parent = parent.clone();
            let config = config.clone();
            let refresh = refresh.clone();
            file_dialog.open(Some(&parent), gio::Cancellable::NONE, move |result| {
                let parent = dialog_parent;
                let Ok(file) = result else {
                    return;
                };
                let Some(path) = file.path() else {
                    return;
                };
                let content = match fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        let dialog = adw::AlertDialog::new(
                            Some("Import Failed"),
                            Some(&format!("Failed to read file: {e}")),
                        );
                        dialog.add_response("ok", "OK");
                        dialog.present(Some(&parent));
                        return;
                    }
                };

                let parsed = match parse_omega_rules(&content) {
                    Ok(p) => p,
                    Err(e) => {
                        let dialog = adw::AlertDialog::new(
                            Some("Import Failed"),
                            Some(&format!("Failed to parse configuration: {e}")),
                        );
                        dialog.add_response("ok", "OK");
                        dialog.present(Some(&parent));
                        return;
                    }
                };

                let rule_count = parsed.rule_count();
                let domain_count = parsed.domain_rules.len();
                let ip_count = parsed.ip_rules.len();
                let file_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("backup file");

                let dialog = adw::AlertDialog::new(
                    Some("Import Omega Rules"),
                    Some(&format!(
                        "Found {rule_count} rules ({domain_count} domain, {ip_count} IP) in \"{file_name}\".\n\nChoose how to import:",
                    )),
                );
                dialog.add_response("cancel", "Cancel");
                dialog.add_response("replace", "Replace All");
                dialog.set_response_appearance("replace", adw::ResponseAppearance::Destructive);
                dialog.add_response("merge", "Merge");
                dialog.set_response_appearance("merge", adw::ResponseAppearance::Suggested);

                let config = config.clone();
                let refresh = refresh.clone();
                let err_parent = parent.clone();
                dialog.connect_response(None, move |_, response| {
                    if response == "cancel" {
                        return;
                    }
                    let mut current = config.borrow_mut();
                    if response == "replace" {
                        current.settings.domain_rules = parsed.domain_rules.clone();
                        current.settings.ip_rules = parsed.ip_rules.clone();
                    } else if response == "merge" {
                        for new_domain in &parsed.domain_rules {
                            if let Some(existing) = current
                                .settings
                                .domain_rules
                                .iter_mut()
                                .find(|item| item.pattern == new_domain.pattern && item.kind == new_domain.kind)
                            {
                                existing.action = new_domain.action;
                            } else {
                                current.settings.domain_rules.push(new_domain.clone());
                            }
                        }
                        for new_ip in &parsed.ip_rules {
                            if let Some(existing) = current
                                .settings
                                .ip_rules
                                .iter_mut()
                                .find(|item| item.network == new_ip.network)
                            {
                                existing.action = new_ip.action;
                            } else {
                                current.settings.ip_rules.push(new_ip.clone());
                            }
                        }
                    }

                    if let Err(e) = current.save() {
                        let err_dialog = adw::AlertDialog::new(
                            Some("Save Failed"),
                            Some(&format!("Failed to save rules: {e}")),
                        );
                        err_dialog.add_response("ok", "OK");
                        err_dialog.present(Some(&err_parent));
                        return;
                    }
                    drop(current);
                    if let Some(refresh) = refresh.borrow().as_ref() {
                        refresh();
                    }
                });

                dialog.present(Some(&parent));
            });
        });
    }
    rules_stack.add_titled(&routing_page, Some("routing"), "Domains & IPs");

    // --- Blocked Page ---
    let blocked_page = adw::PreferencesPage::new();

    // 1. Processes group
    let procs_group = adw::PreferencesGroup::builder()
        .title("Processes")
        .description("Block network access by executable name")
        .build();
    let new_proc_row = adw::EntryRow::builder().title("Process Name").build();
    let add_proc_btn = gtk::Button::from_icon_name("list-add-symbolic");
    add_proc_btn.add_css_class("flat");
    add_proc_btn.set_valign(gtk::Align::Center);
    new_proc_row.add_suffix(&add_proc_btn);
    procs_group.add(&new_proc_row);

    let procs_list_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    procs_group.add(&procs_list_box);
    blocked_page.add(&procs_group);

    // 2. Custom Blocked Targets (Domains & IPs)
    let blocked_targets_group = adw::PreferencesGroup::builder()
        .title("Blocked Domains & IPs")
        .description("Target rules set to REJECT")
        .build();
    let new_target_row = adw::EntryRow::builder().title("Domain or IP").build();
    let add_target_btn = gtk::Button::from_icon_name("list-add-symbolic");
    add_target_btn.add_css_class("flat");
    add_target_btn.set_valign(gtk::Align::Center);
    new_target_row.add_suffix(&add_target_btn);
    blocked_targets_group.add(&new_target_row);

    let blocked_targets_list_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    blocked_targets_group.add(&blocked_targets_list_box);
    blocked_page.add(&blocked_targets_group);

    // 3. Applications group
    let blocked_apps_group = adw::PreferencesGroup::builder()
        .title("Applications")
        .description("Block all network access")
        .build();
    let app_search_row = adw::EntryRow::builder().title("Search Applications").build();
    blocked_apps_group.add(&app_search_row);

    let blocked_apps_list_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    blocked_apps_group.add(&blocked_apps_list_box);
    blocked_page.add(&blocked_apps_group);

    rules_stack.add_titled(&page_scroller(&blocked_page), Some("blocked"), "Blocked");
    view_stack.add_named(&rules_page, Some("rules"));

    // --- Traffic Page ---
    let traffic_page = adw::PreferencesPage::new();

    // 1. 服务器节点
    let session_group = adw::PreferencesGroup::builder().title("服务器节点").build();
    let started_row = adw::ActionRow::builder().title("开始时间").build();
    let started_label = gtk::Label::builder()
        .label("—")
        .css_classes(["dim-label", "numeric"])
        .build();
    started_row.add_suffix(&started_label);
    session_group.add(&started_row);

    let duration_row = adw::ActionRow::builder().title("连接时间").build();
    let duration_label = gtk::Label::builder()
        .label("—")
        .css_classes(["dim-label", "numeric"])
        .build();
    duration_row.add_suffix(&duration_label);
    session_group.add(&duration_row);
    traffic_page.add(&session_group);

    // 2. 流量
    let traffic_group = adw::PreferencesGroup::builder().title("流量").build();
    let traffic_card = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    traffic_card.add_css_class("card");
    traffic_card.set_homogeneous(true);

    let total_up_label = gtk::Label::builder().label("0 B").build();
    let total_down_label = gtk::Label::builder().label("0 B").build();
    let total_col = create_traffic_stat_column("全部", &total_up_label, &total_down_label);
    traffic_card.append(&total_col);

    traffic_card.append(&gtk::Separator::new(gtk::Orientation::Vertical));

    let proxy_up_label = gtk::Label::builder().label("0 B").build();
    let proxy_down_label = gtk::Label::builder().label("0 B").build();
    let proxy_col = create_traffic_stat_column("代理", &proxy_up_label, &proxy_down_label);
    traffic_card.append(&proxy_col);

    traffic_card.append(&gtk::Separator::new(gtk::Orientation::Vertical));

    let direct_up_label = gtk::Label::builder().label("0 B").build();
    let direct_down_label = gtk::Label::builder().label("0 B").build();
    let direct_col = create_traffic_stat_column("直连", &direct_up_label, &direct_down_label);
    traffic_card.append(&direct_col);

    traffic_group.add(&traffic_card);
    traffic_page.add(&traffic_group);

    // 3. 配置
    let config_group = adw::PreferencesGroup::builder().title("配置").build();
    let config_card = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    config_card.add_css_class("card");
    config_card.set_homogeneous(true);

    let direct_count_label = gtk::Label::builder().label("0").build();
    let direct_fill_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let direct_chart_col = create_chart_column("直连", &direct_count_label, &direct_fill_box, "chart-fill-direct");
    config_card.append(&direct_chart_col);

    let proxy_count_label = gtk::Label::builder().label("0").build();
    let proxy_fill_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let proxy_chart_col = create_chart_column("代理", &proxy_count_label, &proxy_fill_box, "chart-fill-proxy");
    config_card.append(&proxy_chart_col);

    let reject_count_label = gtk::Label::builder().label("0").build();
    let reject_fill_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let reject_chart_col = create_chart_column("拒绝", &reject_count_label, &reject_fill_box, "chart-fill-reject");
    config_card.append(&reject_chart_col);

    config_group.add(&config_card);
    traffic_page.add(&config_group);

    // 4. 应用流量
    let app_usage_group = adw::PreferencesGroup::builder().title("应用流量").build();
    let app_traffic_toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let app_traffic_search = gtk::SearchEntry::builder().placeholder_text("Search").hexpand(true).build();
    app_traffic_toolbar.append(&app_traffic_search);
    let app_traffic_sort_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let app_traffic_sort_label = gtk::Label::new(Some("Sort"));
    app_traffic_sort_label.add_css_class("dim-label");
    app_traffic_sort_box.append(&app_traffic_sort_label);
    let app_traffic_sort = gtk::DropDown::from_strings(&["Traffic", "Name"]);
    app_traffic_sort_box.append(&app_traffic_sort);
    app_traffic_toolbar.append(&app_traffic_sort_box);
    app_usage_group.add(&app_traffic_toolbar);

    let app_traffic_list_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    app_usage_group.add(&app_traffic_list_box);
    traffic_page.add(&app_usage_group);
    view_stack.add_named(&page_scroller(&traffic_page), Some("traffic"));

    let refresh_traffic_rule_counts_impl = {
        let config = config.clone();
        let direct_count_label = direct_count_label.clone();
        let proxy_count_label = proxy_count_label.clone();
        let reject_count_label = reject_count_label.clone();
        let direct_fill_box = direct_fill_box.clone();
        let proxy_fill_box = proxy_fill_box.clone();
        let reject_fill_box = reject_fill_box.clone();
        Rc::new(move || {
            let current = config.borrow();
            let imported = imported_rules(&current);
            let custom = custom_rules(&current);
            let app_count = scan_desktop_apps().len();

            let mut direct_count = 0usize;
            let mut proxy_count = 0usize;
            let mut reject_count = 0usize;

            for rule in &imported {
                match rule.action() {
                    RuleAction::Direct => direct_count += 1,
                    RuleAction::Proxy => proxy_count += 1,
                    RuleAction::Block => reject_count += 1,
                }
            }
            for rule in &custom {
                match rule.action() {
                    RuleAction::Direct => direct_count += 1,
                    RuleAction::Proxy => proxy_count += 1,
                    RuleAction::Block => reject_count += 1,
                }
            }
            let mut app_proxy = 0;
            let mut app_block = 0;
            for r in &current.settings.app_rules {
                match r.action {
                    RuleAction::Proxy => app_proxy += 1,
                    RuleAction::Block => app_block += 1,
                    RuleAction::Direct => {}
                }
            }
            let app_direct = app_count.saturating_sub(app_proxy + app_block);
            direct_count += app_direct;
            proxy_count += app_proxy;
            reject_count += app_block;

            direct_count_label.set_text(&direct_count.to_string());
            proxy_count_label.set_text(&proxy_count.to_string());
            reject_count_label.set_text(&reject_count.to_string());

            let max_count = direct_count.max(proxy_count).max(reject_count);
            let calc_height = |count: usize| -> i32 {
                if count == 0 || max_count == 0 {
                    0
                } else {
                    let ratio = count as f64 / max_count as f64;
                    ((ratio * 100.0).round() as i32).clamp(4, 100)
                }
            };

            direct_fill_box.set_height_request(calc_height(direct_count));
            proxy_fill_box.set_height_request(calc_height(proxy_count));
            reject_fill_box.set_height_request(calc_height(reject_count));
        })
    };
    *refresh_traffic_rule_counts.borrow_mut() = Some(refresh_traffic_rule_counts_impl.clone());
    refresh_traffic_rule_counts_impl();

    let app_traffic_data = Rc::new(RefCell::new(Vec::<AppTrafficStat>::new()));
    let refresh_app_traffic = {
        let app_traffic_data = app_traffic_data.clone();
        let app_traffic_search = app_traffic_search.clone();
        let app_traffic_sort = app_traffic_sort.clone();
        let app_traffic_list_box = app_traffic_list_box.clone();
        Rc::new(move || {
            let query = app_traffic_search.text().trim().to_lowercase();
            let mut items: Vec<AppTrafficStat> = app_traffic_data
                .borrow()
                .iter()
                .filter(|item| {
                    query.is_empty()
                        || item.name.to_lowercase().contains(&query)
                        || item.id.to_lowercase().contains(&query)
                })
                .cloned()
                .collect();

            if app_traffic_sort.selected() == 0 {
                items.sort_by(|a, b| (b.upload + b.download).cmp(&(a.upload + a.download)));
            } else {
                items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            }

            while let Some(child) = app_traffic_list_box.first_child() {
                app_traffic_list_box.remove(&child);
            }

            for item in items {
                let row = adw::ActionRow::builder()
                    .title(&item.name)
                    .subtitle(&format!(
                        "↑ {} · ↓ {} · Total {}",
                        format_bytes(item.upload),
                        format_bytes(item.download),
                        format_bytes(item.upload + item.download),
                    ))
                    .build();
                row.add_prefix(&create_app_icon(&item.icon));
                app_traffic_list_box.append(&row);
            }
        })
    };

    {
        let refresh = refresh_app_traffic.clone();
        app_traffic_search.connect_search_changed(move |_| refresh());
    }
    {
        let refresh = refresh_app_traffic.clone();
        app_traffic_sort.connect_selected_notify(move |_| refresh());
    }

    // Wiring up Blocked Page logic
    let on_add_proc = {
        let new_proc_row = new_proc_row.clone();
        let config = config.clone();
        let controller = controller.clone();
        let refresh_blocked = refresh_blocked_views.clone();
        let refresh_rules = refresh_rule_views.clone();
        let refresh_counts = refresh_traffic_rule_counts.clone();
        Rc::new(move || {
            let proc_name = new_proc_row.text().trim().to_string();
            if proc_name.is_empty() {
                return;
            }
            let mut current = config.borrow_mut();
            current.settings.app_rules.retain(|r| {
                r.executable.file_name().and_then(|n| n.to_str()) != Some(&proc_name)
            });
            current.settings.app_rules.push(AppRule {
                executable: PathBuf::from(&proc_name),
                action: RuleAction::Block,
            });
            let _ = current.save();
            drop(current);
            controller.borrow().sync_rules();
            new_proc_row.set_text("");
            if let Some(refresh) = refresh_blocked.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_rules.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_counts.borrow().as_ref() {
                refresh();
            }
        })
    };

    {
        let on_add = on_add_proc.clone();
        add_proc_btn.connect_clicked(move |_| on_add());
    }
    {
        let on_add = on_add_proc.clone();
        new_proc_row.connect_entry_activated(move |_| on_add());
    }

    let on_add_target = {
        let new_target_row = new_target_row.clone();
        let config = config.clone();
        let controller = controller.clone();
        let refresh_blocked = refresh_blocked_views.clone();
        let refresh_rules = refresh_rule_views.clone();
        let refresh_counts = refresh_traffic_rule_counts.clone();
        Rc::new(move || {
            let target = new_target_row.text().trim().to_string();
            if target.is_empty() {
                return;
            }
            let mut current = config.borrow_mut();
            if target.contains('/') || target.parse::<std::net::IpAddr>().is_ok() {
                let parsed = parse_rule_set(&format!("IP-CIDR,{target}"), RuleAction::Block);
                for rule in parsed.ip_rules {
                    current.settings.ip_rules.retain(|r| r.network != rule.network);
                    current.settings.ip_rules.push(rule);
                }
            } else {
                let parsed = parse_rule_set(&format!("DOMAIN-SUFFIX,{target}"), RuleAction::Block);
                for rule in parsed.domain_rules {
                    current.settings.domain_rules.retain(|r| !(r.pattern == rule.pattern && r.kind == rule.kind));
                    current.settings.domain_rules.push(rule);
                }
            }
            let _ = current.save();
            drop(current);
            controller.borrow().sync_rules();
            new_target_row.set_text("");
            if let Some(refresh) = refresh_blocked.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_rules.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_counts.borrow().as_ref() {
                refresh();
            }
        })
    };

    {
        let on_add = on_add_target.clone();
        add_target_btn.connect_clicked(move |_| on_add());
    }
    {
        let on_add = on_add_target.clone();
        new_target_row.connect_entry_activated(move |_| on_add());
    }

    let all_desktop_apps = scan_desktop_apps();
    let app_switches = Rc::new(RefCell::new(Vec::<(String, adw::ActionRow, gtk::Switch)>::new()));

    for app in &all_desktop_apps {
        let row = adw::ActionRow::builder()
            .title(&app.name)
            .subtitle(&app.executable)
            .build();
        row.add_prefix(&create_app_icon(&app.icon));

        let sw = gtk::Switch::builder().valign(gtk::Align::Center).build();
        let is_blocked = current_app_action(&config.borrow(), &app.executable) == RuleAction::Block;
        sw.set_active(is_blocked);

        let config_ref = config.clone();
        let controller_ref = controller.clone();
        let executable = app.executable.clone();
        let refresh_rules = refresh_rule_views.clone();
        let refresh_counts = refresh_traffic_rule_counts.clone();
        sw.connect_active_notify(move |sw| {
            let currently_blocked = current_app_action(&config_ref.borrow(), &executable) == RuleAction::Block;
            if sw.is_active() == currently_blocked {
                return;
            }
            let mut current = config_ref.borrow_mut();
            if sw.is_active() {
                current.settings.app_rules.retain(|r| {
                    r.executable.file_name().and_then(|n| n.to_str()) != Some(&executable)
                });
                current.settings.app_rules.push(AppRule {
                    executable: PathBuf::from(&executable),
                    action: RuleAction::Block,
                });
            } else {
                current.settings.app_rules.retain(|r| {
                    r.executable.file_name().and_then(|n| n.to_str()) != Some(&executable)
                });
            }
            let _ = current.save();
            drop(current);
            controller_ref.borrow().sync_rules();
            if let Some(refresh) = refresh_rules.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_counts.borrow().as_ref() {
                refresh();
            }
        });

        row.add_suffix(&sw);
        row.set_activatable_widget(Some(&sw));
        blocked_apps_list_box.append(&row);

        app_switches.borrow_mut().push((
            format!("{} {}", app.name, app.executable).to_lowercase(),
            row,
            sw,
        ));
    }

    {
        let app_switches = app_switches.clone();
        app_search_row.connect_changed(move |entry| {
            let query = entry.text().trim().to_lowercase();
            for (key, row, _) in app_switches.borrow().iter() {
                row.set_visible(query.is_empty() || key.contains(&query));
            }
        });
    }

    let refresh_blocked_impl: Rc<dyn Fn()> = {
        let procs_list_box = procs_list_box.clone();
        let blocked_targets_list_box = blocked_targets_list_box.clone();
        let app_switches = app_switches.clone();
        let config = config.clone();
        let controller = controller.clone();
        let refresh_blocked = refresh_blocked_views.clone();
        let refresh_rules = refresh_rule_views.clone();
        let refresh_counts = refresh_traffic_rule_counts.clone();

        Rc::new(move || {
            while let Some(child) = procs_list_box.first_child() {
                procs_list_box.remove(&child);
            }
            let current = config.borrow();
            let procs: Vec<String> = current
                .settings
                .app_rules
                .iter()
                .filter(|r| r.action == RuleAction::Block)
                .filter_map(|r| r.executable.file_name().and_then(|n| n.to_str()).map(ToString::to_string))
                .collect();

            while let Some(child) = blocked_targets_list_box.first_child() {
                blocked_targets_list_box.remove(&child);
            }
            let blocked_domains: Vec<DomainRule> = current
                .settings
                .domain_rules
                .iter()
                .filter(|r| r.action == RuleAction::Block)
                .cloned()
                .collect();
            let blocked_ips: Vec<IpRule> = current
                .settings
                .ip_rules
                .iter()
                .filter(|r| r.action == RuleAction::Block)
                .cloned()
                .collect();
            drop(current);

            for proc_name in procs {
                let row = adw::ActionRow::builder()
                    .title(&proc_name)
                    .subtitle("Blocked from network")
                    .build();
                let icon = gtk::Image::from_icon_name("network-offline-symbolic");
                icon.set_valign(gtk::Align::Center);
                row.add_prefix(&icon);

                let del_btn = gtk::Button::from_icon_name("user-trash-symbolic");
                del_btn.add_css_class("flat");
                del_btn.add_css_class("destructive-action");
                del_btn.set_valign(gtk::Align::Center);

                let target = proc_name.clone();
                let config_ref = config.clone();
                let controller_ref = controller.clone();
                let refresh_blocked_ref = refresh_blocked.clone();
                let refresh_rules_ref = refresh_rules.clone();
                let refresh_counts_ref = refresh_counts.clone();
                del_btn.connect_clicked(move |_| {
                    let mut current = config_ref.borrow_mut();
                    current.settings.app_rules.retain(|r| {
                        r.executable.file_name().and_then(|n| n.to_str()) != Some(&target)
                    });
                    let _ = current.save();
                    drop(current);
                    controller_ref.borrow().sync_rules();
                    if let Some(refresh) = refresh_blocked_ref.borrow().as_ref() {
                        refresh();
                    }
                    if let Some(refresh) = refresh_rules_ref.borrow().as_ref() {
                        refresh();
                    }
                    if let Some(refresh) = refresh_counts_ref.borrow().as_ref() {
                        refresh();
                    }
                });
                row.add_suffix(&del_btn);
                procs_list_box.append(&row);
            }

            for rule in blocked_domains {
                let row = adw::ActionRow::builder()
                    .title(&rule.pattern)
                    .subtitle(&format!("{} · REJECT", domain_kind_label(rule.kind)))
                    .build();
                let icon = gtk::Image::from_icon_name("network-server-symbolic");
                icon.set_valign(gtk::Align::Center);
                row.add_prefix(&icon);

                let del_btn = gtk::Button::from_icon_name("user-trash-symbolic");
                del_btn.add_css_class("flat");
                del_btn.add_css_class("destructive-action");
                del_btn.set_valign(gtk::Align::Center);

                let pattern = rule.pattern.clone();
                let kind = rule.kind;
                let config_ref = config.clone();
                let controller_ref = controller.clone();
                let refresh_blocked_ref = refresh_blocked.clone();
                let refresh_rules_ref = refresh_rules.clone();
                let refresh_counts_ref = refresh_counts.clone();
                del_btn.connect_clicked(move |_| {
                    let mut current = config_ref.borrow_mut();
                    current.settings.domain_rules.retain(|r| !(r.pattern == pattern && r.kind == kind));
                    let _ = current.save();
                    drop(current);
                    controller_ref.borrow().sync_rules();
                    if let Some(refresh) = refresh_blocked_ref.borrow().as_ref() {
                        refresh();
                    }
                    if let Some(refresh) = refresh_rules_ref.borrow().as_ref() {
                        refresh();
                    }
                    if let Some(refresh) = refresh_counts_ref.borrow().as_ref() {
                        refresh();
                    }
                });
                row.add_suffix(&del_btn);
                blocked_targets_list_box.append(&row);
            }

            for rule in blocked_ips {
                let row = adw::ActionRow::builder()
                    .title(&rule.network.to_string())
                    .subtitle("IP-CIDR · REJECT")
                    .build();
                let icon = gtk::Image::from_icon_name("network-server-symbolic");
                icon.set_valign(gtk::Align::Center);
                row.add_prefix(&icon);

                let del_btn = gtk::Button::from_icon_name("user-trash-symbolic");
                del_btn.add_css_class("flat");
                del_btn.add_css_class("destructive-action");
                del_btn.set_valign(gtk::Align::Center);

                let network = rule.network;
                let config_ref = config.clone();
                let controller_ref = controller.clone();
                let refresh_blocked_ref = refresh_blocked.clone();
                let refresh_rules_ref = refresh_rules.clone();
                let refresh_counts_ref = refresh_counts.clone();
                del_btn.connect_clicked(move |_| {
                    let mut current = config_ref.borrow_mut();
                    current.settings.ip_rules.retain(|r| r.network != network);
                    let _ = current.save();
                    drop(current);
                    controller_ref.borrow().sync_rules();
                    if let Some(refresh) = refresh_blocked_ref.borrow().as_ref() {
                        refresh();
                    }
                    if let Some(refresh) = refresh_rules_ref.borrow().as_ref() {
                        refresh();
                    }
                    if let Some(refresh) = refresh_counts_ref.borrow().as_ref() {
                        refresh();
                    }
                });
                row.add_suffix(&del_btn);
                blocked_targets_list_box.append(&row);
            }

            for (_, row, sw) in app_switches.borrow().iter() {
                if let Some(exec) = row.subtitle().map(|s| s.to_string()) {
                    let is_blocked = current_app_action(&config.borrow(), &exec) == RuleAction::Block;
                    if sw.is_active() != is_blocked {
                        sw.set_active(is_blocked);
                    }
                }
            }
        })
    };
    *refresh_blocked_views.borrow_mut() = Some(refresh_blocked_impl.clone());
    refresh_blocked_impl();

    let log_page = gtk::Box::new(gtk::Orientation::Vertical, 12);
    log_page.set_margin_start(18);
    log_page.set_margin_end(18);
    log_page.set_margin_top(18);
    log_page.set_margin_bottom(18);
    log_page.set_hexpand(true);
    log_page.set_vexpand(true);
    let log_header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    log_header.set_margin_bottom(4);

    let log_stack = gtk::Stack::new();
    log_stack.set_vexpand(true);
    log_stack.set_hexpand(true);

    let log_switcher = gtk::StackSwitcher::new();
    log_switcher.set_stack(Some(&log_stack));
    log_switcher.set_halign(gtk::Align::Center);
    log_switcher.set_hexpand(true);
    log_header.append(&log_switcher);

    let copy_logs = gtk::Button::from_icon_name("edit-copy-symbolic");
    copy_logs.add_css_class("flat");
    copy_logs.set_tooltip_text(Some("Copy logs"));
    log_header.append(&copy_logs);

    let clear_logs = gtk::Button::from_icon_name("edit-clear-all-symbolic");
    clear_logs.add_css_class("flat");
    clear_logs.set_tooltip_text(Some("Clear logs"));
    log_header.append(&clear_logs);

    log_page.append(&log_header);

    let all_log_view = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .build();
    let all_log_buffer = all_log_view.buffer();
    let all_log_scroller = gtk::ScrolledWindow::builder()
        .child(&all_log_view)
        .min_content_height(180)
        .hexpand(true)
        .vexpand(true)
        .build();
    log_stack.add_titled(&all_log_scroller, Some("all"), "All");

    let system_log_view = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .build();
    let system_log_buffer = system_log_view.buffer();
    let system_log_scroller = gtk::ScrolledWindow::builder()
        .child(&system_log_view)
        .min_content_height(180)
        .hexpand(true)
        .vexpand(true)
        .build();
    log_stack.add_titled(&system_log_scroller, Some("system"), "System");

    let proxy_log_view = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .build();
    let proxy_log_buffer = proxy_log_view.buffer();
    let proxy_log_scroller = gtk::ScrolledWindow::builder()
        .child(&proxy_log_view)
        .min_content_height(180)
        .hexpand(true)
        .vexpand(true)
        .build();
    log_stack.add_titled(&proxy_log_scroller, Some("proxy"), "Proxy");

    let direct_log_view = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .build();
    let direct_log_buffer = direct_log_view.buffer();
    let direct_log_scroller = gtk::ScrolledWindow::builder()
        .child(&direct_log_view)
        .min_content_height(180)
        .hexpand(true)
        .vexpand(true)
        .build();
    log_stack.add_titled(&direct_log_scroller, Some("direct"), "Direct");

    log_page.append(&log_stack);
    view_stack.add_named(&log_page, Some("logs"));

    {
        let all_buffer = all_log_buffer.clone();
        let system_buffer = system_log_buffer.clone();
        let proxy_buffer = proxy_log_buffer.clone();
        let direct_buffer = direct_log_buffer.clone();
        let stack = log_stack.clone();
        clear_logs.connect_clicked(move |_| {
            match stack.visible_child_name().as_deref() {
                Some("all") => all_buffer.set_text(""),
                Some("system") => system_buffer.set_text(""),
                Some("direct") => direct_buffer.set_text(""),
                _ => proxy_buffer.set_text(""),
            }
        });
    }
    {
        let all_buffer = all_log_buffer.clone();
        let system_buffer = system_log_buffer.clone();
        let proxy_buffer = proxy_log_buffer.clone();
        let direct_buffer = direct_log_buffer.clone();
        let stack = log_stack.clone();
        copy_logs.connect_clicked(move |_| {
            let target_buffer = match stack.visible_child_name().as_deref() {
                Some("all") => &all_buffer,
                Some("system") => &system_buffer,
                Some("direct") => &direct_buffer,
                _ => &proxy_buffer,
            };
            let text = target_buffer.text(&target_buffer.start_iter(), &target_buffer.end_iter(), false);
            if let Some(display) = gtk::gdk::Display::default() {
                display.clipboard().set_text(&text);
            }
        });
    }

    {
        let view_stack = view_stack.clone();
        let page_title = page_title.clone();
        let add_connection = add_connection.clone();
        navigation.connect_row_selected(move |_, row| {
            let Some(row) = row else { return; };
            let (name, title) = match row.index() {
                1 => ("rules", "Rules"),
                2 => ("traffic", "Traffic"),
                3 => ("logs", "Logs"),
                _ => ("connect", "Connect"),
            };
            view_stack.set_visible_child_name(name);
            page_title.set_text(title);
            add_connection.set_visible(name == "connect");
        });
    }
    navigation.select_row(Some(&connect_nav));

    let bottom_bar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    bottom_bar.set_margin_start(16);
    bottom_bar.set_margin_end(16);
    bottom_bar.set_margin_top(8);
    bottom_bar.set_margin_bottom(8);
    let bottom_status = gtk::Label::new(Some("Disconnected"));
    bottom_status.add_css_class("dim-label");
    bottom_status.set_halign(gtk::Align::Start);
    bottom_status.set_hexpand(true);
    bottom_bar.append(&bottom_status);
    let speed_label = gtk::Label::new(Some("↑ 0 B/s   ↓ 0 B/s"));
    speed_label.add_css_class("dim-label");
    speed_label.set_halign(gtk::Align::End);
    bottom_bar.append(&speed_label);

    toolbar.set_content(Some(&view_stack));
    toolbar.add_bottom_bar(&bottom_bar);
    root.append(&toolbar);
    window.set_content(Some(&root));

    let session_upload = Rc::new(RefCell::new(0_u64));
    let session_download = Rc::new(RefCell::new(0_u64));

    {
        let config = config.clone();
        policy.connect_selected_notify(move |row| {
            let mut current = config.borrow_mut();
            current.settings.default_policy = match row.selected() {
                1 => RuleAction::Direct,
                2 => RuleAction::Block,
                _ => RuleAction::Proxy,
            };
            let _ = current.save();
        });
    }
    {
        let config = config.clone();
        ipv6.connect_active_notify(move |row| {
            let mut current = config.borrow_mut();
            current.settings.ipv6 = row.is_active();
            let _ = current.save();
        });
    }

    {
        let connection_flow = connection_flow.clone();
        let connect_stack = connect_stack.clone();
        let config = config.clone();
        let controller = controller.clone();
        let event_tx = event_tx.clone();
        let is_connected = is_connected.clone();
        let connection_buttons = connection_buttons.clone();
        let refresh_handle = refresh_connections.clone();
        let tray_manager = tray_manager.clone();
        let parent = window.clone();
        let bottom_status = bottom_status.clone();
        let refresh_impl: Rc<dyn Fn()> = Rc::new(move || {
            while let Some(child) = connection_flow.first_child() {
                let Ok(child) = child.downcast::<gtk::FlowBoxChild>() else { break; };
                connection_flow.remove(&child);
            }
            connection_buttons.borrow_mut().clear();

            let profiles = config.borrow().profiles.clone();
            if profiles.is_empty() {
                connect_stack.set_visible_child_name("empty");
                if let Some(tray) = tray_manager.borrow().as_ref() {
                    tray.refresh_menu();
                }
                return;
            }
            connect_stack.set_visible_child_name("cards");

            for profile in profiles {
                let card = gtk::Box::new(gtk::Orientation::Vertical, 10);
                card.add_css_class("card");
                card.set_size_request(280, -1);
                card.set_valign(gtk::Align::Start);
                card.set_vexpand(false);
                card.set_margin_start(4);
                card.set_margin_end(4);
                card.set_margin_top(4);
                card.set_margin_bottom(4);

                let header_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                header_row.set_margin_start(16);
                header_row.set_margin_end(10);
                header_row.set_margin_top(14);
                let title = gtk::Label::new(Some(&profile.name));
                title.add_css_class("title-3");
                title.set_halign(gtk::Align::Start);
                title.set_hexpand(true);
                title.set_ellipsize(gtk::pango::EllipsizeMode::End);
                header_row.append(&title);
                let edit = gtk::Button::from_icon_name("document-edit-symbolic");
                edit.add_css_class("flat");
                edit.set_tooltip_text(Some("Edit"));
                header_row.append(&edit);
                let remove = gtk::Button::from_icon_name("user-trash-symbolic");
                remove.add_css_class("flat");
                remove.set_tooltip_text(Some("Delete"));
                header_row.append(&remove);
                card.append(&header_row);

                let info = gtk::Box::new(gtk::Orientation::Vertical, 6);
                info.set_margin_start(16);
                info.set_margin_end(16);
                for (key, value) in [
                    ("Server", profile.host.clone()),
                    ("Port", profile.port.to_string()),
                    ("Username", if profile.username.is_empty() { "—".into() } else { profile.username.clone() }),
                    (
                        "Identity",
                        profile.identity_file.as_ref()
                            .map(|path| path.to_string_lossy().to_string())
                            .unwrap_or_else(|| "—".into()),
                    ),
                ] {
                    let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                    let key_label = gtk::Label::new(Some(key));
                    key_label.add_css_class("dim-label");
                    key_label.set_halign(gtk::Align::Start);
                    key_label.set_hexpand(true);
                    let value_label = gtk::Label::new(Some(&value));
                    value_label.set_halign(gtk::Align::End);
                    value_label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
                    line.append(&key_label);
                    line.append(&value_label);
                    info.append(&line);
                }
                card.append(&info);
                card.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

                let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                actions.set_margin_start(16);
                actions.set_margin_end(16);
                actions.set_margin_bottom(14);
                let connect_button = gtk::Button::with_label("Connect");
                connect_button.add_css_class("suggested-action");
                connect_button.add_css_class("pill");
                connect_button.set_halign(gtk::Align::End);
                if *is_connected.borrow() {
                    let is_active = config.borrow().active_profile == Some(profile.id);
                    connect_button.set_label(if is_active { "Disconnect" } else { "Connect" });
                    connect_button.set_sensitive(is_active);
                }
                actions.append(&gtk::Box::new(gtk::Orientation::Horizontal, 0));
                actions.last_child().unwrap().set_hexpand(true);
                actions.append(&connect_button);
                card.append(&actions);

                connection_buttons.borrow_mut().push((profile.id.to_string(), connect_button.clone()));

                {
                    let parent = parent.clone();
                    let config = config.clone();
                    let refresh_handle = refresh_handle.clone();
                    let profile = profile.clone();
                    edit.connect_clicked(move |_| {
                        show_profile_dialog(&parent, config.clone(), Some(profile.clone()), refresh_handle.clone());
                    });
                }
                {
                    let config = config.clone();
                    let refresh_handle = refresh_handle.clone();
                    let is_connected = is_connected.clone();
                    let profile_id = profile.id;
                    remove.connect_clicked(move |_| {
                        let mut current = config.borrow_mut();
                        if *is_connected.borrow() && current.active_profile == Some(profile_id) {
                            return;
                        }
                        current.profiles.retain(|item| item.id != profile_id);
                        if current.active_profile == Some(profile_id) {
                            current.active_profile = current.profiles.first().map(|item| item.id);
                        }
                        if current.save().is_ok() {
                            drop(current);
                            if let Some(refresh) = refresh_handle.borrow().as_ref() {
                                refresh();
                            }
                        }
                    });
                }
                {
                    let config = config.clone();
                    let controller = controller.clone();
                    let event_tx = event_tx.clone();
                    let is_connected = is_connected.clone();
                    let connection_buttons = connection_buttons.clone();
                    let bottom_status = bottom_status.clone();
                    let tray_manager = tray_manager.clone();
                    let profile = profile.clone();
                    let connect_button_ref = connect_button.clone();
                    connect_button.connect_clicked(move |_| {
                        let is_active = config.borrow().active_profile == Some(profile.id);
                        if (*is_connected.borrow() || controller.borrow().is_running()) && is_active {
                            bottom_status.set_text("Disconnecting…");
                            connect_button_ref.set_sensitive(false);
                            if let Some(tray) = tray_manager.borrow().as_ref() {
                                tray.set_state(TrayConnectionState::Disconnecting);
                            }
                            controller.borrow().stop();
                            return;
                        }
                        {
                            let mut current = config.borrow_mut();
                            current.active_profile = Some(profile.id);
                            if let Err(error) = current.save() {
                                bottom_status.set_text(&format!("Config error: {error}"));
                                return;
                            }
                        }
                        let Ok(config_path) = AppConfig::path() else {
                            bottom_status.set_text("Cannot determine config path");
                            return;
                        };
                        bottom_status.set_text("Connecting…");
                        for (_, button) in connection_buttons.borrow().iter() {
                            button.set_sensitive(false);
                            button.set_label("Connect");
                        }
                        connect_button_ref.set_label("Connecting…");
                        if let Some(tray) = tray_manager.borrow().as_ref() {
                            tray.set_state(TrayConnectionState::Connecting);
                        }
                        controller.borrow().start(
                            profile.clone(),
                            config.borrow().clone(),
                            config_path,
                            event_tx.clone(),
                        );
                    });
                }

                connection_flow.insert(&card, -1);
            }
            if let Some(tray) = tray_manager.borrow().as_ref() {
                tray.refresh_menu();
            }
        });
        *refresh_connections.borrow_mut() = Some(refresh_impl.clone());
        refresh_impl();
    }
    {
        let parent = window.clone();
        let config = config.clone();
        let refresh_connections = refresh_connections.clone();
        add_connection.connect_clicked(move |_| {
            show_profile_dialog(&parent, config.clone(), None, refresh_connections.clone());
        });
    }
    {
        let profiles_config = config.clone();
        let select_config = config.clone();
        let select_refresh = refresh_connections.clone();
        let select_connected = is_connected.clone();
        let toggle_config = config.clone();
        let toggle_buttons = connection_buttons.clone();
        let show_window = window.clone();
        let quit_app = app.clone();
        let quit_controller = controller.clone();
        let quitting_ref = quitting.clone();
        let log_buffer_ref = proxy_log_buffer.clone();
        match TrayManager::new(
            Rc::new(move || {
                let current = profiles_config.borrow();
                current
                    .profiles
                    .iter()
                    .map(|profile| {
                        (
                            profile.name.clone(),
                            current.active_profile == Some(profile.id),
                        )
                    })
                    .collect()
            }),
            Rc::new(move |index| {
                if *select_connected.borrow() {
                    return;
                }
                let mut current = select_config.borrow_mut();
                let Some(profile_id) = current.profiles.get(index).map(|profile| profile.id) else {
                    return;
                };
                current.active_profile = Some(profile_id);
                if current.save().is_ok() {
                    drop(current);
                    if let Some(refresh) = select_refresh.borrow().as_ref() {
                        refresh();
                    }
                }
            }),
            Rc::new(move || {
                let active_id = toggle_config.borrow().active_profile.map(|id| id.to_string());
                let Some(active_id) = active_id else {
                    return;
                };
                if let Some((_, button)) = toggle_buttons
                    .borrow()
                    .iter()
                    .find(|(profile_id, _)| profile_id == &active_id)
                {
                    button.emit_clicked();
                }
            }),
            Rc::new(move || show_window.present()),
            Rc::new(move || {
                *quitting_ref.borrow_mut() = true;
                quit_controller.borrow().shutdown();
                quit_app.quit();
            }),
        ) {
            Ok(manager) => {
                *tray_manager.borrow_mut() = Some(Rc::new(manager));
            }
            Err(error) => {
                let mut end = log_buffer_ref.end_iter();
                log_buffer_ref.insert(&mut end, &format!("[tray] {error}\n"));
            }
        }
    }
    {
        let tray_manager = tray_manager.clone();
        let quitting = quitting.clone();
        window.connect_close_request(move |window| {
            if !*quitting.borrow() && tray_manager.borrow().is_some() {
                window.set_visible(false);
                gtk::glib::Propagation::Stop
            } else {
                gtk::glib::Propagation::Proceed
            }
        });
    }
    {
        let parent = window.clone();
        let config = config.clone();
        let refresh_connections = refresh_connections.clone();
        empty_add.connect_clicked(move |_| {
            show_profile_dialog(&parent, config.clone(), None, refresh_connections.clone());
        });
    }
    {
        let event_tx = event_tx.clone();
        let rule_source = rule_source.clone();
        let import_rules = import_rules.clone();
        let import_button = import_button.clone();
        let rule_status = rule_status.clone();
        import_rules.clone().connect_clicked(move |_| {
            let url = rule_source.text().trim().to_string();
            if url.is_empty() {
                rule_status.set_subtitle("Rule source URL is empty");
                return;
            }
            import_rules.set_sensitive(false);
            import_button.set_sensitive(false);
            rule_status.set_subtitle("Downloading and parsing…");
            let events = event_tx.clone();
            thread::spawn(move || match import_rule_source(&url) {
                Ok(result) => {
                    let _ = events.send(RuntimeEvent::RulesImported { result, source_url: url });
                }
                Err(error) => {
                    let _ = events.send(RuntimeEvent::RuleImportFailed(error));
                }
            });
        });
    }
    {
        let is_connected = is_connected.clone();
        let connection_buttons = connection_buttons.clone();
        let config = config.clone();
        let policy = policy.clone();
        let rule_status = rule_status.clone();
        let import_rules = import_rules.clone();
        let import_button = import_button.clone();
        let refresh_rule_views = refresh_rule_views.clone();
        let proxy_log_buffer = proxy_log_buffer.clone();
        let proxy_log_view = proxy_log_view.clone();
        let direct_log_buffer = direct_log_buffer.clone();
        let direct_log_view = direct_log_view.clone();
        let bottom_status = bottom_status.clone();
        let speed_label = speed_label.clone();
        let started_label = started_label.clone();
        let duration_label = duration_label.clone();
        let connect_start_time = connect_start_time.clone();
        let total_up_label = total_up_label.clone();
        let total_down_label = total_down_label.clone();
        let proxy_up_label = proxy_up_label.clone();
        let proxy_down_label = proxy_down_label.clone();
        let direct_up_label = direct_up_label.clone();
        let direct_down_label = direct_down_label.clone();
        let session_upload = session_upload.clone();
        let session_download = session_download.clone();
        let tray_manager = tray_manager.clone();
        let controller_ref = controller.clone();
        let app_traffic_data = app_traffic_data.clone();
        let refresh_app_traffic = refresh_app_traffic.clone();

        let update_traffic_labels = {
            let session_upload = session_upload.clone();
            let session_download = session_download.clone();
            let app_traffic_data = app_traffic_data.clone();
            let total_up_label = total_up_label.clone();
            let total_down_label = total_down_label.clone();
            let proxy_up_label = proxy_up_label.clone();
            let proxy_down_label = proxy_down_label.clone();
            let direct_up_label = direct_up_label.clone();
            let direct_down_label = direct_down_label.clone();
            Rc::new(move || {
                let up_proxy = *session_upload.borrow();
                let down_proxy = *session_download.borrow();
                let (apps_up, apps_down) = app_traffic_data.borrow().iter().fold((0u64, 0u64), |(u, d), app| {
                    (u + app.upload, d + app.download)
                });
                let total_up = up_proxy.max(apps_up);
                let total_down = down_proxy.max(apps_down);
                let direct_up = total_up.saturating_sub(up_proxy);
                let direct_down = total_down.saturating_sub(down_proxy);

                total_up_label.set_text(&format_bytes(total_up));
                total_down_label.set_text(&format_bytes(total_down));
                proxy_up_label.set_text(&format_bytes(up_proxy));
                proxy_down_label.set_text(&format_bytes(down_proxy));
                direct_up_label.set_text(&format_bytes(direct_up));
                direct_down_label.set_text(&format_bytes(direct_down));
            })
        };

        gtk::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
            while let Ok(event) = event_rx.try_recv() {
                match event {
                    RuntimeEvent::Connected => {
                        *session_upload.borrow_mut() = 0;
                        *session_download.borrow_mut() = 0;
                        app_traffic_data.borrow_mut().clear();
                        refresh_app_traffic();
                        update_traffic_labels();
                        *connect_start_time.borrow_mut() = Some(std::time::Instant::now());
                        if let Ok(now) = gtk::glib::DateTime::now_local() {
                            started_label.set_text(&now.format("%Y-%m-%d %H:%M:%S").map_or_else(|_| "—".into(), |s| s.to_string()));
                        }
                        duration_label.set_text("00:00:00");
                        *is_connected.borrow_mut() = true;
                        bottom_status.set_text("Connected");
                        if let Some(tray) = tray_manager.borrow().as_ref() {
                            tray.set_state(TrayConnectionState::Connected);
                        }
                        let active_id = config.borrow().active_profile.map(|id| id.to_string());
                        for (profile_id, button) in connection_buttons.borrow().iter() {
                            let is_active = active_id.as_ref() == Some(profile_id);
                            button.set_label(if is_active { "Disconnect" } else { "Connect" });
                            button.set_sensitive(is_active);
                        }
                    }
                    RuntimeEvent::Disconnected => {
                        *is_connected.borrow_mut() = false;
                        *connect_start_time.borrow_mut() = None;
                        bottom_status.set_text("Disconnected");
                        if let Some(tray) = tray_manager.borrow().as_ref() {
                            tray.set_state(TrayConnectionState::Disconnected);
                        }
                        speed_label.set_text("↑ 0 B/s   ↓ 0 B/s");
                        for (_, button) in connection_buttons.borrow().iter() {
                            button.set_label("Connect");
                            button.set_sensitive(true);
                        }
                    }
                    RuntimeEvent::Status(status) => {
                        bottom_status.set_text(&status);
                        if let Some(tray) = tray_manager.borrow().as_ref() {
                            tray.set_state(TrayConnectionState::Connecting);
                        }
                        let active_id = config.borrow().active_profile.map(|id| id.to_string());
                        for (profile_id, button) in connection_buttons.borrow().iter() {
                            let is_active = active_id.as_ref() == Some(profile_id);
                            if is_active {
                                button.set_label("Disconnect");
                                button.set_sensitive(true);
                            } else {
                                button.set_label("Connect");
                                button.set_sensitive(false);
                            }
                        }
                    }
                    RuntimeEvent::Error(error) => {
                        *is_connected.borrow_mut() = false;
                        *connect_start_time.borrow_mut() = None;
                        bottom_status.set_text(&error);
                        if let Some(tray) = tray_manager.borrow().as_ref() {
                            tray.set_state(TrayConnectionState::Disconnected);
                        }
                        speed_label.set_text("↑ 0 B/s   ↓ 0 B/s");
                        for (_, button) in connection_buttons.borrow().iter() {
                            button.set_label("Connect");
                            button.set_sensitive(true);
                        }
                        import_rules.set_sensitive(true);
                        import_button.set_sensitive(true);
                    }
                    RuntimeEvent::Log(line) => {
                        let time_str = gtk::glib::DateTime::now_local()
                            .and_then(|dt| dt.format("%H:%M:%S"))
                            .map(|gstr| gstr.to_string())
                            .unwrap_or_else(|_| "00:00:00".to_string());
                        let formatted = format!("[{time_str}] {line}\n");

                        let append_to = |buffer: &gtk::TextBuffer, view: &gtk::TextView| {
                            let mut end = buffer.end_iter();
                            buffer.insert(&mut end, &formatted);
                            let end = buffer.end_iter();
                            let mark = buffer.create_mark(None, &end, false);
                            view.scroll_mark_onscreen(&mark);
                        };

                        append_to(&all_log_buffer, &all_log_view);

                        let is_proxy = line.contains("[proxy]") || line.contains("-> Proxy");
                        let is_direct = line.contains("[direct]") || line.contains("-> Direct") || line.contains("Direct (");
                        if is_proxy {
                            append_to(&proxy_log_buffer, &proxy_log_view);
                        } else if is_direct {
                            append_to(&direct_log_buffer, &direct_log_view);
                        } else {
                            append_to(&system_log_buffer, &system_log_view);
                        }
                    }
                    RuntimeEvent::Speed { upload, download } => {
                        *session_upload.borrow_mut() += upload;
                        *session_download.borrow_mut() += download;
                        let up_total = *session_upload.borrow();
                        let down_total = *session_download.borrow();
                        update_traffic_labels();
                        speed_label.set_text(&format!(
                            "↑ {} ({})   ↓ {} ({})",
                            format_speed(upload), format_bytes(up_total),
                            format_speed(download), format_bytes(down_total)
                        ));
                    }
                    RuntimeEvent::AppTraffic(stats) => {
                        *app_traffic_data.borrow_mut() = stats;
                        refresh_app_traffic();
                        update_traffic_labels();
                    }
                    RuntimeEvent::RuleImportFailed(error) => {
                        rule_status.set_subtitle(&format!("Import failed: {error}"));
                        import_rules.set_sensitive(true);
                        import_button.set_sensitive(true);
                    }
                    RuntimeEvent::RulesImported { result, source_url } => {
                        let (direct, proxy, block) = result.action_counts();
                        let total = result.rule_count();
                        {
                            let mut current = config.borrow_mut();
                            current.settings.imported_domain_rules = result.domain_rules;
                            current.settings.imported_ip_rules = result.ip_rules;
                            current.settings.default_policy = result.default_policy;
                            current.settings.rule_source_name = rule_source_name(&source_url);
                            current.settings.rule_source_url = source_url;
                            current.settings.rule_source_updated_at = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .map_or(0, |duration| duration.as_secs() as i64);
                            if let Err(error) = current.save() {
                                rule_status.set_subtitle(&format!("Config error: {error}"));
                                import_rules.set_sensitive(true);
                                import_button.set_sensitive(true);
                                continue;
                            }
                        }
                        policy.set_selected(match result.default_policy {
                            RuleAction::Proxy => 0,
                            RuleAction::Direct => 1,
                            RuleAction::Block => 2,
                        });
                        rule_status.set_subtitle(&format!(
                            "{total} rules · {direct} direct · {proxy} proxy · {block} block · {} ignored",
                            result.ignored_count
                        ));
                        let mut end = proxy_log_buffer.end_iter();
                        proxy_log_buffer.insert(
                            &mut end,
                            &format!("[rules] imported {total} rules ({direct} direct, {proxy} proxy, {block} block)\n"),
                        );
                        for warning in result.warnings.iter().take(3) {
                            let mut end = proxy_log_buffer.end_iter();
                            proxy_log_buffer.insert(&mut end, &format!("[rules] warning: {warning}\n"));
                        }
                        import_rules.set_sensitive(true);
                        import_button.set_sensitive(true);
                        if let Some(refresh) = refresh_rule_views.borrow().as_ref() {
                            refresh();
                        }
                        controller_ref.borrow().sync_rules();
                    }
                }
            }
            if let Some(start) = *connect_start_time.borrow() {
                let elapsed = start.elapsed().as_secs();
                duration_label.set_text(&format_duration(elapsed));
            }
            gtk::glib::ControlFlow::Continue
        });
    }

    window.present();
}
