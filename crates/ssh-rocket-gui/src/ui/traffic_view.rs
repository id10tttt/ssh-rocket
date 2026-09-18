use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use ssh_rocket_core::{AppConfig, RuleAction};
use std::{cell::RefCell, rc::Rc};

use crate::{
    scan_desktop_apps,
    ui::widgets::{create_app_icon, format_bytes},
    AppTrafficStat,
};

pub struct TrafficView {
    pub page: gtk::ScrolledWindow,

    // 1. KPI 概览面板组件
    pub started_label: gtk::Label,
    pub duration_label: gtk::Label,
    pub total_hero_label: gtk::Label,
    pub total_up_label: gtk::Label,
    pub total_down_label: gtk::Label,
    pub proxy_hero_label: gtk::Label,
    pub proxy_up_label: gtk::Label,
    pub proxy_down_label: gtk::Label,
    pub direct_hero_label: gtk::Label,
    pub direct_up_label: gtk::Label,
    pub direct_down_label: gtk::Label,

    // 2. 规则策略横向分段比例条
    pub total_rules_label: gtk::Label,
    pub proxy_seg: gtk::Box,
    pub reject_seg: gtk::Box,
    pub direct_seg: gtk::Box,
    pub proxy_legend_label: gtk::Label,
    pub reject_legend_label: gtk::Label,
    pub direct_legend_label: gtk::Label,

    // 3. 进程流量统计
    pub app_traffic_search: gtk::SearchEntry,
    pub app_traffic_sort: gtk::DropDown,
    pub app_traffic_list_box: gtk::Box,
}

