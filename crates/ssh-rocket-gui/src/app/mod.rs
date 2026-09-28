pub mod event_loop;
pub mod rules;

use adw::prelude::*;
use gtk4::{self as gtk, gio, glib};
use libadwaita as adw;
use ssh_rocket_core::{AppConfig, Language, RuleAction, ThemeMode};
use std::{
    cell::{Cell, RefCell},
    fs,
    rc::Rc,
    sync::mpsc,
    thread,
};

use crate::{
    app_scanner::{current_app_action, scan_desktop_apps, set_app_action},
    controller::{RuntimeController, RuntimeEvent},
    rule_manager::import_rule_source,
    sys_monitor::SystemMonitor,
    tray::TrayManager,
    ui::{
        connect_view::{render_connection_cards, ConnectView},
        dialogs::{
            show_forward_dialog, show_profile_dialog, RefreshConnections, RefreshForwards,
            RefreshRules,
        },
        floating_widget::FloatingWidget,
        forward_view::{render_forward_cards, ForwardView},
        logs_view::LogsView,
        rules_view::RulesView,
        settings_view::SettingsView,
        theme::init_theme,
        traffic_view::{
            refresh_app_traffic_list, refresh_connection_list, refresh_traffic_rule_counts,
            TrafficView,
        },
        widgets::create_app_icon,
        window::create_main_window,
    },
    ActiveConnectionStat, AppTrafficStat,
};
use ssh_rocket_runtime::ForwardManager;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

pub fn apply_theme_mode(mode: ThemeMode) {
    let style_manager = adw::StyleManager::default();
    match mode {
        ThemeMode::Auto => style_manager.set_color_scheme(adw::ColorScheme::Default),
        ThemeMode::Light => style_manager.set_color_scheme(adw::ColorScheme::ForceLight),
        ThemeMode::Dark => style_manager.set_color_scheme(adw::ColorScheme::ForceDark),
    }
}

