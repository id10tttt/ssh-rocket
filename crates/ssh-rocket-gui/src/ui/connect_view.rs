use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use ssh_rocket_core::{AppConfig, AuthType};
use std::{cell::RefCell, rc::Rc, sync::mpsc};

use crate::{
    tray::{TrayConnectionState, TrayManager},
    ui::dialogs::{show_profile_dialog, RefreshConnections},
    RuntimeController, RuntimeEvent,
};

pub struct ConnectView {
    pub container: gtk::Stack,
    pub empty_add_button: gtk::Button,
    pub connection_flow: gtk::FlowBox,
}

impl ConnectView {
    pub fn new() -> Self {
        let connect_stack = gtk::Stack::new();
        connect_stack.set_vexpand(true);

        // 空状态页
        let empty_connections = adw::StatusPage::builder()
            .icon_name("network-server-symbolic")
            .title("暂无节点配置")
            .description("添加 SSH 节点服务器以开启透明代理")
            .build();
        let empty_add = gtk::Button::with_label("添加连接");
        empty_add.add_css_class("suggested-action");
        empty_add.add_css_class("pill");
        empty_add.set_halign(gtk::Align::Center);
        empty_connections.set_child(Some(&empty_add));
        connect_stack.add_named(&empty_connections, Some("empty"));

        // 节点卡片流式网格（50% / 50% 响应式双列等宽均分）
        let connection_flow = gtk::FlowBox::new();
        connection_flow.set_selection_mode(gtk::SelectionMode::None);
        connection_flow.set_column_spacing(14);
        connection_flow.set_row_spacing(14);
        connection_flow.set_min_children_per_line(2);
        connection_flow.set_max_children_per_line(2);
        connection_flow.set_homogeneous(true);
        connection_flow.set_valign(gtk::Align::Start);
        connection_flow.set_margin_start(16);
        connection_flow.set_margin_end(16);
        connection_flow.set_margin_top(14);
        connection_flow.set_margin_bottom(14);

        let connection_scroller = gtk::ScrolledWindow::builder()
            .child(&connection_flow)
            .vexpand(true)
            .build();
        connect_stack.add_named(&connection_scroller, Some("cards"));

        Self {
            container: connect_stack,
            empty_add_button: empty_add,
            connection_flow,
        }
    }
}

