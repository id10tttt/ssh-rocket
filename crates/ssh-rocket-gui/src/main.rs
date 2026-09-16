mod tray;

use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use ssh_rocket_core::{
    AppConfig, AppRule, DomainRuleKind, Profile, RuleAction, RuleImportResult,
    parse_rule_set, parse_shadowrocket_rules,
};
use ssh_rocket_runtime::SshSession;
use tray::{TrayConnectionState, TrayManager};
use std::{cell::RefCell, collections::HashSet, fs, path::{Path, PathBuf}, process::{Command as StdCommand, Stdio}, rc::Rc, sync::mpsc, thread, time::{Duration, SystemTime, UNIX_EPOCH}};
use tokio::{io::{AsyncBufReadExt, BufReader}, process::Command, sync::oneshot};

const APP_ID: &str = "io.github.idi0t.SshRocket";
const SOCKS_PORT: u16 = 17880;
const DEFAULT_RULE_SOURCE: &str = "https://johnshall.github.io/Shadowrocket-ADBlock-Rules-Forever/sr_top500_banlist_ad.conf";
const MAX_RULE_SOURCE_SIZE: usize = 16 * 1024 * 1024;

enum RuntimeEvent {
    Connected,
    Disconnected,
    Error(String),
    Log(String),
    Speed { upload: u64, download: u64 },
    RuleImportFailed(String),
    RulesImported {
        result: RuleImportResult,
        source_url: String,
    },
}

#[derive(Default)]
struct RuntimeController {
    stop: Option<oneshot::Sender<()>>,
}

#[derive(Clone)]
struct DesktopApp {
    name: String,
    executable: String,
    icon: String,
}

type RefreshConnections = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

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

fn add_domain_rule_row(
    group: &adw::PreferencesGroup,
    config: &Rc<RefCell<AppConfig>>,
    pattern: String,
    kind: DomainRuleKind,
    action: RuleAction,
) {
    let row = adw::ActionRow::builder()
        .title(&pattern)
        .subtitle(format!("{} · {}", domain_kind_label(kind), action_label(action)))
        .build();
    let remove = gtk::Button::from_icon_name("user-trash-symbolic");
    remove.add_css_class("flat");
    remove.set_tooltip_text(Some("Remove"));
    row.add_suffix(&remove);
    let group_ref = group.clone();
    let config_ref = config.clone();
    let row_ref = row.clone();
    remove.connect_clicked(move |_| {
        let mut current = config_ref.borrow_mut();
        current.settings.domain_rules.retain(|rule| !(rule.pattern == pattern && rule.kind == kind));
        if current.save().is_ok() {
            group_ref.remove(&row_ref);
        }
    });
    group.add(&row);
}

