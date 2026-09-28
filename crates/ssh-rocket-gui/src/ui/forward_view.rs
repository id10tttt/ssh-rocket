use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use ssh_rocket_core::{AppConfig, ForwardType};
use std::{cell::RefCell, rc::Rc, sync::mpsc};

use crate::{
    controller::{RuntimeController, RuntimeEvent},
    i18n::tr,
    ui::dialogs::{show_forward_dialog, RefreshForwards},
};

#[derive(Clone)]
pub struct ForwardView {
    pub container: gtk::Stack,
    pub empty_page: adw::StatusPage,
    pub empty_add_button: gtk::Button,
    pub forward_flow: gtk::FlowBox,
}

impl ForwardView {
    pub fn new() -> Self {
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);

        // 空状态页
        let empty_page = adw::StatusPage::builder()
            .icon_name("network-transmit-receive-symbolic")
            .title(tr("forward.empty.title"))
            .description(tr("forward.empty.desc"))
            .build();
        let empty_add = gtk::Button::with_label(tr("forward.btn.add"));
        empty_add.add_css_class("suggested-action");
        empty_add.add_css_class("pill");
        empty_add.set_halign(gtk::Align::Center);
        empty_page.set_child(Some(&empty_add));
        stack.add_named(&empty_page, Some("empty"));

        // 转发规则流式网格（双列等宽）
        let forward_flow = gtk::FlowBox::new();
        forward_flow.set_selection_mode(gtk::SelectionMode::None);
        forward_flow.set_column_spacing(14);
        forward_flow.set_row_spacing(14);
        forward_flow.set_min_children_per_line(2);
        forward_flow.set_max_children_per_line(2);
        forward_flow.set_homogeneous(true);
        forward_flow.set_valign(gtk::Align::Start);
        forward_flow.set_margin_start(16);
        forward_flow.set_margin_end(16);
        forward_flow.set_margin_top(14);
        forward_flow.set_margin_bottom(14);

        let scroller = gtk::ScrolledWindow::builder()
            .child(&forward_flow)
            .vexpand(true)
            .build();
        stack.add_named(&scroller, Some("cards"));

        Self {
            container: stack,
            empty_page,
            empty_add_button: empty_add,
            forward_flow,
        }
    }

    pub fn refresh_labels(&self) {
        self.empty_page.set_title(tr("forward.empty.title"));
        self.empty_page.set_description(Some(tr("forward.empty.desc")));
        self.empty_add_button.set_label(tr("forward.btn.add"));
    }
}

