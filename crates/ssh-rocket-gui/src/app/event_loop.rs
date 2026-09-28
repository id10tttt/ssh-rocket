use adw::prelude::*;
use gtk4::{self as gtk, prelude::*};
use libadwaita as adw;
use ssh_rocket_core::{AppConfig, RuleAction};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::mpsc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    controller::{RuntimeController, RuntimeEvent},
    rule_manager::rule_source_name,
    sys_monitor::SystemMonitor,
    tray::{TrayConnectionState, TrayManager},
    ui::{
        dialogs::{RefreshConnections, RefreshRules},
        floating_widget::FloatingWidget,
        logs_view::LogsView,
        rules_view::RulesView,
        traffic_view::TrafficView,
        widgets::{format_bytes, format_duration, format_speed},
        window::MainWindowWidgets,
    },
    ActiveConnectionStat, AppTrafficStat,
};

pub fn setup_runtime_event_loop(
    event_rx: mpsc::Receiver<RuntimeEvent>,
    file_log_tx: mpsc::Sender<String>,
    win: &MainWindowWidgets,
    rules_view: &RulesView,
    traffic_view: &TrafficView,
    logs_view: &LogsView,
    config: &Rc<RefCell<AppConfig>>,
    controller: &Rc<RefCell<RuntimeController>>,
    is_connected: &Rc<RefCell<bool>>,
    connect_start_time: &Rc<RefCell<Option<std::time::Instant>>>,
    refresh_connections: &RefreshConnections,
    refresh_rule_views: &RefreshRules,
    tray_manager: &Rc<RefCell<Option<Rc<TrayManager>>>>,
    floating_widget: &Rc<RefCell<Option<FloatingWidget>>>,
    sys_monitor: &Rc<RefCell<SystemMonitor>>,
    latest_proxy_speed: &Rc<Cell<(u64, u64)>>,
    app_traffic_data: &Rc<RefCell<Vec<AppTrafficStat>>>,
    active_conns_data: &Rc<RefCell<Vec<ActiveConnectionStat>>>,
    refresh_app_traffic: &Rc<dyn Fn()>,
    refresh_conns: &Rc<dyn Fn()>,
) {
    let session_upload = Rc::new(RefCell::new(0_u64));
    let session_download = Rc::new(RefCell::new(0_u64));
    let session_started = Rc::new(RefCell::new(String::from("—")));
    let session_duration = Rc::new(RefCell::new(String::from("00:00:00")));

    let update_session_status = {
        let traffic_view = traffic_view.clone();
        let session_started = session_started.clone();
        let session_duration = session_duration.clone();
        let is_connected = is_connected.clone();
        Rc::new(move || {
            if *is_connected.borrow() {
                traffic_view.update_session_subtitle(
                    &session_duration.borrow(),
                    &session_started.borrow(),
                );
            } else {
                traffic_view.update_session_subtitle("00:00:00", "—");
            }
        })
    };

    let total_hero_label = traffic_view.total_hero_label.clone();
    let total_up_label = traffic_view.total_up_label.clone();
    let total_down_label = traffic_view.total_down_label.clone();
    let proxy_hero_label = traffic_view.proxy_hero_label.clone();
    let proxy_up_label = traffic_view.proxy_up_label.clone();
    let proxy_down_label = traffic_view.proxy_down_label.clone();
    let direct_hero_label = traffic_view.direct_hero_label.clone();
    let direct_up_label = traffic_view.direct_up_label.clone();
    let direct_down_label = traffic_view.direct_down_label.clone();

    let update_traffic_labels = {
        let session_upload = session_upload.clone();
        let session_download = session_download.clone();
        let app_traffic_data = app_traffic_data.clone();
        let total_hero_label = total_hero_label.clone();
        let total_up_label = total_up_label.clone();
        let total_down_label = total_down_label.clone();
        let proxy_hero_label = proxy_hero_label.clone();
        let proxy_up_label = proxy_up_label.clone();
        let proxy_down_label = proxy_down_label.clone();
        let direct_hero_label = direct_hero_label.clone();
        let direct_up_label = direct_up_label.clone();
        let direct_down_label = direct_down_label.clone();
        Rc::new(move || {
            let up_proxy = *session_upload.borrow();
            let down_proxy = *session_download.borrow();
            let (apps_up, apps_down) =
                app_traffic_data
                    .borrow()
                    .iter()
                    .fold((0u64, 0u64), |(u, d), app| {
                        (u + app.upload, d + app.download)
                    });
            let total_up = up_proxy.max(apps_up);
            let total_down = down_proxy.max(apps_down);
            let direct_up = total_up.saturating_sub(up_proxy);
            let direct_down = total_down.saturating_sub(down_proxy);

            total_hero_label.set_text(&format_bytes(total_up + total_down));
            total_up_label.set_text(&format_bytes(total_up));
            total_down_label.set_text(&format_bytes(total_down));

            proxy_hero_label.set_text(&format_bytes(up_proxy + down_proxy));
            proxy_up_label.set_text(&format_bytes(up_proxy));
            proxy_down_label.set_text(&format_bytes(down_proxy));

            direct_hero_label.set_text(&format_bytes(direct_up + direct_down));
            direct_up_label.set_text(&format_bytes(direct_up));
            direct_down_label.set_text(&format_bytes(direct_down));
        })
    };

    let is_connected = is_connected.clone();
    let config = config.clone();
    let policy = rules_view.policy_row.clone();
    let rule_status = rules_view.rule_status_row.clone();
    let import_rules = rules_view.import_trigger_btn.clone();
    let import_button = rules_view.import_button.clone();
    let refresh_rule_views = refresh_rule_views.clone();
    let refresh_connections = refresh_connections.clone();
    let proxy_log_buffer = logs_view.proxy_log_buffer.clone();
    let proxy_log_view = logs_view.proxy_log_view.clone();
    let direct_log_buffer = logs_view.direct_log_buffer.clone();
    let direct_log_view = logs_view.direct_log_view.clone();
    let system_log_buffer = logs_view.system_log_buffer.clone();
    let system_log_view = logs_view.system_log_view.clone();
    let all_log_buffer = logs_view.all_log_buffer.clone();
    let all_log_view = logs_view.all_log_view.clone();
    let bottom_status_dot = win.bottom_status_dot.clone();
    let bottom_status = win.bottom_status.clone();
    let speed_label = win.speed_label.clone();
    let view_stack = win.view_stack.clone();
    let connect_start_time = connect_start_time.clone();
    let tray_manager = tray_manager.clone();
    let controller_ref = controller.clone();
    let app_traffic_data = app_traffic_data.clone();
    let active_conns_data = active_conns_data.clone();
    let refresh_app_traffic = refresh_app_traffic.clone();
    let refresh_conns = refresh_conns.clone();
    let traffic_view = traffic_view.clone();

    let sys_monitor = sys_monitor.clone();
    let latest_proxy_speed = latest_proxy_speed.clone();
    let floating_widget = floating_widget.clone();
    let mut sys_sample_counter = 0u32;

    gtk::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
        let mut event_count = 0usize;
        let loop_start = std::time::Instant::now();
        while let Ok(event) = event_rx.try_recv() {
            event_count += 1;
            match event {
                RuntimeEvent::Connected => {
                    latest_proxy_speed.set((0, 0));
                    *session_upload.borrow_mut() = 0;
                    *session_download.borrow_mut() = 0;
                    traffic_view.speed_history.borrow_mut().clear();
                    traffic_view.speed_drawing_area.queue_draw();
                    traffic_view.speed_current_label.set_text("↓ 0 B/s   ↑ 0 B/s");
                    app_traffic_data.borrow_mut().clear();
                    active_conns_data.borrow_mut().clear();
                    if view_stack.visible_child_name().as_deref() == Some("traffic") {
                        match traffic_view.stack.visible_child_name().as_deref() {
                            Some("apps") => refresh_app_traffic(),
                            Some("connections") => refresh_conns(),
                            _ => {}
                        }
                    }
                    update_traffic_labels();
                    *connect_start_time.borrow_mut() = Some(std::time::Instant::now());
                    if let Ok(now) = gtk::glib::DateTime::now_local() {
                        *session_started.borrow_mut() = now
                            .format("%Y-%m-%d %H:%M:%S")
                            .map_or_else(|_| "—".into(), |s| s.to_string());
                    }
                    *session_duration.borrow_mut() = "00:00:00".into();
                    *is_connected.borrow_mut() = true;
                    update_session_status();

                    bottom_status_dot.remove_css_class("status-dot-disconnected");
                    bottom_status_dot.remove_css_class("status-dot-connecting");
                    bottom_status_dot.add_css_class("status-dot-connected");
                    bottom_status.set_text(crate::i18n::tr("status.connected"));

                    if let Some(tray) = tray_manager.borrow().as_ref() {
                        tray.set_state(TrayConnectionState::Connected);
                    }
                    if let Some(refresh) = refresh_connections.borrow().as_ref() {
                        refresh();
                    }
                }
                RuntimeEvent::Disconnected => {
                    latest_proxy_speed.set((0, 0));
                    *is_connected.borrow_mut() = false;
                    *connect_start_time.borrow_mut() = None;
                    *session_started.borrow_mut() = "—".into();
                    *session_duration.borrow_mut() = "00:00:00".into();
                    update_session_status();

                    bottom_status_dot.remove_css_class("status-dot-connected");
                    bottom_status_dot.remove_css_class("status-dot-connecting");
                    bottom_status_dot.add_css_class("status-dot-disconnected");
                    bottom_status.set_text(crate::i18n::tr("status.disconnected"));

                    if let Some(tray) = tray_manager.borrow().as_ref() {
                        tray.set_state(TrayConnectionState::Disconnected);
                    }
                    speed_label.set_text("↑ 0 B/s   ↓ 0 B/s");
                    if let Some(refresh) = refresh_connections.borrow().as_ref() {
                        refresh();
                    }
                }
                RuntimeEvent::Status(status) => {
                    bottom_status_dot.remove_css_class("status-dot-connected");
                    bottom_status_dot.remove_css_class("status-dot-disconnected");
                    bottom_status_dot.add_css_class("status-dot-connecting");
                    bottom_status.set_text(&status);

                    if let Some(tray) = tray_manager.borrow().as_ref() {
                        tray.set_state(TrayConnectionState::Connecting);
                    }
                    if let Some(refresh) = refresh_connections.borrow().as_ref() {
                        refresh();
                    }
                }
                RuntimeEvent::Error(error) => {
                    latest_proxy_speed.set((0, 0));
                    *is_connected.borrow_mut() = false;
                    *connect_start_time.borrow_mut() = None;
                    *session_started.borrow_mut() = "—".into();
                    *session_duration.borrow_mut() = "00:00:00".into();
                    update_session_status();

                    bottom_status_dot.remove_css_class("status-dot-connected");
                    bottom_status_dot.remove_css_class("status-dot-connecting");
                    bottom_status_dot.add_css_class("status-dot-disconnected");
                    bottom_status.set_text(&error);

                    if let Some(tray) = tray_manager.borrow().as_ref() {
                        tray.set_state(TrayConnectionState::Disconnected);
                    }
                    speed_label.set_text("↑ 0 B/s   ↓ 0 B/s");
                    if let Some(refresh) = refresh_connections.borrow().as_ref() {
                        refresh();
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
                    let _ = file_log_tx.send(formatted.clone());

                    const MAX_LOG_LINES: i32 = 1000;
                    let is_logs_visible = view_stack.visible_child_name().as_deref() == Some("logs");

                    let append_to = |buffer: &gtk::TextBuffer, view: &gtk::TextView| {
                        let mut end = buffer.end_iter();
                        buffer.insert(&mut end, &formatted);

                        let line_count = buffer.line_count();
                        if line_count > MAX_LOG_LINES {
                            let mut prune_end = buffer.start_iter();
                            prune_end.forward_lines(line_count - MAX_LOG_LINES);
                            let mut prune_start = buffer.start_iter();
                            buffer.delete(&mut prune_start, &mut prune_end);
                        }

                        if is_logs_visible {
                            let end_iter = buffer.end_iter();
                            let mark = buffer.create_mark(None, &end_iter, false);
                            view.scroll_mark_onscreen(&mark);
                            buffer.delete_mark(&mark);
                        }
                    };

                    append_to(&all_log_buffer, &all_log_view);

                    let is_proxy = line.contains("[proxy]") || line.contains("-> Proxy");
                    let is_direct = line.contains("[direct]")
                        || line.contains("-> Direct")
                        || line.contains("Direct (");
                    if is_proxy {
                        append_to(&proxy_log_buffer, &proxy_log_view);
                    } else if is_direct {
                        append_to(&direct_log_buffer, &direct_log_view);
                    } else {
                        append_to(&system_log_buffer, &system_log_view);
                    }
                }
                RuntimeEvent::Speed { upload, download } => {
                    latest_proxy_speed.set((upload, download));
                    *session_upload.borrow_mut() += upload;
                    *session_download.borrow_mut() += download;
                    let up_total = *session_upload.borrow();
                    let down_total = *session_download.borrow();

                    {
                        let mut hist = traffic_view.speed_history.borrow_mut();
                        if hist.len() >= 60 {
                            hist.pop_front();
                        }
                        hist.push_back((std::time::Instant::now(), upload, download));
                    }
                    traffic_view.speed_drawing_area.queue_draw();
                    traffic_view.speed_current_label.set_text(&format!(
                        "↓ {}   ↑ {}",
                        format_speed(download),
                        format_speed(upload)
                    ));

                    update_traffic_labels();
                    speed_label.set_text(&format!(
                        "↑ {} ({})   ↓ {} ({})",
                        format_speed(upload),
                        format_bytes(up_total),
                        format_speed(download),
                        format_bytes(down_total)
                    ));
                }
                RuntimeEvent::AppTraffic { stats, conns } => {
                    *app_traffic_data.borrow_mut() = stats;
                    *active_conns_data.borrow_mut() = conns;
                    if view_stack.visible_child_name().as_deref() == Some("traffic") {
                        match traffic_view.stack.visible_child_name().as_deref() {
                            Some("apps") => refresh_app_traffic(),
                            Some("connections") => refresh_conns(),
                            _ => {}
                        }
                    }
                    update_traffic_labels();
                }
                RuntimeEvent::RuleImportFailed(error) => {
                    rule_status.set_subtitle(&format!("导入失败: {error}"));
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
                            rule_status.set_subtitle(&format!("配置保存失败: {error}"));
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
                        "共 {total} 条规则 · 直连 {direct} · 代理 {proxy} · 拦截 {block} · 跳过 {}",
                        result.ignored_count
                    ));
                    let mut end = proxy_log_buffer.end_iter();
                    proxy_log_buffer.insert(
                        &mut end,
                        &format!(
                            "[rules] 成功导入 {total} 条规则 (直连 {direct}, 代理 {proxy}, 拦截 {block})\n"
                        ),
                    );
                    for warning in result.warnings.iter().take(3) {
                        let mut end = proxy_log_buffer.end_iter();
                        proxy_log_buffer.insert(&mut end, &format!("[rules] 警告: {warning}\n"));
                    }
                    import_rules.set_sensitive(true);
                    import_button.set_sensitive(true);
                    if let Some(refresh) = refresh_rule_views.borrow().as_ref() {
                        refresh();
                    }
                    controller_ref.borrow().sync_rules();
                }
            }

            if event_count >= 30 || loop_start.elapsed() >= std::time::Duration::from_millis(15) {
                break;
            }
        }
        if let Some(start) = *connect_start_time.borrow() {
            let elapsed = start.elapsed().as_secs();
            *session_duration.borrow_mut() = format_duration(elapsed);
            update_session_status();
        }

        sys_sample_counter += 1;
        if sys_sample_counter >= 5 {
            sys_sample_counter = 0;
            let metrics = sys_monitor.borrow_mut().sample();
            let (p_up, p_down) = latest_proxy_speed.get();
            let is_conn = *is_connected.borrow();
            let actual_p_up = if is_conn { p_up } else { 0 };
            let actual_p_down = if is_conn { p_down } else { 0 };
            let d_up = metrics.sys_upload_speed.saturating_sub(actual_p_up);
            let d_down = metrics.sys_download_speed.saturating_sub(actual_p_down);
            if let Some(hud) = floating_widget.borrow().as_ref() {
                hud.update_stats(is_conn, actual_p_up, actual_p_down, d_up, d_down, &metrics);
            }
        }

        gtk::glib::ControlFlow::Continue
    });
}
