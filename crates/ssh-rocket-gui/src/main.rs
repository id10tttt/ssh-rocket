use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use ssh_rocket_core::{AppConfig, Profile, RuleAction};
use ssh_rocket_runtime::SshSession;
use std::{cell::RefCell, path::PathBuf, process::Stdio, rc::Rc, sync::mpsc, thread, time::Duration};
use tokio::{process::Command, sync::oneshot};

const APP_ID: &str = "io.github.idi0t.SshRocket";
const SOCKS_PORT: u16 = 17880;

enum RuntimeEvent {
    Connected,
    Disconnected,
    Error(String),
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

                let _ = events.send(RuntimeEvent::Connected);
                tokio::select! {
                    _ = stop_rx => {
                        process.stdin.take();
                        if tokio::time::timeout(Duration::from_secs(3), process.wait()).await.is_err() {
                            let _ = process.kill().await;
                        }
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
                    }
                }
                let _ = ssh.stop().await;
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
        .default_width(760)
        .default_height(640)
        .build();

    let header = adw::HeaderBar::new();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);

    let page = adw::PreferencesPage::new();
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
    page.add(&connection_group);

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
    routing_group.add(&policy);
    routing_group.add(&ipv6);
    page.add(&routing_group);

    let runtime_group = adw::PreferencesGroup::builder().title("Runtime").build();
    let status = adw::ActionRow::builder().title("Status").subtitle("Disconnected").build();
    let connect = gtk::Button::with_label("Connect");
    connect.add_css_class("suggested-action");
    connect.set_valign(gtk::Align::Center);
    status.add_suffix(&connect);
    runtime_group.add(&status);
    page.add(&runtime_group);

    let scroller = gtk::ScrolledWindow::builder().child(&page).vexpand(true).build();
    toolbar.set_content(Some(&scroller));
    window.set_content(Some(&toolbar));

    let is_connected = Rc::new(RefCell::new(false));
    {
        let config = config.clone();
        let controller = controller.clone();
        let event_tx = event_tx.clone();
        let is_connected = is_connected.clone();
        let status = status.clone();
        let connect = connect.clone();
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
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
            while let Ok(event) = event_rx.try_recv() {
                match event {
                    RuntimeEvent::Connected => {
                        *is_connected.borrow_mut() = true;
                        status.set_subtitle("Connected");
                        connect.set_label("Disconnect");
                        connect.set_sensitive(true);
                    }
                    RuntimeEvent::Disconnected => {
                        *is_connected.borrow_mut() = false;
                        status.set_subtitle("Disconnected");
                        connect.set_label("Connect");
                        connect.set_sensitive(true);
                    }
                    RuntimeEvent::Error(error) => {
                        *is_connected.borrow_mut() = false;
                        status.set_subtitle(&error);
                        connect.set_label("Connect");
                        connect.set_sensitive(true);
                    }
                }
            }
            gtk::glib::ControlFlow::Continue
        });
    }

    window.present();
}