pub fn build_ui(app: &adw::Application) {
    let config = Rc::new(RefCell::new(AppConfig::load().unwrap_or_default()));
    let initial_theme = config.borrow().settings.theme_mode;
    let initial_lang = config.borrow().settings.language;
    crate::i18n::set_language(initial_lang);
    apply_theme_mode(initial_theme);

    init_theme();

    let controller = Rc::new(RefCell::new(RuntimeController::default()));
    let (event_tx, event_rx) = mpsc::channel::<RuntimeEvent>();
    let (file_log_tx, file_log_rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let Ok(log_path) = AppConfig::log_file_path() else {
            return;
        };
        if let Some(parent) = log_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let file = match fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            Ok(f) => f,
            Err(_) => return,
        };
        let mut writer = std::io::BufWriter::new(file);
        use std::io::Write;
        while let Ok(line) = file_log_rx.recv() {
            let _ = writer.write_all(line.as_bytes());
            let _ = writer.flush();
        }
    });
    let is_connected = Rc::new(RefCell::new(false));
    let connect_start_time = Rc::new(RefCell::new(None::<std::time::Instant>));
    let connection_buttons = Rc::new(RefCell::new(Vec::<(String, gtk::Button)>::new()));
    let refresh_connections: RefreshConnections = Rc::new(RefCell::new(None));
    let refresh_forwards: RefreshForwards = Rc::new(RefCell::new(None));
    let forward_manager = Arc::new(TokioMutex::new(ForwardManager::new()));
    let tray_manager = Rc::new(RefCell::new(None::<Rc<TrayManager>>));
    let quitting = Rc::new(RefCell::new(false));

    let floating_widget: Rc<RefCell<Option<FloatingWidget>>> = Rc::new(RefCell::new(None));
    let sys_monitor = Rc::new(RefCell::new(SystemMonitor::new()));
    let latest_proxy_speed = Rc::new(Cell::new((0u64, 0u64)));

    let on_floating_changed = {
        let config = config.clone();
        let floating_widget = floating_widget.clone();
        Rc::new(move || {
            let cfg = config.borrow().settings.floating_widget.clone();
            if let Some(hud) = floating_widget.borrow().as_ref() {
                hud.set_shown(cfg.enabled);
                hud.update_config(cfg.idle_opacity, cfg.fade_delay_secs, cfg.speed_decimals);
            }
        })
    };

    // 1. 构建主窗口与导航
    let win = create_main_window(app);

    let refresh_ui_for_language: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let on_lang_changed = {
        let refresh_ui = refresh_ui_for_language.clone();
        Rc::new(move |lang: Language| {
            crate::i18n::set_language(lang);
            let refresh_ui = refresh_ui.clone();
            glib::idle_add_local_once(move || {
                if let Some(refresh) = refresh_ui.borrow().as_ref() {
                    refresh();
                }
            });
        })
    };
    let on_theme_changed = Rc::new(move |mode| {
        apply_theme_mode(mode);
    });

    // 2. 构建各功能视图
    let connect_view = ConnectView::new();
    let rules_view = RulesView::new(&config);
    let traffic_view = TrafficView::new();
    let logs_view = LogsView::new();
    let forward_view = ForwardView::new();
    let settings_view = Rc::new(SettingsView::new(
        &config,
        on_theme_changed,
        on_lang_changed,
        on_floating_changed,
    ));

    win.view_stack
        .add_named(&connect_view.container, Some("connect"));
    win.view_stack.add_named(&rules_view.container, Some("rules"));
    win.view_stack.add_named(&traffic_view.page, Some("traffic"));
    win.view_stack.add_named(&logs_view.container, Some("logs"));
    win.view_stack.add_named(&forward_view.container, Some("forward"));
    win.view_stack.add_named(&settings_view.container, Some("settings"));

    let refresh_active_traffic_tab: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));

    {
        let win_connect_lbl = win.connect_nav_label.clone();
        let win_rules_lbl = win.rules_nav_label.clone();
        let win_traffic_lbl = win.traffic_nav_label.clone();
        let win_logs_lbl = win.logs_nav_label.clone();
        let win_forward_lbl = win.forward_nav_label.clone();
        let win_settings_lbl = win.settings_nav_label.clone();
        let win_back_btn = win.back_button.clone();
        let win_add_conn = win.add_connection.clone();
        let win_page_title = win.page_title.clone();
        let win_navigation = win.navigation.clone();
        let win_bottom_status = win.bottom_status.clone();
        let is_connected_clone = is_connected.clone();
        let connect_view_clone = connect_view.clone();
        let rules_view_clone = rules_view.clone();
        let traffic_view_clone = traffic_view.clone();
        let logs_view_clone = logs_view.clone();
        let forward_view_clone = forward_view.clone();
        let settings_view_clone = settings_view.clone();
        let refresh_conn_ref = refresh_connections.clone();
        let refresh_forward_ref = refresh_forwards.clone();
        let tray_manager_clone = tray_manager.clone();
        let refresh_traffic_tab_clone = refresh_active_traffic_tab.clone();
        let floating_widget_clone = floating_widget.clone();

        *refresh_ui_for_language.borrow_mut() = Some(Rc::new(move || {
            win_connect_lbl.set_text(crate::i18n::tr("nav.connect"));
            win_rules_lbl.set_text(crate::i18n::tr("nav.rules"));
            win_traffic_lbl.set_text(crate::i18n::tr("nav.traffic"));
            win_logs_lbl.set_text(crate::i18n::tr("nav.logs"));
            win_forward_lbl.set_text(crate::i18n::tr("nav.forward"));
            win_settings_lbl.set_text(crate::i18n::tr("nav.settings"));
            win_back_btn.set_tooltip_text(Some(crate::i18n::tr("btn.back")));
            win_add_conn.set_tooltip_text(Some(crate::i18n::tr("btn.add_connection")));
            if *is_connected_clone.borrow() {
                win_bottom_status.set_text(crate::i18n::tr("status.connected"));
            } else {
                win_bottom_status.set_text(crate::i18n::tr("status.disconnected"));
            }

            if let Some(selected_row) = win_navigation.selected_row() {
                let title = match selected_row.index() {
                    1 => crate::i18n::tr("nav.rules"),
                    2 => crate::i18n::tr("nav.traffic"),
                    3 => crate::i18n::tr("nav.logs"),
                    4 => crate::i18n::tr("nav.forward"),
                    5 => crate::i18n::tr("nav.settings"),
                    _ => crate::i18n::tr("nav.connect"),
                };
                win_page_title.set_text(title);
            }

            connect_view_clone.refresh_labels();
            rules_view_clone.refresh_labels();
            traffic_view_clone.refresh_labels();
            logs_view_clone.refresh_labels();
            forward_view_clone.refresh_labels();
            settings_view_clone.refresh_labels();

            if let Some(hud) = floating_widget_clone.borrow().as_ref() {
                hud.refresh_labels();
            }

            if let Some(refresh) = refresh_conn_ref.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_forward_ref.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_traffic_tab_clone.borrow().as_ref() {
                refresh();
            }
            if let Some(tray) = tray_manager_clone.borrow().as_ref() {
                tray.refresh_menu();
            }
        }));
    }

    // 3. 侧边栏导航切换
    {
        let view_stack = win.view_stack.clone();
        let page_title = win.page_title.clone();
        let add_connection = win.add_connection.clone();
        let header = win.header.clone();
        let back_button = win.back_button.clone();
        let rules_switcher_box = rules_view.switcher_box.clone();
        let rules_domain_stack = rules_view.domain_stack.clone();
        let refresh_traffic_tab = refresh_active_traffic_tab.clone();
        win.navigation.connect_row_selected(move |_, row| {
            let Some(row) = row else { return; };
            let (name, title) = match row.index() {
                1 => ("rules", crate::i18n::tr("nav.rules")),
                2 => ("traffic", crate::i18n::tr("nav.traffic")),
                3 => ("logs", crate::i18n::tr("nav.logs")),
                4 => ("forward", crate::i18n::tr("nav.forward")),
                5 => ("settings", crate::i18n::tr("nav.settings")),
                _ => ("connect", crate::i18n::tr("nav.connect")),
            };
            view_stack.set_visible_child_name(name);
            add_connection.set_visible(name == "connect" || name == "forward");
            if name == "forward" {
                add_connection.set_tooltip_text(Some(crate::i18n::tr("btn.add_forward")));
            } else if name == "connect" {
                add_connection.set_tooltip_text(Some(crate::i18n::tr("btn.add_connection")));
            }
            back_button.set_visible(false);
            header.set_title_widget(Some(&page_title));
            page_title.set_text(title);
            if name == "rules" {
                rules_domain_stack.set_visible_child_name("overview");
                rules_switcher_box.set_visible(true);
            }
            if name == "traffic" {
                if let Some(refresh) = refresh_traffic_tab.borrow().as_ref() {
                    refresh();
                }
            }
        });
    }
    win.navigation.select_row(Some(&win.connect_nav));

    // 4. 节点列表渲染与刷新
    let refresh_rule_views: RefreshRules = Rc::new(RefCell::new(None));
    let refresh_blocked_views: RefreshRules = Rc::new(RefCell::new(None));
    let refresh_traffic_rule_counts_fn: Rc<RefCell<Option<Rc<dyn Fn()>>>> =
        Rc::new(RefCell::new(None));

    {
        let connection_flow = connect_view.connection_flow.clone();
        let connect_stack = connect_view.container.clone();
        let config = config.clone();
        let controller = controller.clone();
        let event_tx = event_tx.clone();
        let is_connected = is_connected.clone();
        let connection_buttons = connection_buttons.clone();
        let refresh_handle = refresh_connections.clone();
        let tray_manager = tray_manager.clone();
        let parent = win.window.clone();
        let bottom_status = win.bottom_status.clone();
        let refresh_impl: Rc<dyn Fn()> = Rc::new(move || {
            render_connection_cards(
                &connection_flow,
                &connect_stack,
                &config,
                &controller,
                &event_tx,
                &is_connected,
                &connection_buttons,
                &refresh_handle,
                &tray_manager,
                &parent,
                &bottom_status,
            );
        });
        *refresh_connections.borrow_mut() = Some(refresh_impl.clone());
        refresh_impl();
    }

    // 4.1 端口转发规则渲染与刷新
    {
        let forward_flow = forward_view.forward_flow.clone();
        let forward_container = forward_view.container.clone();
        let config = config.clone();
        let forward_manager = forward_manager.clone();
        let refresh_handle = refresh_forwards.clone();
        let parent = win.window.clone();

        let refresh_impl: Rc<dyn Fn()> = Rc::new(move || {
            render_forward_cards(
                &forward_flow,
                &forward_container,
                &config,
                &forward_manager,
                &refresh_handle,
                &parent,
            );
        });
        *refresh_forwards.borrow_mut() = Some(refresh_impl.clone());
        refresh_impl();
    }

    {
        let parent = win.window.clone();
        let config = config.clone();
        let refresh_connections = refresh_connections.clone();
        let refresh_forwards = refresh_forwards.clone();
        let view_stack = win.view_stack.clone();
        win.add_connection.connect_clicked(move |_| {
            if view_stack.visible_child_name().as_deref() == Some("forward") {
                show_forward_dialog(&parent, config.clone(), None, refresh_forwards.clone());
            } else {
                show_profile_dialog(&parent, config.clone(), None, refresh_connections.clone());
            }
        });
    }
    {
        let parent = win.window.clone();
        let config = config.clone();
        let refresh_connections = refresh_connections.clone();
        connect_view.empty_add_button.connect_clicked(move |_| {
            show_profile_dialog(&parent, config.clone(), None, refresh_connections.clone());
        });
    }
    {
        let parent = win.window.clone();
        let config = config.clone();
        let refresh_forwards = refresh_forwards.clone();
        forward_view.empty_add_button.connect_clicked(move |_| {
            show_forward_dialog(&parent, config.clone(), None, refresh_forwards.clone());
        });
    }

    // 4.2 自动启动已启用的端口转发
    {
        let config_clone = config.borrow().clone();
        let forward_manager_clone = forward_manager.clone();
        tokio::spawn(async move {
            let mut mgr = forward_manager_clone.lock().await;
            for rule in config_clone.port_forwards {
                if rule.enabled {
                    if let Some(profile) = config_clone.profiles.iter().find(|p| p.id == rule.profile_id) {
                        let _ = mgr.start(&rule, profile).await;
                    }
                }
            }
        });
    }

    // 5. 应用列表初始化
    for app_info in scan_desktop_apps() {
        let action = current_app_action(&config.borrow(), &app_info.executable);
        let row = adw::ComboRow::builder()
            .title(&app_info.name)
            .subtitle(&app_info.executable)
            .model(&gtk::StringList::new(&[
                crate::i18n::tr("action.direct"),
                crate::i18n::tr("action.proxy"),
                crate::i18n::tr("action.block"),
            ]))
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
        let refresh_traffic_counts_ref = refresh_traffic_rule_counts_fn.clone();
        row.connect_selected_notify(move |row| {
            let action = match row.selected() {
                1 => RuleAction::Proxy,
                2 => RuleAction::Block,
                _ => RuleAction::Direct,
            };
            if current_app_action(&config_ref.borrow(), &executable) == action {
                return;
            }
            set_app_action(&config_ref, &executable, action);
            controller_ref.borrow().sync_rules();
            if let Some(refresh) = refresh_blocked_ref.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_traffic_counts_ref.borrow().as_ref() {
                refresh();
            }
        });
        rules_view.applications_group.add(&row);
        rules_view.app_rows.borrow_mut().push((
            format!("{} {}", app_info.name, app_info.executable).to_lowercase(),
            app_info.name.to_lowercase(),
            row,
        ));
    }
    {
        let app_rows = rules_view.app_rows.clone();
        rules_view.app_search.connect_search_changed(move |entry| {
            let query = entry.text().to_lowercase();
            for (search_text, _, row) in app_rows.borrow().iter() {
                row.set_visible(query.is_empty() || search_text.contains(&query));
            }
        });
    }
    {
        let app_rows = rules_view.app_rows.clone();
        let applications_group = rules_view.applications_group.clone();
        rules_view.app_sort.connect_selected_notify(move |sort| {
            let mut rows = app_rows.borrow_mut();
            rows.sort_by(|left, right| {
                if sort.selected() == 1 {
                    left.2
                        .selected()
                        .cmp(&right.2.selected())
                        .then_with(|| left.1.cmp(&right.1))
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

    // 6. 域名与 IP 规则逻辑
    rules::setup_rules_logic(
        &win,
        &rules_view,
        &config,
        &controller,
        &refresh_rule_views,
        &refresh_blocked_views,
        &refresh_traffic_rule_counts_fn,
    );

    // 7. 流量监控逻辑
    let refresh_traffic_rule_counts_impl = {
        let config = config.clone();
        let total_rules_label = traffic_view.total_rules_label.clone();
        let distribution_data = traffic_view.distribution_data.clone();
        let distribution_area = traffic_view.distribution_area.clone();
        let proxy_legend_label = traffic_view.proxy_legend_label.clone();
        let reject_legend_label = traffic_view.reject_legend_label.clone();
        let direct_legend_label = traffic_view.direct_legend_label.clone();
        Rc::new(move || {
            refresh_traffic_rule_counts(
                &config,
                &total_rules_label,
                &distribution_data,
                &distribution_area,
                &proxy_legend_label,
                &reject_legend_label,
                &direct_legend_label,
            );
        })
    };
    *refresh_traffic_rule_counts_fn.borrow_mut() =
        Some(refresh_traffic_rule_counts_impl.clone());
    refresh_traffic_rule_counts_impl();

    let app_traffic_data = Rc::new(RefCell::new(Vec::<AppTrafficStat>::new()));
    let refresh_app_traffic: Rc<dyn Fn()> = {
        let app_traffic_data = app_traffic_data.clone();
        let app_traffic_search = traffic_view.app_traffic_search.clone();
        let traffic_scope_filter = traffic_view.traffic_scope_filter.clone();
        let app_traffic_sort = traffic_view.app_traffic_sort.clone();
        let app_traffic_list_box = traffic_view.app_traffic_list_box.clone();
        Rc::new(move || {
            refresh_app_traffic_list(
                &app_traffic_data,
                &app_traffic_search,
                &traffic_scope_filter,
                &app_traffic_sort,
                &app_traffic_list_box,
            );
        })
    };
    {
        let refresh = refresh_app_traffic.clone();
        traffic_view
            .app_traffic_search
            .connect_search_changed(move |_| refresh());
    }
    {
        let refresh = refresh_app_traffic.clone();
        traffic_view
            .traffic_scope_filter
            .connect_selected_notify(move |_| refresh());
    }
    {
        let refresh = refresh_app_traffic.clone();
        traffic_view
            .app_traffic_sort
            .connect_selected_notify(move |_| refresh());
    }

    let active_conns_data = Rc::new(RefCell::new(Vec::<ActiveConnectionStat>::new()));
    let refresh_conns: Rc<dyn Fn()> = {
        let active_conns_data = active_conns_data.clone();
        let conn_search = traffic_view.conn_search.clone();
        let conn_stats_label = traffic_view.conn_stats_label.clone();
        let conn_list_box = traffic_view.conn_list_box.clone();
        Rc::new(move || {
            refresh_connection_list(
                &active_conns_data,
                &conn_search,
                &conn_stats_label,
                &conn_list_box,
            );
        })
    };
    {
        let refresh = refresh_conns.clone();
        traffic_view
            .conn_search
            .connect_search_changed(move |_| refresh());
    }

    let refresh_active_traffic_tab_impl = {
        let refresh_apps = refresh_app_traffic.clone();
        let refresh_connections = refresh_conns.clone();
        let stack = traffic_view.stack.clone();
        Rc::new(move || {
            match stack.visible_child_name().as_deref() {
                Some("apps") => refresh_apps(),
                Some("connections") => refresh_connections(),
                _ => {}
            }
        })
    };
    *refresh_active_traffic_tab.borrow_mut() = Some(refresh_active_traffic_tab_impl.clone());

    {
        let refresh = refresh_active_traffic_tab_impl.clone();
        traffic_view.stack.connect_visible_child_name_notify(move |_| {
            refresh();
        });
    }

    // 8. 日志视图逻辑
    {
        let all_buffer = logs_view.all_log_buffer.clone();
        let system_buffer = logs_view.system_log_buffer.clone();
        let proxy_buffer = logs_view.proxy_log_buffer.clone();
        let direct_buffer = logs_view.direct_log_buffer.clone();
        let stack = logs_view.log_stack.clone();
        logs_view.clear_logs_btn.connect_clicked(move |_| {
            match stack.visible_child_name().as_deref() {
                Some("all") => all_buffer.set_text(""),
                Some("system") => system_buffer.set_text(""),
                Some("direct") => direct_buffer.set_text(""),
                _ => proxy_buffer.set_text(""),
            }
        });
    }
    {
        let all_buffer = logs_view.all_log_buffer.clone();
        let system_buffer = logs_view.system_log_buffer.clone();
        let proxy_buffer = logs_view.proxy_log_buffer.clone();
        let direct_buffer = logs_view.direct_log_buffer.clone();
        let stack = logs_view.log_stack.clone();
        logs_view.copy_logs_btn.connect_clicked(move |_| {
            let target_buffer = match stack.visible_child_name().as_deref() {
                Some("all") => &all_buffer,
                Some("system") => &system_buffer,
                Some("direct") => &direct_buffer,
                _ => &proxy_buffer,
            };
            let text =
                target_buffer.text(&target_buffer.start_iter(), &target_buffer.end_iter(), false);
            if let Some(display) = gtk::gdk::Display::default() {
                display.clipboard().set_text(&text);
            }
        });
    }
    {
        logs_view.open_logs_btn.connect_clicked(move |_| {
            if let Ok(log_path) = AppConfig::log_file_path() {
                if let Some(parent) = log_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if !log_path.exists() {
                    let _ = fs::write(&log_path, "");
                }
                let uri = format!("file://{}", log_path.display());
                let _ = gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>);
            }
        });
    }

    // 9. 托盘初始化
    {
        let profiles_config = config.clone();
        let select_config = config.clone();
        let select_refresh = refresh_connections.clone();
        let select_connected = is_connected.clone();
        let toggle_config = config.clone();
        let toggle_buttons = connection_buttons.clone();
        let show_window = win.window.clone();
        let quit_app = app.clone();
        let quit_controller = controller.clone();
        let quitting_ref = quitting.clone();
        let log_buffer_ref = logs_view.proxy_log_buffer.clone();
        let quit_forward_manager = forward_manager.clone();
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
                let f_mgr = quit_forward_manager.clone();
                tokio::spawn(async move {
                    f_mgr.lock().await.stop_all().await;
                });
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

    // 10. 桌面通用悬浮监控球 (Floating HUD)
    {
        let on_show_main = {
            let win_window = win.window.clone();
            Rc::new(move || {
                win_window.set_visible(true);
                win_window.present();
            })
        };
        let on_toggle_proxy = {
            let toggle_config = config.clone();
            let toggle_buttons = connection_buttons.clone();
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
            })
        };
        let on_open_settings = {
            let win_window = win.window.clone();
            let view_stack = win.view_stack.clone();
            Rc::new(move || {
                view_stack.set_visible_child_name("settings");
                win_window.set_visible(true);
                win_window.present();
            })
        };

        let floating_cfg = config.borrow().settings.floating_widget.clone();
        let get_is_connected = {
            let is_connected = is_connected.clone();
            Rc::new(move || *is_connected.borrow())
        };
        let hud = FloatingWidget::new(
            app,
            &floating_cfg,
            on_show_main,
            on_toggle_proxy,
            on_open_settings,
            get_is_connected,
        );
        hud.set_shown(floating_cfg.enabled);
        *floating_widget.borrow_mut() = Some(hud);
    }

    {
        let tray_manager = tray_manager.clone();
        let floating_widget = floating_widget.clone();
        let quitting = quitting.clone();
        win.window.connect_close_request(move |window| {
            if !*quitting.borrow() && tray_manager.borrow().is_some() {
                window.set_visible(false);
                gtk::glib::Propagation::Stop
            } else {
                if let Some(hud) = floating_widget.borrow().as_ref() {
                    hud.window.destroy();
                }
                gtk::glib::Propagation::Proceed
            }
        });
    }

    // 11. 远程规则下载触发
    {
        let event_tx = event_tx.clone();
        let rule_source = rules_view.rule_source_entry.clone();
        let import_rules = rules_view.import_trigger_btn.clone();
        let import_button = rules_view.import_button.clone();
        let rule_status = rules_view.rule_status_row.clone();
        import_rules.clone().connect_clicked(move |_| {
            let url = rule_source.text().trim().to_string();
            if url.is_empty() {
                rule_status.set_subtitle("订阅地址不能为空");
                return;
            }
            import_rules.set_sensitive(false);
            import_button.set_sensitive(false);
            rule_status.set_subtitle("正在下载并解析规则…");
            let events = event_tx.clone();
            thread::spawn(move || match import_rule_source(&url) {
                Ok(result) => {
                    let _ = events.send(RuntimeEvent::RulesImported {
                        result,
                        source_url: url,
                    });
                }
                Err(error) => {
                    let _ = events.send(RuntimeEvent::RuleImportFailed(error));
                }
            });
        });
    }

    // 12. 运行时事件总线轮询
    event_loop::setup_runtime_event_loop(
        event_rx,
        file_log_tx,
        &win,
        &rules_view,
        &traffic_view,
        &logs_view,
        &config,
        &controller,
        &is_connected,
        &connect_start_time,
        &refresh_connections,
        &refresh_rule_views,
        &tray_manager,
        &floating_widget,
        &sys_monitor,
        &latest_proxy_speed,
        &app_traffic_data,
        &active_conns_data,
        &refresh_app_traffic,
        &refresh_conns,
    );

    win.window.present();
}
