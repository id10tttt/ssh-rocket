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

#[derive(Clone)]
pub struct TrafficView {
    pub page: gtk::ScrolledWindow,

    // 1. 会话与传输总览
    pub overview_group: adw::PreferencesGroup,
    pub total_hero_label: gtk::Label,
    pub total_up_label: gtk::Label,
    pub total_down_label: gtk::Label,
    pub proxy_hero_label: gtk::Label,
    pub proxy_up_label: gtk::Label,
    pub proxy_down_label: gtk::Label,
    pub direct_hero_label: gtk::Label,
    pub direct_up_label: gtk::Label,
    pub direct_down_label: gtk::Label,

    // 2. 规则策略横向分段比例条 (自适应 DrawingArea)
    pub total_rules_label: gtk::Label,
    pub distribution_data: Rc<RefCell<(f64, f64, f64)>>,
    pub distribution_area: gtk::DrawingArea,
    pub proxy_legend_label: gtk::Label,
    pub reject_legend_label: gtk::Label,
    pub direct_legend_label: gtk::Label,

    // 3. 进程流量统计
    pub app_traffic_search: gtk::SearchEntry,
    pub app_traffic_sort: gtk::DropDown,
    pub app_traffic_list_box: gtk::Box,
}

impl TrafficView {
    pub fn update_session_subtitle(&self, duration: &str, started: &str) {
        if started == "—" || started.is_empty() {
            self.overview_group.set_description(Some("未连接"));
        } else {
            self.overview_group.set_description(Some(&format!(
                "连接时长: {duration} · 始于 {started}"
            )));
        }
    }

    pub fn new() -> Self {
        let traffic_page = adw::PreferencesPage::new();

        // --- 1. 会话与传输总览 (3 列纯流量 KPI 面板，连接信息置于小标题) ---
        let overview_group = adw::PreferencesGroup::builder()
            .title("会话与传输总览")
            .description("未连接")
            .build();

        let overview_card = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        overview_card.add_css_class("card");
        overview_card.set_homogeneous(true);

        // 列 1: 总传输量
        let (tile_total, total_hero_label, total_up_label, total_down_label) =
            create_kpi_tile("总传输量", "0 B");
        overview_card.append(&tile_total);

        // 列 2: 代理流量
        let (tile_proxy, proxy_hero_label, proxy_up_label, proxy_down_label) =
            create_kpi_tile("代理流量", "0 B");
        overview_card.append(&tile_proxy);

        // 列 3: 直连流量
        let (tile_direct, direct_hero_label, direct_up_label, direct_down_label) =
            create_kpi_tile("直连流量", "0 B");
        overview_card.append(&tile_direct);

        overview_group.add(&overview_card);
        traffic_page.add(&overview_group);

        // --- 2. 规则策略分布 (自适应分段比例条) ---
        let distribution_group = adw::PreferencesGroup::builder()
            .title("规则策略分布")
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

        // 横向彩色自适应分段条 (DrawingArea 弹性绘制，随窗口宽度自适应)
        let distribution_data = Rc::new(RefCell::new((0.0f64, 0.0f64, 0.0f64)));
        let distribution_area = gtk::DrawingArea::builder()
            .content_height(12)
            .hexpand(true)
            .build();

        let draw_data = distribution_data.clone();
        distribution_area.set_draw_func(move |_area, cr, width, height| {
            let (proxy_ratio, reject_ratio, direct_ratio) = *draw_data.borrow();
            let total_ratio = proxy_ratio + reject_ratio + direct_ratio;
            let w = width as f64;
            let h = height as f64;
            if w <= 0.0 || h <= 0.0 {
                return;
            }

            // 圆角剪裁 (半径 6px)
            let r = 6.0f64.min(h / 2.0).min(w / 2.0);
            cr.new_sub_path();
            cr.arc(w - r, r, r, -std::f64::consts::FRAC_PI_2, 0.0);
            cr.arc(w - r, h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
            cr.arc(r, h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
            cr.arc(r, r, r, std::f64::consts::PI, 3.0 * std::f64::consts::FRAC_PI_2);
            cr.close_path();
            let _ = cr.clip();

            // 轨道底色
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.08);
            let _ = cr.paint();

            if total_ratio <= 0.0 {
                return;
            }

            let proxy_w = (w * proxy_ratio).round();
            let reject_w = (w * reject_ratio).round();
            let direct_w = (w - proxy_w - reject_w).max(0.0);

            let mut current_x = 0.0;

            // 代理绿: #2ec27e
            if proxy_w > 0.0 {
                cr.set_source_rgb(0.18, 0.76, 0.49);
                cr.rectangle(current_x, 0.0, proxy_w, h);
                let _ = cr.fill();
                current_x += proxy_w;
            }

            // 拦截红: #e01b24
            if reject_w > 0.0 {
                cr.set_source_rgb(0.88, 0.11, 0.14);
                cr.rectangle(current_x, 0.0, reject_w, h);
                let _ = cr.fill();
                current_x += reject_w;
            }

            // 直连蓝: #3584e4
            if direct_w > 0.0 {
                cr.set_source_rgb(0.21, 0.52, 0.89);
                cr.rectangle(current_x, 0.0, direct_w, h);
                let _ = cr.fill();
            }
        });

        dist_content.append(&distribution_area);

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
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();

        Self {
            page: scroller,
            overview_group,
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
            distribution_data,
            distribution_area,
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
        *distribution_data.borrow_mut() = (0.0, 0.0, 0.0);
        distribution_area.queue_draw();
        proxy_legend_label.set_text("代理: 0 (0.0%)");
        reject_legend_label.set_text("拦截: 0 (0.0%)");
        direct_legend_label.set_text("直连: 0 (0.0%)");
        return;
    }

    let proxy_ratio = proxy_count as f64 / total as f64;
    let reject_ratio = reject_count as f64 / total as f64;
    let direct_ratio = direct_count as f64 / total as f64;

    *distribution_data.borrow_mut() = (proxy_ratio, reject_ratio, direct_ratio);
    distribution_area.queue_draw();

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
