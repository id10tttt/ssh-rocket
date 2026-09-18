use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use ssh_rocket_core::{AppConfig, RuleAction};
use std::{cell::RefCell, rc::Rc};

use crate::{
    scan_desktop_apps,
    ui::widgets::{
        create_app_icon, create_chart_column, create_traffic_stat_column, format_bytes,
    },
    AppTrafficStat,
};

pub struct TrafficView {
    pub page: gtk::ScrolledWindow,
    pub started_label: gtk::Label,
    pub duration_label: gtk::Label,
    pub total_up_label: gtk::Label,
    pub total_down_label: gtk::Label,
    pub proxy_up_label: gtk::Label,
    pub proxy_down_label: gtk::Label,
    pub direct_up_label: gtk::Label,
    pub direct_down_label: gtk::Label,
    pub direct_count_label: gtk::Label,
    pub proxy_count_label: gtk::Label,
    pub reject_count_label: gtk::Label,
    pub direct_fill_box: gtk::Box,
    pub proxy_fill_box: gtk::Box,
    pub reject_fill_box: gtk::Box,
    pub app_traffic_search: gtk::SearchEntry,
    pub app_traffic_sort: gtk::DropDown,
    pub app_traffic_list_box: gtk::Box,
}

impl TrafficView {
    pub fn new() -> Self {
        let traffic_page = adw::PreferencesPage::new();

        // 1. 会话节点状态
        let session_group = adw::PreferencesGroup::builder().title("连接会话").build();
        let started_row = adw::ActionRow::builder().title("开始时间").build();
        let started_label = gtk::Label::builder()
            .label("—")
            .css_classes(["dim-label", "numeric"])
            .build();
        started_row.add_suffix(&started_label);
        session_group.add(&started_row);

        let duration_row = adw::ActionRow::builder().title("连接时长").build();
        let duration_label = gtk::Label::builder()
            .label("—")
            .css_classes(["dim-label", "numeric"])
            .build();
        duration_row.add_suffix(&duration_label);
        session_group.add(&duration_row);
        traffic_page.add(&session_group);

        // 2. 实时传输量统计
        let traffic_group = adw::PreferencesGroup::builder().title("传输流量").build();
        let traffic_card = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        traffic_card.add_css_class("card");
        traffic_card.set_homogeneous(true);

        let total_up_label = gtk::Label::builder().label("0 B").build();
        let total_down_label = gtk::Label::builder().label("0 B").build();
        let total_col = create_traffic_stat_column("总计", &total_up_label, &total_down_label);
        traffic_card.append(&total_col);

        traffic_card.append(&gtk::Separator::new(gtk::Orientation::Vertical));

        let proxy_up_label = gtk::Label::builder().label("0 B").build();
        let proxy_down_label = gtk::Label::builder().label("0 B").build();
        let proxy_col = create_traffic_stat_column("代理", &proxy_up_label, &proxy_down_label);
        traffic_card.append(&proxy_col);

        traffic_card.append(&gtk::Separator::new(gtk::Orientation::Vertical));

        let direct_up_label = gtk::Label::builder().label("0 B").build();
        let direct_down_label = gtk::Label::builder().label("0 B").build();
        let direct_col = create_traffic_stat_column("直连", &direct_up_label, &direct_down_label);
        traffic_card.append(&direct_col);

        traffic_group.add(&traffic_card);
        traffic_page.add(&traffic_group);

        // 3. 规则策略分布
        let config_group = adw::PreferencesGroup::builder().title("规则策略分布").build();
        let config_card = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        config_card.add_css_class("card");
        config_card.set_homogeneous(true);

        let direct_count_label = gtk::Label::builder().label("0").build();
        let direct_fill_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let direct_chart_col = create_chart_column(
            "直连",
            &direct_count_label,
            &direct_fill_box,
            "chart-fill-direct",
        );
        config_card.append(&direct_chart_col);

        let proxy_count_label = gtk::Label::builder().label("0").build();
        let proxy_fill_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let proxy_chart_col = create_chart_column(
            "代理",
            &proxy_count_label,
            &proxy_fill_box,
            "chart-fill-proxy",
        );
        config_card.append(&proxy_chart_col);

        let reject_count_label = gtk::Label::builder().label("0").build();
        let reject_fill_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let reject_chart_col = create_chart_column(
            "拦截",
            &reject_count_label,
            &reject_fill_box,
            "chart-fill-reject",
        );
        config_card.append(&reject_chart_col);

        config_group.add(&config_card);
        traffic_page.add(&config_group);

        // 4. 应用流量排行
        let app_usage_group = adw::PreferencesGroup::builder().title("进程流量统计").build();
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
            total_up_label,
            total_down_label,
            proxy_up_label,
            proxy_down_label,
            direct_up_label,
            direct_down_label,
            direct_count_label,
            proxy_count_label,
            reject_count_label,
            direct_fill_box,
            proxy_fill_box,
            reject_fill_box,
            app_traffic_search,
            app_traffic_sort,
            app_traffic_list_box,
        }
    }
}

/// 刷新规则分布图表高度与数字
pub fn refresh_traffic_rule_counts(
    config: &Rc<RefCell<AppConfig>>,
    direct_count_label: &gtk::Label,
    proxy_count_label: &gtk::Label,
    reject_count_label: &gtk::Label,
    direct_fill_box: &gtk::Box,
    proxy_fill_box: &gtk::Box,
    reject_fill_box: &gtk::Box,
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

    direct_count_label.set_text(&direct_count.to_string());
    proxy_count_label.set_text(&proxy_count.to_string());
    reject_count_label.set_text(&reject_count.to_string());

    let max_count = direct_count.max(proxy_count).max(reject_count);
    let calc_height = |count: usize| -> i32 {
        if count == 0 || max_count == 0 {
            0
        } else {
            let ratio = count as f64 / max_count as f64;
            ((ratio * 100.0).round() as i32).clamp(4, 100)
        }
    };

    direct_fill_box.set_height_request(calc_height(direct_count));
    proxy_fill_box.set_height_request(calc_height(proxy_count));
    reject_fill_box.set_height_request(calc_height(reject_count));
}

/// 刷新进程流量列表
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
        let row = adw::ActionRow::builder()
            .title(&item.name)
            .subtitle(&format!(
                "↑ {} · ↓ {} · 合计 {}",
                format_bytes(item.upload),
                format_bytes(item.download),
                format_bytes(item.upload + item.download),
            ))
            .build();
        row.add_prefix(&create_app_icon(&item.icon));
        app_traffic_list_box.append(&row);
    }
}