/// 重新渲染节点卡片流
pub fn render_connection_cards(
    connection_flow: &gtk::FlowBox,
    connect_stack: &gtk::Stack,
    config: &Rc<RefCell<AppConfig>>,
    controller: &Rc<RefCell<RuntimeController>>,
    event_tx: &mpsc::Sender<RuntimeEvent>,
    is_connected: &Rc<RefCell<bool>>,
    connection_buttons: &Rc<RefCell<Vec<(String, gtk::Button)>>>,
    refresh_handle: &RefreshConnections,
    tray_manager: &Rc<RefCell<Option<Rc<TrayManager>>>>,
    parent: &adw::ApplicationWindow,
    bottom_status: &gtk::Label,
) {
    while let Some(child) = connection_flow.first_child() {
        let Ok(child) = child.downcast::<gtk::FlowBoxChild>() else {
            break;
        };
        connection_flow.remove(&child);
    }
    connection_buttons.borrow_mut().clear();

    let current_config = config.borrow();
    let profiles = current_config.profiles.clone();
    let active_profile_id = current_config.active_profile;
    drop(current_config);

    if profiles.is_empty() {
        connect_stack.set_visible_child_name("empty");
        if let Some(tray) = tray_manager.borrow().as_ref() {
            tray.refresh_menu();
        }
        return;
    }
    connect_stack.set_visible_child_name("cards");

    let connected = *is_connected.borrow();

    for profile in profiles {
        let is_active = active_profile_id == Some(profile.id);

        let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
        card.add_css_class("card");
        if is_active && connected {
            card.add_css_class("active-profile-card");
        }
        card.set_hexpand(true);
        card.set_valign(gtk::Align::Start);
        card.set_vexpand(false);
        card.set_margin_start(2);
        card.set_margin_end(2);
        card.set_margin_top(2);
        card.set_margin_bottom(2);

        // 卡片顶部标题栏
        let header_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        header_row.set_margin_start(14);
        header_row.set_margin_end(8);
        header_row.set_margin_top(10);

        // 状态圆点
        let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        dot.add_css_class("status-dot");
        if is_active && connected {
            dot.add_css_class("status-dot-connected");
        } else if is_active && controller.borrow().is_running() {
            dot.add_css_class("status-dot-connecting");
        } else {
            dot.add_css_class("status-dot-disconnected");
        }
        dot.set_valign(gtk::Align::Center);
        header_row.append(&dot);

        let title = gtk::Label::new(Some(&profile.name));
        title.add_css_class("title-3");
        title.set_halign(gtk::Align::Start);
        title.set_hexpand(true);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        header_row.append(&title);

        if is_active && connected {
            let active_tag = gtk::Label::new(Some("已连接"));
            active_tag.add_css_class("status-badge");
            active_tag.add_css_class("status-badge-connected");
            active_tag.set_valign(gtk::Align::Center);
            header_row.append(&active_tag);
        }

        let edit = gtk::Button::from_icon_name("document-edit-symbolic");
        edit.add_css_class("flat");
        edit.set_tooltip_text(Some("编辑节点"));
        header_row.append(&edit);

        let remove = gtk::Button::from_icon_name("user-trash-symbolic");
        remove.add_css_class("flat");
        remove.set_tooltip_text(Some("删除节点"));
        header_row.append(&remove);
        card.append(&header_row);

        // 参数信息列表
        let info = gtk::Box::new(gtk::Orientation::Vertical, 4);
        info.set_margin_start(14);
        info.set_margin_end(14);
        for (key, value) in [
            ("服务器", profile.host.clone()),
            ("端口", profile.port.to_string()),
            (
                "用户名",
                if profile.username.is_empty() {
                    "—".into()
                } else {
                    profile.username.clone()
                },
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

        // 认证信息（私钥或密码）及脱敏切换
        let (auth_key, auth_val) = match profile.auth_type {
            AuthType::Key => (
                "私钥",
                profile
                    .identity_file
                    .as_ref()
                    .map(|path| path.to_string_lossy().to_string()),
            ),
            AuthType::Password => ("密码", profile.password.clone()),
        };

        let auth_line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let key_label = gtk::Label::new(Some(auth_key));
        key_label.add_css_class("dim-label");
        key_label.set_halign(gtk::Align::Start);
        key_label.set_hexpand(true);
        auth_line.append(&key_label);

        if let Some(val) = auth_val.filter(|v| !v.trim().is_empty()) {
            let value_label = gtk::Label::new(Some("••••••••"));
            value_label.set_halign(gtk::Align::End);
            value_label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            auth_line.append(&value_label);

            let eye_btn = gtk::Button::from_icon_name("view-reveal-symbolic");
            eye_btn.add_css_class("flat");
            eye_btn.set_valign(gtk::Align::Center);
            eye_btn.set_tooltip_text(Some("查看明文"));

            let is_revealed = Rc::new(RefCell::new(false));
            {
                let is_revealed = is_revealed.clone();
                let value_label = value_label.clone();
                let real_text = val.clone();
                eye_btn.connect_clicked(move |btn| {
                    let mut rev = is_revealed.borrow_mut();
                    *rev = !*rev;
                    if *rev {
                        value_label.set_text(&real_text);
                        btn.set_icon_name("view-conceal-symbolic");
                        btn.set_tooltip_text(Some("隐藏明文"));
                    } else {
                        value_label.set_text("••••••••");
                        btn.set_icon_name("view-reveal-symbolic");
                        btn.set_tooltip_text(Some("查看明文"));
                    }
                });
            }
            auth_line.append(&eye_btn);
        } else {
            let value_label = gtk::Label::new(Some("—"));
            value_label.set_halign(gtk::Align::End);
            auth_line.append(&value_label);
        }
        info.append(&auth_line);

        card.append(&info);
        card.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        // 操作区域
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        actions.set_margin_start(14);
        actions.set_margin_end(14);
        actions.set_margin_bottom(10);

        let connect_button = gtk::Button::with_label(if is_active && connected {
            "断开连接"
        } else {
            "连接"
        });
        if is_active && connected {
            connect_button.add_css_class("destructive-action");
        } else {
            connect_button.add_css_class("suggested-action");
        }
        connect_button.add_css_class("pill");
        connect_button.set_halign(gtk::Align::End);

        if connected {
            connect_button.set_sensitive(is_active);
        }

        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        actions.append(&spacer);
        actions.append(&connect_button);
        card.append(&actions);

        connection_buttons
            .borrow_mut()
            .push((profile.id.to_string(), connect_button.clone()));

        // 事件绑定
        {
            let parent = parent.clone();
            let config = config.clone();
            let refresh_handle = refresh_handle.clone();
            let profile = profile.clone();
            edit.connect_clicked(move |_| {
                show_profile_dialog(
                    &parent,
                    config.clone(),
                    Some(profile.clone()),
                    refresh_handle.clone(),
                );
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
                    bottom_status.set_text("正在断开…");
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
                        bottom_status.set_text(&format!("配置保存失败: {error}"));
                        return;
                    }
                }
                let Ok(config_path) = AppConfig::path() else {
                    bottom_status.set_text("无法解析配置文件路径");
                    return;
                };
                bottom_status.set_text("正在连接…");
                for (_, button) in connection_buttons.borrow().iter() {
                    button.set_sensitive(false);
                    button.set_label("连接");
                }
                connect_button_ref.set_label("连接中…");
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
        if let Some(child) = card.parent().and_then(|p| p.downcast::<gtk::FlowBoxChild>().ok()) {
            child.set_hexpand(true);
            child.set_halign(gtk::Align::Fill);
        }
    }

    if let Some(tray) = tray_manager.borrow().as_ref() {
        tray.refresh_menu();
    }
}