fn add_ip_rule_row(
    group: &adw::PreferencesGroup,
    config: &Rc<RefCell<AppConfig>>,
    network: String,
    action: RuleAction,
) {
    let row = adw::ActionRow::builder()
        .title(&network)
        .subtitle(format!("IP-CIDR · {}", action_label(action)))
        .build();
    let remove = gtk::Button::from_icon_name("user-trash-symbolic");
    remove.add_css_class("flat");
    remove.set_tooltip_text(Some("Remove"));
    row.add_suffix(&remove);
    let group_ref = group.clone();
    let config_ref = config.clone();
    let row_ref = row.clone();
    remove.connect_clicked(move |_| {
        let mut current = config_ref.borrow_mut();
        current.settings.ip_rules.retain(|rule| rule.network.to_string() != network);
        if current.save().is_ok() {
            group_ref.remove(&row_ref);
        }
    });
    group.add(&row);
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
    fn start(
        &mut self,
        profile: Profile,
        config: AppConfig,
        config_path: PathBuf,
        events: mpsc::Sender<RuntimeEvent>,
    ) {
        self.stop();
        let (stop_tx, stop_rx) = oneshot::channel();
        self.stop = Some(stop_tx);

        thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build();
            let Ok(runtime) = runtime else {
                let _ = events.send(RuntimeEvent::Error("Failed to create runtime".into()));
                return;
            };
            runtime.block_on(async move {
                let mut ssh = match SshSession::start(&profile, SOCKS_PORT, config.settings.dns_server).await {
                    Ok(session) => session,
                    Err(error) => {
                        let _ = events.send(RuntimeEvent::Error(error.to_string()));
                        return;
                    }
                };
                if let Some(stderr) = ssh.take_stderr() {
                    spawn_log_reader(stderr, "ssh", events.clone());
                }

                let helper = helper_path();
                let uid = unsafe { libc::getuid() };
                let mut helper_command = Command::new("pkexec");
                helper_command
                    .arg(helper)
                    .arg("run")
                    .arg(config_path)
                    .arg(uid.to_string())
                    .arg(ssh.socks_port.to_string())
                    .arg(ssh.dns_port.to_string())
                    .arg(ssh.server_port.to_string());
                for address in &ssh.server_addresses {
                    helper_command.arg(address.to_string());
                }
                let mut process = match helper_command
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped())
                    .kill_on_drop(true)
                    .spawn()
                {
                    Ok(process) => process,
                    Err(error) => {
                        let _ = ssh.stop().await;
                        let _ = events.send(RuntimeEvent::Error(format!("Failed to start helper: {error}")));
                        return;
                    }
                };
                let Some(stderr) = process.stderr.take() else {
                    let _ = ssh.stop().await;
                    let _ = events.send(RuntimeEvent::Error("Failed to capture helper output".into()));
                    return;
                };
                let mut helper_lines = BufReader::new(stderr).lines();
                let mut stop_rx = stop_rx;
                let mut traffic_interval = tokio::time::interval(Duration::from_secs(1));
                let startup_timeout = tokio::time::sleep(Duration::from_secs(20));
                tokio::pin!(startup_timeout);
                let mut previous_traffic = None;
                let mut helper_ready = false;
                let mut helper_stderr_open = true;
                let mut last_helper_line = None;
                let mut had_error = false;
                loop {
                    tokio::select! {
                        _ = &mut stop_rx => {
                            break;
                        }
                        status = process.wait() => {
                            let message = match status {
                                Ok(status) => last_helper_line.unwrap_or_else(|| format!("Helper exited with {status}")),
                                Err(error) => format!("Helper failed: {error}"),
                            };
                            let _ = events.send(RuntimeEvent::Error(message));
                            had_error = true;
                            break;
                        }
                        status = ssh.wait() => {
                            let message = match status {
                                Ok(status) => format!("SSH connection exited with {status}"),
                                Err(error) => error.to_string(),
                            };
                            let _ = events.send(RuntimeEvent::Error(message));
                            had_error = true;
                            break;
                        }
                        line = helper_lines.next_line(), if helper_stderr_open => {
                            match line {
                                Ok(Some(line)) => {
                                    if !line.trim().is_empty() {
                                        last_helper_line = Some(line.clone());
                                    }
                                    let _ = events.send(RuntimeEvent::Log(format!("[helper] {line}")));
                                    if !helper_ready && line.contains("routing is active") {
                                        helper_ready = true;
                                        let _ = events.send(RuntimeEvent::Connected);
                                    }
                                }
                                Ok(None) => helper_stderr_open = false,
                                Err(error) => {
                                    let _ = events.send(RuntimeEvent::Error(format!("Failed to read helper output: {error}")));
                                    had_error = true;
                                    break;
                                }
                            }
                        }
                        _ = &mut startup_timeout, if !helper_ready => {
                            let _ = events.send(RuntimeEvent::Error(
                                last_helper_line.unwrap_or_else(|| "Timed out waiting for network routing".into()),
                            ));
                            had_error = true;
                            break;
                        }
                        _ = traffic_interval.tick() => {
                            if helper_ready {
                                if let Some((sent, received)) = read_ssh_traffic(&profile.host).await {
                                    let (upload, download) = previous_traffic
                                        .map(|(old_sent, old_received)| {
                                            (sent.saturating_sub(old_sent), received.saturating_sub(old_received))
                                        })
                                        .unwrap_or((0, 0));
                                    previous_traffic = Some((sent, received));
                                    let _ = events.send(RuntimeEvent::Speed { upload, download });
                                }
                            }
                        }
                    }
                }
                process.stdin.take();
                if process.id().is_some()
                    && tokio::time::timeout(Duration::from_secs(3), process.wait()).await.is_err()
                {
                    let _ = process.kill().await;
                }
                let _ = ssh.stop().await;
                let _ = events.send(RuntimeEvent::Speed { upload: 0, download: 0 });
                if !had_error {
                    let _ = events.send(RuntimeEvent::Disconnected);
                }
            });
        });
    }

    fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
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
    let config = Rc::new(RefCell::new(AppConfig::load().unwrap_or_default()));
    let controller = Rc::new(RefCell::new(RuntimeController::default()));
    let (event_tx, event_rx) = mpsc::channel::<RuntimeEvent>();
    let is_connected = Rc::new(RefCell::new(false));
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
        if !app_info.icon.is_empty() {
            let icon = if app_info.icon.starts_with('/') {
                gtk::Image::from_file(&app_info.icon)
            } else {
                gtk::Image::from_icon_name(&app_info.icon)
            };
            icon.set_pixel_size(28);
            row.add_prefix(&icon);
        }
        let executable = app_info.executable.clone();
        let config_ref = config.clone();
        row.connect_selected_notify(move |row| {
            let action = match row.selected() {
                1 => RuleAction::Proxy,
                2 => RuleAction::Block,
                _ => RuleAction::Direct,
            };
            set_app_action(&config_ref, &executable, action);
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

    let routing_page = adw::PreferencesPage::new();
    let routing_group = adw::PreferencesGroup::builder().title("Routing").build();
    let policy = adw::ComboRow::builder()
        .title("Default Policy")
        .model(&gtk::StringList::new(&["Proxy", "Direct", "Block"]))
        .selected(match config.borrow().settings.default_policy {
            RuleAction::Proxy => 0,
            RuleAction::Direct => 1,
            RuleAction::Block => 2,
        })
        .build();
    let ipv6 = adw::SwitchRow::builder().title("IPv6").active(config.borrow().settings.ipv6).build();
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
    let import_rules = gtk::Button::with_label("Import");
    import_rules.set_valign(gtk::Align::Center);
    import_rules.add_css_class("suggested-action");
    rule_source.add_suffix(&import_rules);
    let rule_status = adw::ActionRow::builder().title("Imported Rules").build();
    {
        let settings = &config.borrow().settings;
        let total = settings.imported_domain_rules.len() + settings.imported_ip_rules.len();
        let subtitle = if total == 0 { "None".to_string() } else { format!("{total} rules") };
        rule_status.set_subtitle(&subtitle);
    }
    routing_group.add(&policy);
    routing_group.add(&ipv6);
    routing_group.add(&rule_source);
    routing_group.add(&rule_status);
    routing_page.add(&routing_group);

    let custom_rules_group = adw::PreferencesGroup::builder().title("Custom Rules").build();
    let add_rule = gtk::Button::with_label("Add Rule");
    add_rule.add_css_class("suggested-action");
    add_rule.set_valign(gtk::Align::Center);
    custom_rules_group.set_header_suffix(Some(&add_rule));
    {
        let settings = &config.borrow().settings;
        for rule in &settings.domain_rules {
            add_domain_rule_row(
                &custom_rules_group,
                &config,
                rule.pattern.clone(),
                rule.kind,
                rule.action,
            );
        }
        for rule in &settings.ip_rules {
            add_ip_rule_row(&custom_rules_group, &config, rule.network.to_string(), rule.action);
        }
    }
    {
        let parent = window.clone();
        let config = config.clone();
        let custom_rules_group = custom_rules_group.clone();
        add_rule.connect_clicked(move |_| {
            let dialog = adw::AlertDialog::new(Some("Add Rule"), None);
            let group = adw::PreferencesGroup::new();
            let pattern = adw::EntryRow::builder().title("Domain, IP, or CIDR").build();
            let rule_type = adw::ComboRow::builder()
                .title("Rule Type")
                .model(&gtk::StringList::new(&[
                    "DOMAIN-SUFFIX",
                    "DOMAIN",
                    "DOMAIN-KEYWORD",
                    "IP-CIDR",
                ]))
                .build();
            let action = adw::ComboRow::builder()
                .title("Action")
                .model(&gtk::StringList::new(&["DIRECT", "PROXY", "REJECT"]))
                .selected(1)
                .build();
            group.add(&pattern);
            group.add(&rule_type);
            group.add(&action);
            dialog.set_extra_child(Some(&group));
            dialog.add_response("cancel", "Cancel");
            dialog.add_response("save", "Save");
            dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
            let config = config.clone();
            let custom_rules_group = custom_rules_group.clone();
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
                let parsed = parse_rule_set(&format!("{rule_type_text},{value}"), selected_action);
                if parsed.rule_count() != 1 {
                    dialog.set_body("The rule is invalid.");
                    return;
                }

                if let Some(rule) = parsed.domain_rules.into_iter().next() {
                    {
                        let mut current = config.borrow_mut();
                        current.settings.domain_rules.retain(|existing| {
                            !(existing.pattern == rule.pattern && existing.kind == rule.kind)
                        });
                        current.settings.domain_rules.push(rule.clone());
                        if current.save().is_err() {
                            dialog.set_body("Failed to save the rule.");
                            return;
                        }
                    }
                    add_domain_rule_row(
                        &custom_rules_group,
                        &config,
                        rule.pattern,
                        rule.kind,
                        rule.action,
                    );
                } else if let Some(rule) = parsed.ip_rules.into_iter().next() {
                    let network = rule.network.to_string();
                    {
                        let mut current = config.borrow_mut();
                        current.settings.ip_rules.retain(|existing| existing.network != rule.network);
                        current.settings.ip_rules.push(rule.clone());
                        if current.save().is_err() {
                            dialog.set_body("Failed to save the rule.");
                            return;
                        }
                    }
                    add_ip_rule_row(&custom_rules_group, &config, network, rule.action);
                }
            });
            dialog.present(Some(&parent));
        });
    }
    routing_page.add(&custom_rules_group);
    rules_stack.add_titled(&page_scroller(&routing_page), Some("routing"), "Domains & IPs");

    let blocked_page = adw::PreferencesPage::new();
    let blocked_group = adw::PreferencesGroup::builder().title("Blocked Applications").build();
    for app_info in scan_desktop_apps().into_iter().filter(|app_info| {
        current_app_action(&config.borrow(), &app_info.executable) == RuleAction::Block
    }) {
        let row = adw::ActionRow::builder().title(&app_info.name).subtitle(&app_info.executable).build();
        blocked_group.add(&row);
    }
    blocked_page.add(&blocked_group);
    rules_stack.add_titled(&page_scroller(&blocked_page), Some("blocked"), "Blocked");
    view_stack.add_named(&rules_page, Some("rules"));

    let traffic_page = adw::PreferencesPage::new();
    let traffic_group = adw::PreferencesGroup::builder().title("Current Session").build();
    let uploaded_row = adw::ActionRow::builder().title("Uploaded").subtitle("0 B").build();
    let downloaded_row = adw::ActionRow::builder().title("Downloaded").subtitle("0 B").build();
    let total_row = adw::ActionRow::builder().title("Total").subtitle("0 B").build();
    traffic_group.add(&uploaded_row);
    traffic_group.add(&downloaded_row);
    traffic_group.add(&total_row);
    traffic_page.add(&traffic_group);
    let traffic_rules_group = adw::PreferencesGroup::builder().title("Application Rules").build();
    let proxy_apps_row = adw::ActionRow::builder().title("PROXY").build();
    let direct_apps_row = adw::ActionRow::builder().title("DIRECT").build();
    let blocked_apps_row = adw::ActionRow::builder().title("REJECT").build();
    let app_count = scan_desktop_apps().len();
    let proxy_count = config.borrow().settings.app_rules.iter().filter(|rule| rule.action == RuleAction::Proxy).count();
    let block_count = config.borrow().settings.app_rules.iter().filter(|rule| rule.action == RuleAction::Block).count();
    proxy_apps_row.set_subtitle(&format!("{proxy_count} applications"));
    blocked_apps_row.set_subtitle(&format!("{block_count} applications"));
    direct_apps_row.set_subtitle(&format!("{} applications", app_count.saturating_sub(proxy_count + block_count)));
    traffic_rules_group.add(&proxy_apps_row);
    traffic_rules_group.add(&direct_apps_row);
    traffic_rules_group.add(&blocked_apps_row);
    traffic_page.add(&traffic_rules_group);
    view_stack.add_named(&page_scroller(&traffic_page), Some("traffic"));

    let log_page = adw::PreferencesPage::new();
    let log_group = adw::PreferencesGroup::builder().title("Proxy Logs").build();
    let log_actions = adw::ActionRow::builder().title("SSH and routing output").build();
    let copy_logs = gtk::Button::from_icon_name("edit-copy-symbolic");
    copy_logs.add_css_class("flat");
    copy_logs.set_tooltip_text(Some("Copy logs"));
    log_actions.add_suffix(&copy_logs);
    let clear_logs = gtk::Button::from_icon_name("edit-clear-all-symbolic");
    clear_logs.add_css_class("flat");
    clear_logs.set_tooltip_text(Some("Clear logs"));
    log_actions.add_suffix(&clear_logs);
    log_group.add(&log_actions);
    let log_view = gtk::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk::WrapMode::WordChar)
        .build();
    let log_buffer = log_view.buffer();
    let log_scroller = gtk::ScrolledWindow::builder()
        .child(&log_view)
        .min_content_height(180)
        .vexpand(true)
        .build();
    log_group.add(&log_scroller);
    log_page.add(&log_group);
    view_stack.add_named(&page_scroller(&log_page), Some("logs"));

    {
        let log_buffer = log_buffer.clone();
        clear_logs.connect_clicked(move |_| log_buffer.set_text(""));
    }
    {
        let log_buffer = log_buffer.clone();
        copy_logs.connect_clicked(move |_| {
            let text = log_buffer.text(&log_buffer.start_iter(), &log_buffer.end_iter(), false);
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
                        if *is_connected.borrow() && is_active {
                            bottom_status.set_text("Disconnecting…");
                            connect_button_ref.set_sensitive(false);
                            if let Some(tray) = tray_manager.borrow().as_ref() {
                                tray.set_state(TrayConnectionState::Disconnecting);
                            }
                            controller.borrow_mut().stop();
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
                        controller.borrow_mut().start(
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
        let log_buffer_ref = log_buffer.clone();
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
                quit_controller.borrow_mut().stop();
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
        let rule_status = rule_status.clone();
        import_rules.clone().connect_clicked(move |_| {
            let url = rule_source.text().trim().to_string();
            if url.is_empty() {
                rule_status.set_subtitle("Rule source URL is empty");
                return;
            }
            import_rules.set_sensitive(false);
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
        let log_buffer = log_buffer.clone();
        let log_view = log_view.clone();
        let bottom_status = bottom_status.clone();
        let speed_label = speed_label.clone();
        let uploaded_row = uploaded_row.clone();
        let downloaded_row = downloaded_row.clone();
        let total_row = total_row.clone();
        let session_upload = session_upload.clone();
        let session_download = session_download.clone();
        let tray_manager = tray_manager.clone();
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
            while let Ok(event) = event_rx.try_recv() {
                match event {
                    RuntimeEvent::Connected => {
                        *session_upload.borrow_mut() = 0;
                        *session_download.borrow_mut() = 0;
                        uploaded_row.set_subtitle("0 B");
                        downloaded_row.set_subtitle("0 B");
                        total_row.set_subtitle("0 B");
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
                    RuntimeEvent::Error(error) => {
                        *is_connected.borrow_mut() = false;
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
                    }
                    RuntimeEvent::Log(line) => {
                        let mut end = log_buffer.end_iter();
                        log_buffer.insert(&mut end, &format!("{line}\n"));
                        let end = log_buffer.end_iter();
                        let mark = log_buffer.create_mark(None, &end, false);
                        log_view.scroll_mark_onscreen(&mark);
                    }
                    RuntimeEvent::Speed { upload, download } => {
                        *session_upload.borrow_mut() += upload;
                        *session_download.borrow_mut() += download;
                        let up_total = *session_upload.borrow();
                        let down_total = *session_download.borrow();
                        uploaded_row.set_subtitle(&format_bytes(up_total));
                        downloaded_row.set_subtitle(&format_bytes(down_total));
                        total_row.set_subtitle(&format_bytes(up_total + down_total));
                        speed_label.set_text(&format!(
                            "↑ {} ({})   ↓ {} ({})",
                            format_speed(upload), format_bytes(up_total),
                            format_speed(download), format_bytes(down_total)
                        ));
                    }
                    RuntimeEvent::RuleImportFailed(error) => {
                        rule_status.set_subtitle(&format!("Import failed: {error}"));
                        import_rules.set_sensitive(true);
                    }
                    RuntimeEvent::RulesImported { result, source_url } => {
                        let (direct, proxy, block) = result.action_counts();
                        let total = result.rule_count();
                        {
                            let mut current = config.borrow_mut();
                            current.settings.imported_domain_rules = result.domain_rules;
                            current.settings.imported_ip_rules = result.ip_rules;
                            current.settings.default_policy = result.default_policy;
                            current.settings.rule_source_url = source_url;
                            current.settings.rule_source_name = "Shadowrocket Rule Source".into();
                            current.settings.rule_source_updated_at = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .map_or(0, |duration| duration.as_secs() as i64);
                            if let Err(error) = current.save() {
                                rule_status.set_subtitle(&format!("Config error: {error}"));
                                import_rules.set_sensitive(true);
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
                        let mut end = log_buffer.end_iter();
                        log_buffer.insert(
                            &mut end,
                            &format!("[rules] imported {total} rules ({direct} direct, {proxy} proxy, {block} block)\n"),
                        );
                        for warning in result.warnings.iter().take(3) {
                            let mut end = log_buffer.end_iter();
                            log_buffer.insert(&mut end, &format!("[rules] warning: {warning}\n"));
                        }
                        import_rules.set_sensitive(true);
                    }
                }
            }
            gtk::glib::ControlFlow::Continue
        });
    }

    window.present();
}