impl TrafficView {
    pub fn new() -> Self {
        let traffic_page = adw::PreferencesPage::new();

        // --- 1. 会话与传输总览 (4 列统一 KPI 面板) ---
        let overview_group = adw::PreferencesGroup::builder()
            .title("会话与传输总览")
            .build();

        let overview_card = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        overview_card.add_css_class("card");
        overview_card.set_homogeneous(true);

        // 列 1: 总传输量
        let (tile_total, total_hero_label, total_up_label, total_down_label) =
            create_kpi_tile("总传输量", "0 B");
        overview_card.append(&tile_total);

        // 列 2: 连接会话 (时长与开始时间)
        let tile_session = gtk::Box::new(gtk::Orientation::Vertical, 6);
        tile_session.add_css_class("metric-tile");
        tile_session.set_hexpand(true);

        let session_title = gtk::Label::builder()
            .label("连接时长")
            .halign(gtk::Align::Start)
            .css_classes(["metric-title", "dim-label"])
            .build();
        tile_session.append(&session_title);

        let duration_label = gtk::Label::builder()
            .label("00:00:00")
            .halign(gtk::Align::Start)
            .css_classes(["metric-hero", "numeric"])
            .build();
        tile_session.append(&duration_label);

        let started_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let started_prefix = gtk::Label::builder()
            .label("始于")
            .css_classes(["dim-label"])
            .build();
        started_box.append(&started_prefix);
        let started_label = gtk::Label::builder()
            .label("—")
            .halign(gtk::Align::Start)
            .css_classes(["dim-label", "numeric"])
            .build();
        started_box.append(&started_label);
        tile_session.append(&started_box);
        overview_card.append(&tile_session);

        // 列 3: 代理流量
        let (tile_proxy, proxy_hero_label, proxy_up_label, proxy_down_label) =
            create_kpi_tile("代理流量", "0 B");
        overview_card.append(&tile_proxy);

        // 列 4: 直连流量
        let (tile_direct, direct_hero_label, direct_up_label, direct_down_label) =
            create_kpi_tile("直连流量", "0 B");
        overview_card.append(&tile_direct);

        overview_group.add(&overview_card);
        traffic_page.add(&overview_group);

        // --- 2. 规则策略分布 (横向分段比例条) ---
        let distribution_group = adw::PreferencesGroup::builder()
            .title("规则策略分布")
            .build();

        let total_rules_label = gtk::Label::builder()
            .label("共 0 条规则")
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

        // 横向彩色分段条
        let distribution_bar = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        distribution_bar.add_css_class("distribution-bar");
        distribution_bar.set_hexpand(true);

        let proxy_seg = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        proxy_seg.add_css_class("distribution-seg-proxy");
        proxy_seg.set_hexpand(false);
        distribution_bar.append(&proxy_seg);

        let reject_seg = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        reject_seg.add_css_class("distribution-seg-reject");
        reject_seg.set_hexpand(false);
        distribution_bar.append(&reject_seg);

        let direct_seg = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        direct_seg.add_css_class("distribution-seg-direct");
        direct_seg.set_hexpand(false);
        distribution_bar.append(&direct_seg);

        dist_content.append(&distribution_bar);

        // 图例与数值指示 (Legend)
        let legend_box = gtk::Box::new(gtk::Orientation::Horizontal, 24);
        legend_box.set_halign(gtk::Align::Start);

        let (proxy_legend_item, proxy_legend_label) =
            create_legend_item("distribution-seg-proxy", "代理: 0 (0%)");
        legend_box.append(&proxy_legend_item);

        let (reject_legend_item, reject_legend_label) =
            create_legend_item("distribution-seg-reject", "拦截: 0 (0%)");
        legend_box.append(&reject_legend_item);

        let (direct_legend_item, direct_legend_label) =
            create_legend_item("distribution-seg-direct", "直连: 0 (0%)");
        legend_box.append(&direct_legend_item);

        dist_content.append(&legend_box);
        dist_card.append(&dist_content);

        distribution_group.add(&dist_card);
        traffic_page.add(&distribution_group);

        // --- 3. 进程流量排行 ---
        let app_usage_group = adw::PreferencesGroup::builder()
            .title("进程流量统计")
            .build();
        let app_traffic_toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let app_traffic_search = gtk::SearchEntry::builder()
            .placeholder_text("搜索应用或进程")
            .hexpand(true)
            .build();
        app_traffic_toolbar.append(&app_traffic_search);
        let app_traffic_sort_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let app_traffic_sort_label = gtk::Label::new(Some("排序"));
        app_traffic_sort_label.add_css_class("dim-label");
        app_traffic_sort_box.append(&app_traffic_sort_label);
        let app_traffic_sort = gtk::DropDown::from_strings(&["按流量", "按名称"]);
        app_traffic_sort_box.append(&app_traffic_sort);
        app_traffic_toolbar.append(&app_traffic_sort_box);
        app_usage_group.add(&app_traffic_toolbar);

        let app_traffic_list_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        app_usage_group.add(&app_traffic_list_box);
        traffic_page.add(&app_usage_group);

        let scroller = gtk::ScrolledWindow::builder()
            .child(&traffic_page)
            .vexpand(true)
            .build();

        Self {
            page: scroller,
            started_label,
            duration_label,
            total_hero_label,
            total_up_label,
            total_down_label,
            proxy_hero_label,
            proxy_up_label,
            proxy_down_label,
            direct_hero_label,
            direct_up_label,
            direct_down_label,
            total_rules_label,
            proxy_seg,
            reject_seg,
            direct_seg,
            proxy_legend_label,
            reject_legend_label,
            direct_legend_label,
            app_traffic_search,
            app_traffic_sort,
            app_traffic_list_box,
        }
    }
}

/// 辅助创建单一 KPI 指标卡片单元
fn create_kpi_tile(
    title: &str,
    initial_hero: &str,
) -> (gtk::Box, gtk::Label, gtk::Label, gtk::Label) {
    let tile = gtk::Box::new(gtk::Orientation::Vertical, 6);
    tile.add_css_class("metric-tile");
    tile.set_hexpand(true);

    let title_lbl = gtk::Label::builder()
        .label(title)
        .halign(gtk::Align::Start)
        .css_classes(["metric-title", "dim-label"])
        .build();
    tile.append(&title_lbl);

    let hero_lbl = gtk::Label::builder()
        .label(initial_hero)
        .halign(gtk::Align::Start)
        .css_classes(["metric-hero", "numeric"])
        .build();
    tile.append(&hero_lbl);

    let sub_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);

    let up_box = gtk::Box::new(gtk::Orientation::Horizontal, 3);
    let up_arrow = gtk::Label::builder()
        .label("↑")
        .css_classes(["stat-arrow-up"])
        .build();
    up_box.append(&up_arrow);
    let up_lbl = gtk::Label::builder()
        .label("0 B")
        .halign(gtk::Align::Start)
        .css_classes(["dim-label", "numeric"])
        .build();
    up_box.append(&up_lbl);
    sub_box.append(&up_box);

    let down_box = gtk::Box::new(gtk::Orientation::Horizontal, 3);
    let down_arrow = gtk::Label::builder()
        .label("↓")
        .css_classes(["stat-arrow-down"])
        .build();
    down_box.append(&down_arrow);
    let down_lbl = gtk::Label::builder()
        .label("0 B")
        .halign(gtk::Align::Start)
        .css_classes(["dim-label", "numeric"])
        .build();
    down_box.append(&down_lbl);
    sub_box.append(&down_box);

    tile.append(&sub_box);

    (tile, hero_lbl, up_lbl, down_lbl)
}