/// 重新渲染端口转发规则卡片
pub fn render_forward_cards(
    forward_flow: &gtk::FlowBox,
    container: &gtk::Stack,
    config: &Rc<RefCell<AppConfig>>,
    controller: &Rc<RefCell<RuntimeController>>,
    event_tx: &mpsc::Sender<RuntimeEvent>,
    refresh_handle: &RefreshForwards,
    parent: &adw::ApplicationWindow,
) {
    while let Some(child) = forward_flow.first_child() {
        let Ok(child) = child.downcast::<gtk::FlowBoxChild>() else {
            break;
        };
        forward_flow.remove(&child);
    }

    let current_config = config.borrow();
    let rules = current_config.port_forwards.clone();
    let profiles = current_config.profiles.clone();
    drop(current_config);

    if rules.is_empty() {
        container.set_visible_child_name("empty");
        return;
    }
    container.set_visible_child_name("cards");

    for rule in rules {
        let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
        card.add_css_class("card");
        card.set_hexpand(true);
        card.set_valign(gtk::Align::Start);
        card.set_vexpand(false);
        card.set_margin_start(2);
        card.set_margin_end(2);
        card.set_margin_top(2);
        card.set_margin_bottom(2);

        // 卡片顶部标题栏
        let header_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        header_row.set_margin_start(14);
        header_row.set_margin_end(10);
        header_row.set_margin_top(12);

        // 状态指示灯
        let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        dot.add_css_class("status-dot");
        if rule.enabled {
            dot.add_css_class("status-dot-connected");
        }
        header_row.append(&dot);

        // 规则名称
        let title_label = gtk::Label::new(Some(&rule.name));
        title_label.add_css_class("card-title");
        title_label.set_hexpand(true);
        title_label.set_halign(gtk::Align::Start);
        title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        header_row.append(&title_label);

        // 转发类型徽章
        let type_badge = gtk::Label::new(Some(match rule.forward_type {
            ForwardType::Local => tr("forward.type.local"),
            ForwardType::Remote => tr("forward.type.remote"),
        }));
        type_badge.add_css_class("badge");
        header_row.append(&type_badge);

        // 启停开关
        let switch = gtk::Switch::builder()
            .active(rule.enabled)
            .valign(gtk::Align::Center)
            .build();

        {
            let config_clone = config.clone();
            let controller_clone = controller.clone();
            let event_tx = event_tx.clone();
            let rule_id = rule.id;
            let dot_clone = dot.clone();
            switch.connect_state_set(move |_, state| {
                let mut current = config_clone.borrow_mut();
                if let Some(target) = current.port_forwards.iter_mut().find(|r| r.id == rule_id) {
                    target.enabled = state;
                    let rule_clone = target.clone();
                    let profile_opt = current.profiles.iter().find(|p| p.id == rule_clone.profile_id).cloned();
                    let _ = current.save();
                    drop(current);

                    if state {
                        dot_clone.add_css_class("status-dot-connected");
                    } else {
                        dot_clone.remove_css_class("status-dot-connected");
                    }

                    controller_clone.borrow().set_forward_enabled(
                        rule_clone,
                        profile_opt,
                        state,
                        event_tx.clone(),
                    );
                }
                gtk::glib::Propagation::Proceed
            });
        }
        header_row.append(&switch);

        // 编辑按钮
        let edit_btn = gtk::Button::from_icon_name("document-edit-symbolic");
        edit_btn.add_css_class("flat");
        edit_btn.add_css_class("circular");
        edit_btn.set_tooltip_text(Some(tr("forward.menu.edit")));
        {
            let parent_clone = parent.clone();
            let config_clone = config.clone();
            let refresh_clone = refresh_handle.clone();
            let rule_clone = rule.clone();
            edit_btn.connect_clicked(move |_| {
                show_forward_dialog(&parent_clone, config_clone.clone(), Some(rule_clone.clone()), refresh_clone.clone());
            });
        }
        header_row.append(&edit_btn);

        // 删除按钮
        let delete_btn = gtk::Button::from_icon_name("user-trash-symbolic");
        delete_btn.add_css_class("flat");
        delete_btn.add_css_class("circular");
        delete_btn.set_tooltip_text(Some(tr("forward.menu.delete")));
        {
            let config_clone = config.clone();
            let refresh_clone = refresh_handle.clone();
            let controller_clone = controller.clone();
            let event_tx = event_tx.clone();
            let rule_id = rule.id;
            delete_btn.connect_clicked(move |_| {
                let mut current = config_clone.borrow_mut();
                current.port_forwards.retain(|r| r.id != rule_id);
                let _ = current.save();
                drop(current);

                controller_clone
                    .borrow()
                    .stop_forward(rule_id, event_tx.clone());

                if let Some(refresh) = refresh_clone.borrow().as_ref() {
                    refresh();
                }
            });
        }
        header_row.append(&delete_btn);

        card.append(&header_row);

        // 核心可视化拓扑卡片 (本地端 <--> SSH 连接 <--> 远端)
        let topo_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        topo_box.set_margin_start(14);
        topo_box.set_margin_end(14);
        topo_box.set_margin_top(6);
        topo_box.set_margin_bottom(12);
        topo_box.set_valign(gtk::Align::Center);

        // 左端：本地端
        let local_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
        local_box.set_hexpand(true);
        let local_title = gtk::Label::new(Some("本地端口"));
        local_title.add_css_class("dim-label");
        local_title.set_xalign(0.0);
        let local_val = gtk::Label::new(Some(&format!("{}:{}", rule.local_host, rule.local_port)));
        local_val.add_css_class("monospace");
        local_val.set_xalign(0.0);
        local_box.append(&local_title);
        local_box.append(&local_val);
        topo_box.append(&local_box);

        // 中间：流向指示与 SSH 节点
        let profile_name = profiles
            .iter()
            .find(|p| p.id == rule.profile_id)
            .map(|p| p.name.as_str())
            .unwrap_or("未知节点");

        let mid_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
        mid_box.set_halign(gtk::Align::Center);

        let arrow_text = match rule.forward_type {
            ForwardType::Local => "── 转发至 ──>",
            ForwardType::Remote => "<── 暴露自 ──",
        };
        let arrow_lbl = gtk::Label::new(Some(arrow_text));
        arrow_lbl.add_css_class("accent");
        arrow_lbl.set_halign(gtk::Align::Center);

        let conn_badge = gtk::Label::new(Some(&format!("via {}", profile_name)));
        conn_badge.add_css_class("dim-label");
        conn_badge.set_ellipsize(gtk::pango::EllipsizeMode::End);
        conn_badge.set_max_width_chars(16);
        conn_badge.set_halign(gtk::Align::Center);

        mid_box.append(&arrow_lbl);
        mid_box.append(&conn_badge);
        topo_box.append(&mid_box);

        // 右端：远端目标
        let remote_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
        remote_box.set_hexpand(true);
        let remote_title = gtk::Label::new(Some("目标地址"));
        remote_title.add_css_class("dim-label");
        remote_title.set_xalign(1.0);
        let remote_val = gtk::Label::new(Some(&format!("{}:{}", rule.remote_host, rule.remote_port)));
        remote_val.add_css_class("monospace");
        remote_val.set_xalign(1.0);
        remote_box.append(&remote_title);
        remote_box.append(&remote_val);
        topo_box.append(&remote_box);

        card.append(&topo_box);

        forward_flow.append(&card);
    }
}
