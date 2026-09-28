use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use ssh_rocket_core::{AppConfig, RuleAction};
use std::{cell::RefCell, rc::Rc};

use crate::{
    i18n::tr,
    scan_desktop_apps,
    ui::{
        traffic_view::overview::create_legend_item,
        widgets::{create_app_icon, format_bytes},
    },
    ActiveConnectionStat,
};

pub struct ConnectionsWidgets {
    pub conn_scroller: gtk::ScrolledWindow,
    pub conn_group: adw::PreferencesGroup,
    pub conn_search: gtk::SearchEntry,
    pub conn_stats_label: gtk::Label,
    pub conn_list_box: gtk::ListBox,

    pub distribution_group: adw::PreferencesGroup,
    pub total_rules_label: gtk::Label,
    pub distribution_data: Rc<RefCell<(f64, f64, f64)>>,
    pub distribution_area: gtk::DrawingArea,
    pub proxy_legend_label: gtk::Label,
    pub reject_legend_label: gtk::Label,
    pub direct_legend_label: gtk::Label,
}

pub fn build_connections_tab() -> ConnectionsWidgets {
    let conn_box = gtk::Box::new(gtk::Orientation::Vertical, 16);
    conn_box.set_margin_start(18);
    conn_box.set_margin_end(18);
    conn_box.set_margin_top(12);
    conn_box.set_margin_bottom(18);

    let conn_group = adw::PreferencesGroup::builder()
        .title(tr("traffic.conn.title"))
        .build();

    let conn_stats_label = gtk::Label::builder()
        .label("0")
        .css_classes(["dim-label", "numeric"])
        .build();
    conn_group.set_header_suffix(Some(&conn_stats_label));

    let conn_toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    conn_toolbar.set_margin_bottom(8);

    let conn_search = gtk::SearchEntry::builder()
        .placeholder_text(tr("traffic.conn.search"))
        .hexpand(true)
        .build();
    conn_toolbar.append(&conn_search);
    conn_group.add(&conn_toolbar);

    let conn_list_box = gtk::ListBox::new();
    conn_list_box.add_css_class("boxed-list");
    conn_list_box.set_selection_mode(gtk::SelectionMode::None);
    conn_group.add(&conn_list_box);
    conn_box.append(&conn_group);

    let distribution_group = adw::PreferencesGroup::builder()
        .title(tr("traffic.conn.rules_dist"))
        .build();

    let total_rules_label = gtk::Label::builder()
        .label("共 0 条策略")
        .css_classes(["dim-label", "numeric"])
        .build();
    distribution_group.set_header_suffix(Some(&total_rules_label));

    let dist_card = gtk::Box::new(gtk::Orientation::Vertical, 14);
    dist_card.add_css_class("card");
    dist_card.set_margin_top(4);
    dist_card.set_margin_bottom(4);
    dist_card.set_margin_start(4);
    dist_card.set_margin_end(4);

    let dist_content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    dist_content.set_margin_start(16);
    dist_content.set_margin_end(16);
    dist_content.set_margin_top(16);
    dist_content.set_margin_bottom(16);

    let distribution_data = Rc::new(RefCell::new((0.0f64, 0.0f64, 0.0f64)));
    let distribution_area = gtk::DrawingArea::builder()
        .content_height(12)
        .hexpand(true)
        .build();

    let draw_dist = distribution_data.clone();
    distribution_area.set_draw_func(move |_area, cr, width, height| {
        let (proxy_ratio, reject_ratio, direct_ratio) = *draw_dist.borrow();
        let total_ratio = proxy_ratio + reject_ratio + direct_ratio;
        let w = width as f64;
        let h = height as f64;
        if w <= 0.0 || h <= 0.0 {
            return;
        }

        let r = 6.0f64.min(h / 2.0).min(w / 2.0);
        cr.new_sub_path();
        cr.arc(w - r, r, r, -std::f64::consts::FRAC_PI_2, 0.0);
        cr.arc(w - r, h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
        cr.arc(r, h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
        cr.arc(r, r, r, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2);
        cr.close_path();
        let _ = cr.clip();

        cr.set_source_rgba(1.0, 1.0, 1.0, 0.08);
        let _ = cr.paint();

        if total_ratio <= 0.0 {
            return;
        }

        let proxy_w = (w * proxy_ratio).round();
        let reject_w = (w * reject_ratio).round();
        let direct_w = (w - proxy_w - reject_w).max(0.0);

        let mut current_x = 0.0;
        if proxy_w > 0.0 {
            cr.set_source_rgb(0.18, 0.76, 0.49);
            cr.rectangle(current_x, 0.0, proxy_w, h);
            let _ = cr.fill();
            current_x += proxy_w;
        }
        if reject_w > 0.0 {
            cr.set_source_rgb(0.88, 0.11, 0.14);
            cr.rectangle(current_x, 0.0, reject_w, h);
            let _ = cr.fill();
            current_x += reject_w;
        }
        if direct_w > 0.0 {
            cr.set_source_rgb(0.21, 0.52, 0.89);
            cr.rectangle(current_x, 0.0, direct_w, h);
            let _ = cr.fill();
        }
    });

    dist_content.append(&distribution_area);

    let legend_box = gtk::Box::new(gtk::Orientation::Horizontal, 24);
    legend_box.set_halign(gtk::Align::Start);

    let (proxy_legend_item, proxy_legend_label) =
        create_legend_item("distribution-seg-proxy", &format!("{}: 0 (0%)", tr("traffic.conn.proxy")));
    legend_box.append(&proxy_legend_item);

    let (reject_legend_item, reject_legend_label) =
        create_legend_item("distribution-seg-reject", &format!("{}: 0 (0%)", tr("traffic.conn.reject")));
    legend_box.append(&reject_legend_item);

    let (direct_legend_item, direct_legend_label) =
        create_legend_item("distribution-seg-direct", &format!("{}: 0 (0%)", tr("traffic.conn.direct")));
    legend_box.append(&direct_legend_item);

    dist_content.append(&legend_box);
    dist_card.append(&dist_content);
    distribution_group.add(&dist_card);
    conn_box.append(&distribution_group);

    let conn_scroller = gtk::ScrolledWindow::builder()
        .child(&conn_box)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();

    ConnectionsWidgets {
        conn_scroller,
        conn_group,
        conn_search,
        conn_stats_label,
        conn_list_box,
        distribution_group,
        total_rules_label,
        distribution_data,
        distribution_area,
        proxy_legend_label,
        reject_legend_label,
        direct_legend_label,
    }
}

/// 刷新规则策略分布自适应比例条
pub fn refresh_traffic_rule_counts(
    config: &Rc<RefCell<AppConfig>>,
    total_rules_label: &gtk::Label,
    distribution_data: &Rc<RefCell<(f64, f64, f64)>>,
    distribution_area: &gtk::DrawingArea,
    proxy_legend_label: &gtk::Label,
    reject_legend_label: &gtk::Label,
    direct_legend_label: &gtk::Label,
) {
    let current = config.borrow();
    let app_count = scan_desktop_apps().len();

    let mut direct_count = 0usize;
    let mut proxy_count = 0usize;
    let mut reject_count = 0usize;

    for rule in &current.settings.imported_domain_rules {
        match rule.action {
            RuleAction::Direct => direct_count += 1,
            RuleAction::Proxy => proxy_count += 1,
            RuleAction::Block => reject_count += 1,
        }
    }
    for rule in &current.settings.imported_ip_rules {
        match rule.action {
            RuleAction::Direct => direct_count += 1,
            RuleAction::Proxy => proxy_count += 1,
            RuleAction::Block => reject_count += 1,
        }
    }
    for rule in &current.settings.domain_rules {
        match rule.action {
            RuleAction::Direct => direct_count += 1,
            RuleAction::Proxy => proxy_count += 1,
            RuleAction::Block => reject_count += 1,
        }
    }
    for rule in &current.settings.ip_rules {
        match rule.action {
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

    let total = direct_count + proxy_count + reject_count;
    let is_en = crate::i18n::current_language() == ssh_rocket_core::Language::English;
    total_rules_label.set_text(&if is_en {
        format!("{total} rules total")
    } else {
        format!("共 {total} 条策略")
    });

    let proxy_name = tr("action.proxy");
    let reject_name = tr("action.block");
    let direct_name = tr("action.direct");

    if total == 0 {
        *distribution_data.borrow_mut() = (0.0, 0.0, 0.0);
        distribution_area.queue_draw();
        proxy_legend_label.set_text(&format!("{proxy_name}: 0 (0.0%)"));
        reject_legend_label.set_text(&format!("{reject_name}: 0 (0.0%)"));
        direct_legend_label.set_text(&format!("{direct_name}: 0 (0.0%)"));
        return;
    }

    let proxy_ratio = proxy_count as f64 / total as f64;
    let reject_ratio = reject_count as f64 / total as f64;
    let direct_ratio = direct_count as f64 / total as f64;

    *distribution_data.borrow_mut() = (proxy_ratio, reject_ratio, direct_ratio);
    distribution_area.queue_draw();

    proxy_legend_label.set_text(&format!(
        "{proxy_name}: {proxy_count} ({:.1}%)",
        proxy_ratio * 100.0
    ));
    reject_legend_label.set_text(&format!(
        "{reject_name}: {reject_count} ({:.1}%)",
        reject_ratio * 100.0
    ));
    direct_legend_label.set_text(&format!(
        "{direct_name}: {direct_count} ({:.1}%)",
        direct_ratio * 100.0
    ));
}

/// 刷新活跃 Socket 连接明细列表
pub fn refresh_connection_list(
    conns_data: &Rc<RefCell<Vec<ActiveConnectionStat>>>,
    conn_search: &gtk::SearchEntry,
    conn_stats_label: &gtk::Label,
    conn_list_box: &gtk::ListBox,
) {
    let query = conn_search.text().trim().to_lowercase();
    let mut items: Vec<ActiveConnectionStat> = conns_data
        .borrow()
        .iter()
        .filter(|conn| {
            query.is_empty()
                || conn.proc_name.to_lowercase().contains(&query)
                || conn.peer_addr.to_lowercase().contains(&query)
                || conn.local_addr.to_lowercase().contains(&query)
        })
        .cloned()
        .collect();

    let is_en = crate::i18n::current_language() == ssh_rocket_core::Language::English;
    conn_stats_label.set_text(&if is_en {
        format!("{} active connections", items.len())
    } else {
        format!("共 {} 个活跃连接", items.len())
    });
    items.truncate(100);

    while let Some(child) = conn_list_box.first_child() {
        conn_list_box.remove(&child);
    }

    if items.is_empty() {
        let empty_row = adw::ActionRow::builder()
            .title(if is_en { "No Active Connections" } else { "暂无活跃连接" })
            .build();
        conn_list_box.append(&empty_row);
        return;
    }

    for conn in items {
        let row = adw::ActionRow::builder()
            .title(&conn.proc_name)
            .subtitle(&format!("{} ➔ {}", conn.local_addr, conn.peer_addr))
            .build();
        row.add_prefix(&create_app_icon(&conn.icon));

        // 实时上下行流量（左列：固定宽度右对齐）
        let traffic_lbl = gtk::Label::builder()
            .label(&format!(
                "↑ {}   ↓ {}",
                format_bytes(conn.upload),
                format_bytes(conn.download)
            ))
            .css_classes(["dim-label", "numeric"])
            .valign(gtk::Align::Center)
            .halign(gtk::Align::End)
            .width_request(160)
            .build();
        row.add_suffix(&traffic_lbl);

        // 路由走向胶囊徽标（最右侧：固定宽度居中对齐）
        let badge = gtk::Label::builder()
            .label(conn.conn_type.label())
            .css_classes([conn.conn_type.badge_class()])
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Center)
            .width_request(58)
            .build();
        row.add_suffix(&badge);

        conn_list_box.append(&row);
    }
}