/// 辅助创建图例项
fn create_legend_item(dot_class: &str, text: &str) -> (gtk::Box, gtk::Label) {
    let item = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    item.set_valign(gtk::Align::Center);

    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dot.add_css_class("legend-dot");
    dot.add_css_class(dot_class);
    dot.set_valign(gtk::Align::Center);
    item.append(&dot);

    let label = gtk::Label::builder()
        .label(text)
        .css_classes(["numeric", "dim-label"])
        .build();
    item.append(&label);

    (item, label)
}

/// 刷新规则策略分布分段比例条
pub fn refresh_traffic_rule_counts(
    config: &Rc<RefCell<AppConfig>>,
    total_rules_label: &gtk::Label,
    proxy_seg: &gtk::Box,
    reject_seg: &gtk::Box,
    direct_seg: &gtk::Box,
    proxy_legend_label: &gtk::Label,
    reject_legend_label: &gtk::Label,
    direct_legend_label: &gtk::Label,
) {
    let current = config.borrow();
    let imported = crate::imported_rules(&current);
    let custom = crate::custom_rules(&current);
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

    let total = direct_count + proxy_count + reject_count;
    total_rules_label.set_text(&format!("共 {total} 条策略"));

    if total == 0 {
        proxy_seg.set_visible(false);
        reject_seg.set_visible(false);
        direct_seg.set_visible(false);
        proxy_legend_label.set_text("代理: 0 (0.0%)");
        reject_legend_label.set_text("拦截: 0 (0.0%)");
        direct_legend_label.set_text("直连: 0 (0.0%)");
        return;
    }

    let proxy_ratio = proxy_count as f64 / total as f64;
    let reject_ratio = reject_count as f64 / total as f64;
    let direct_ratio = direct_count as f64 / total as f64;

    // 动态调整色块可见性与相对宽度分配 (以 1000 为基准权重)
    let total_width = 1000.0;
    proxy_seg.set_visible(proxy_count > 0);
    reject_seg.set_visible(reject_count > 0);
    direct_seg.set_visible(direct_count > 0);

    if proxy_count > 0 {
        proxy_seg.set_size_request(((proxy_ratio * total_width).round() as i32).max(4), 12);
    }
    if reject_count > 0 {
        reject_seg.set_size_request(((reject_ratio * total_width).round() as i32).max(4), 12);
    }
    if direct_count > 0 {
        direct_seg.set_size_request(((direct_ratio * total_width).round() as i32).max(4), 12);
    }

    proxy_legend_label.set_text(&format!(
        "代理: {proxy_count} ({:.1}%)",
        proxy_ratio * 100.0
    ));
    reject_legend_label.set_text(&format!(
        "拦截: {reject_count} ({:.1}%)",
        reject_ratio * 100.0
    ));
    direct_legend_label.set_text(&format!(
        "直连: {direct_count} ({:.1}%)",
        direct_ratio * 100.0
    ));
}

/// 刷新进程流量列表，关键数据右对齐高亮
pub fn refresh_app_traffic_list(
    app_traffic_data: &Rc<RefCell<Vec<AppTrafficStat>>>,
    app_traffic_search: &gtk::SearchEntry,
    app_traffic_sort: &gtk::DropDown,
    app_traffic_list_box: &gtk::Box,
) {
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
        let total_bytes = item.upload + item.download;
        let row = adw::ActionRow::builder()
            .title(&item.name)
            .subtitle(&format!(
                "↑ {}   ↓ {}",
                format_bytes(item.upload),
                format_bytes(item.download),
            ))
            .build();
        row.add_prefix(&create_app_icon(&item.icon));

        // 右侧加粗高亮总流量数值
        let total_lbl = gtk::Label::builder()
            .label(format_bytes(total_bytes))
            .css_classes(["process-traffic-total", "numeric"])
            .valign(gtk::Align::Center)
            .halign(gtk::Align::End)
            .build();
        row.add_suffix(&total_lbl);

        app_traffic_list_box.append(&row);
    }
}
