use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use ssh_rocket_core::{AppConfig, Profile, RuleAction, RuleImportResult, parse_rule_set, parse_shadowrocket_rules};
use ssh_rocket_runtime::SshSession;
use std::{cell::RefCell, path::PathBuf, process::{Command as StdCommand, Stdio}, rc::Rc, sync::mpsc, thread, time::{Duration, SystemTime, UNIX_EPOCH}};
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
                let mut process = match Command::new("pkexec")
                    .arg(helper)
                    .arg("run")
                    .arg(config_path)
                    .arg(uid.to_string())
                    .arg(ssh.socks_port.to_string())
                    .arg(ssh.dns_port.to_string())
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
                if let Some(stderr) = process.stderr.take() {
                    spawn_log_reader(stderr, "helper", events.clone());
                }

                let _ = events.send(RuntimeEvent::Connected);
                let mut stop_rx = stop_rx;
                let mut traffic_interval = tokio::time::interval(Duration::from_secs(1));
                let mut previous_traffic = None;
                loop {
                    tokio::select! {
                        _ = &mut stop_rx => {
                            process.stdin.take();
                            if tokio::time::timeout(Duration::from_secs(3), process.wait()).await.is_err() {
                                let _ = process.kill().await;
                            }
                            break;
                        }
                        status = process.wait() => {
                            match status {
                                Ok(status) if status.success() => {},
                                Ok(status) => {
                                    let _ = events.send(RuntimeEvent::Error(format!("Helper exited with {status}")));
                                }
                                Err(error) => {
                                    let _ = events.send(RuntimeEvent::Error(format!("Helper failed: {error}")));
                                }
                            }
                            break;
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
                        }
                    }
                }
                let _ = ssh.stop().await;
                let _ = events.send(RuntimeEvent::Speed { upload: 0, download: 0 });
                let _ = events.send(RuntimeEvent::Disconnected);
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
    if config.borrow().profiles.is_empty() {
        let profile = Profile::default();
        let id = profile.id;
        let mut config = config.borrow_mut();
        config.profiles.push(profile);
        config.active_profile = Some(id);
    }

    let active_profile = config.borrow().active_profile().cloned().unwrap_or_default();
    let controller = Rc::new(RefCell::new(RuntimeController::default()));
    let (event_tx, event_rx) = mpsc::channel::<RuntimeEvent>();

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
    let connect_nav = navigation_row("network-server-symbolic", "Connect");
    let routing_nav = navigation_row("preferences-system-network-symbolic", "Routing");
    let logs_nav = navigation_row("utilities-terminal-symbolic", "Logs");
    navigation.append(&connect_nav);
    navigation.append(&routing_nav);
    navigation.append(&logs_nav);
    sidebar.append(&navigation);
    root.append(&sidebar);
    root.append(&gtk::Separator::new(gtk::Orientation::Vertical));

    let header = adw::HeaderBar::new();
    let page_title = gtk::Label::new(Some("Connect"));
    page_title.add_css_class("title-3");
    header.set_title_widget(Some(&page_title));
    let toolbar = adw::ToolbarView::new();
    toolbar.set_hexpand(true);
    toolbar.add_top_bar(&header);

    let view_stack = gtk::Stack::new();
    view_stack.set_hexpand(true);
    view_stack.set_vexpand(true);

    let connect_page = adw::PreferencesPage::new();
    let connection_group = adw::PreferencesGroup::builder().title("SSH Connection").build();
    let host = adw::EntryRow::builder().title("Host").text(&active_profile.host).build();
    let port = adw::EntryRow::builder().title("Port").text(active_profile.port.to_string()).build();
    let username = adw::EntryRow::builder().title("Username").text(&active_profile.username).build();
    let identity = adw::EntryRow::builder()
        .title("Identity File")
        .text(active_profile.identity_file.as_ref().map(|path| path.to_string_lossy()).unwrap_or_default())
        .build();
    connection_group.add(&host);
    connection_group.add(&port);
    connection_group.add(&username);
    connection_group.add(&identity);
    connect_page.add(&connection_group);

    let runtime_group = adw::PreferencesGroup::builder().title("Runtime").build();
    let status = adw::ActionRow::builder().title("Status").subtitle("Disconnected").build();
    let connect = gtk::Button::with_label("Connect");
    connect.add_css_class("suggested-action");
    connect.set_valign(gtk::Align::Center);
    status.add_suffix(&connect);
    runtime_group.add(&status);
    connect_page.add(&runtime_group);
    view_stack.add_named(&page_scroller(&connect_page), Some("connect"));

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
    view_stack.add_named(&page_scroller(&routing_page), Some("routing"));

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
        navigation.connect_row_selected(move |_, row| {
            let Some(row) = row else { return; };
            let (name, title) = match row.index() {
                1 => ("routing", "Routing"),
                2 => ("logs", "Logs"),
                _ => ("connect", "Connect"),
            };
            view_stack.set_visible_child_name(name);
            page_title.set_text(title);
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

    let is_connected = Rc::new(RefCell::new(false));
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
        let config = config.clone();
        let controller = controller.clone();
        let event_tx = event_tx.clone();
        let is_connected = is_connected.clone();
        let status = status.clone();
        let connect = connect.clone();
        let policy = policy.clone();
        connect.clone().connect_clicked(move |_| {
            if *is_connected.borrow() {
                status.set_subtitle("Disconnecting…");
                controller.borrow_mut().stop();
                return;
            }

            let port_number = port.text().parse::<u16>().unwrap_or(22);
            let identity_file = (!identity.text().is_empty()).then(|| PathBuf::from(identity.text().as_str()));
            let mut profile = config.borrow().active_profile().cloned().unwrap_or_default();
            profile.host = host.text().to_string();
            profile.port = port_number;
            profile.username = username.text().to_string();
            profile.identity_file = identity_file;

            {
                let mut current = config.borrow_mut();
                if let Some(existing) = current.profiles.iter_mut().find(|item| item.id == profile.id) {
                    *existing = profile.clone();
                }
                current.settings.default_policy = match policy.selected() {
                    1 => RuleAction::Direct,
                    2 => RuleAction::Block,
                    _ => RuleAction::Proxy,
                };
                current.settings.ipv6 = ipv6.is_active();
                if let Err(error) = current.save() {
                    status.set_subtitle(&format!("Config error: {error}"));
                    return;
                }
            }

            let Ok(config_path) = AppConfig::path() else {
                status.set_subtitle("Cannot determine config path");
                return;
            };
            status.set_subtitle("Connecting…");
            connect.set_sensitive(false);
            controller.borrow_mut().start(profile, config.borrow().clone(), config_path, event_tx.clone());
        });
    }

    {
        let is_connected = is_connected.clone();
        let connect = connect.clone();
        let status = status.clone();
        let config = config.clone();
        let policy = policy.clone();
        let rule_status = rule_status.clone();
        let import_rules = import_rules.clone();
        let log_buffer = log_buffer.clone();
        let log_view = log_view.clone();
        let bottom_status = bottom_status.clone();
        let speed_label = speed_label.clone();
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
            while let Ok(event) = event_rx.try_recv() {
                match event {
                    RuntimeEvent::Connected => {
                        *is_connected.borrow_mut() = true;
                        status.set_subtitle("Connected");
                        bottom_status.set_text("Connected");
                        connect.set_label("Disconnect");
                        connect.set_sensitive(true);
                    }
                    RuntimeEvent::Disconnected => {
                        *is_connected.borrow_mut() = false;
                        status.set_subtitle("Disconnected");
                        bottom_status.set_text("Disconnected");
                        speed_label.set_text("↑ 0 B/s   ↓ 0 B/s");
                        connect.set_label("Connect");
                        connect.set_sensitive(true);
                    }
                    RuntimeEvent::Error(error) => {
                        *is_connected.borrow_mut() = false;
                        status.set_subtitle(&error);
                        bottom_status.set_text("Disconnected");
                        speed_label.set_text("↑ 0 B/s   ↓ 0 B/s");
                        connect.set_label("Connect");
                        connect.set_sensitive(true);
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
                        speed_label.set_text(&format!(
                            "↑ {}   ↓ {}",
                            format_speed(upload),
                            format_speed(download)
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
