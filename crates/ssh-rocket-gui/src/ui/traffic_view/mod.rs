pub mod apps;
pub mod connections;
pub mod overview;

use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use std::{cell::RefCell, collections::VecDeque, rc::Rc, time::Instant};

use crate::i18n::tr;

pub use apps::refresh_app_traffic_list;
pub use connections::{refresh_connection_list, refresh_traffic_rule_counts};

#[derive(Clone)]
pub struct TrafficView {
    pub page: gtk::Box,
    pub stack: gtk::Stack,
    pub stack_switcher: gtk::StackSwitcher,

    // --- Tab 1: 监控总览 ---
    pub overview_scroller: gtk::ScrolledWindow,
    pub overview_group: adw::PreferencesGroup,
    pub total_title_label: gtk::Label,
    pub total_hero_label: gtk::Label,
    pub total_up_label: gtk::Label,
    pub total_down_label: gtk::Label,
    pub proxy_title_label: gtk::Label,
    pub proxy_hero_label: gtk::Label,
    pub proxy_up_label: gtk::Label,
    pub proxy_down_label: gtk::Label,
    pub direct_title_label: gtk::Label,
    pub direct_hero_label: gtk::Label,
    pub direct_up_label: gtk::Label,
    pub direct_down_label: gtk::Label,

    // Speed
    pub speed_group: adw::PreferencesGroup,
    pub speed_history: Rc<RefCell<VecDeque<(Instant, u64, u64)>>>,
    pub speed_drawing_area: gtk::DrawingArea,
    pub speed_current_label: gtk::Label,
    pub speed_legend_down: gtk::Label,
    pub speed_legend_up: gtk::Label,

    // --- Tab 2: 应用统计 ---
    pub apps_scroller: gtk::ScrolledWindow,
    pub app_usage_group: adw::PreferencesGroup,
    pub app_traffic_search: gtk::SearchEntry,
    pub traffic_scope_filter: gtk::DropDown,
    pub app_traffic_sort: gtk::DropDown,
    pub app_traffic_list_box: gtk::ListBox,

    // --- Tab 3: 实时连接 & 规则分布 ---
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

impl TrafficView {
    pub fn update_session_subtitle(&self, duration: &str, started: &str) {
        if started == "—" || started.is_empty() {
            self.overview_group.set_description(Some(tr("status.disconnected")));
        } else {
            self.overview_group.set_description(Some(&format!(
                "{}: {duration} · {} {started}",
                tr("traffic.session.duration"),
                tr("traffic.session.started"),
            )));
        }
    }

    pub fn new() -> Self {
        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page.set_vexpand(true);
        page.set_hexpand(true);

        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.set_hexpand(true);
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);

        // 顶部分段切换栏
        let switcher_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        switcher_box.set_margin_top(12);
        switcher_box.set_margin_bottom(8);
        let stack_switcher = gtk::StackSwitcher::new();
        stack_switcher.set_stack(Some(&stack));
        stack_switcher.set_halign(gtk::Align::Center);
        stack_switcher.set_hexpand(true);
        switcher_box.append(&stack_switcher);
        page.append(&switcher_box);

        // ==========================================
        // Tab 1: 监控总览 (Overview)
        // ==========================================
        let overview = overview::build_overview_tab();
        stack.add_titled(&overview.scroller, Some("overview"), tr("traffic.tab.overview"));

        // ==========================================
        // Tab 2: 应用统计 (App Traffic)
        // ==========================================
        let apps = apps::build_apps_tab();
        stack.add_titled(&apps.scroller, Some("apps"), tr("traffic.tab.apps"));

        // ==========================================
        // Tab 3: 实时连接 (Active Connections) & 规则分布
        // ==========================================
        let conns = connections::build_connections_tab();
        stack.add_titled(&conns.conn_scroller, Some("connections"), tr("traffic.tab.connections"));

        page.append(&stack);

        Self {
            page,
            stack,
            stack_switcher,
            overview_scroller: overview.scroller,
            overview_group: overview.group,
            total_title_label: overview.total_title_label,
            total_hero_label: overview.total_hero_label,
            total_up_label: overview.total_up_label,
            total_down_label: overview.total_down_label,
            proxy_title_label: overview.proxy_title_label,
            proxy_hero_label: overview.proxy_hero_label,
            proxy_up_label: overview.proxy_up_label,
            proxy_down_label: overview.proxy_down_label,
            direct_title_label: overview.direct_title_label,
            direct_hero_label: overview.direct_hero_label,
            direct_up_label: overview.direct_up_label,
            direct_down_label: overview.direct_down_label,
            speed_group: overview.speed_group,
            speed_history: overview.speed_history,
            speed_drawing_area: overview.speed_drawing_area,
            speed_current_label: overview.speed_current_label,
            speed_legend_down: overview.speed_legend_down,
            speed_legend_up: overview.speed_legend_up,
            apps_scroller: apps.scroller,
            app_usage_group: apps.app_usage_group,
            app_traffic_search: apps.app_traffic_search,
            traffic_scope_filter: apps.traffic_scope_filter,
            app_traffic_sort: apps.app_traffic_sort,
            app_traffic_list_box: apps.app_traffic_list_box,
            conn_scroller: conns.conn_scroller,
            conn_group: conns.conn_group,
            conn_search: conns.conn_search,
            conn_stats_label: conns.conn_stats_label,
            conn_list_box: conns.conn_list_box,
            distribution_group: conns.distribution_group,
            total_rules_label: conns.total_rules_label,
            distribution_data: conns.distribution_data,
            distribution_area: conns.distribution_area,
            proxy_legend_label: conns.proxy_legend_label,
            reject_legend_label: conns.reject_legend_label,
            direct_legend_label: conns.direct_legend_label,
        }
    }

    pub fn refresh_labels(&self) {
        self.stack.page(&self.overview_scroller).set_title(tr("traffic.tab.overview"));
        self.stack.page(&self.apps_scroller).set_title(tr("traffic.tab.apps"));
        self.stack.page(&self.conn_scroller).set_title(tr("traffic.tab.connections"));

        self.overview_group.set_title(tr("traffic.overview.title"));
        self.total_title_label.set_text(tr("traffic.overview.total"));
        self.proxy_title_label.set_text(tr("traffic.overview.proxy"));
        self.direct_title_label.set_text(tr("traffic.overview.direct"));

        self.speed_group.set_title(tr("traffic.speed.title"));
        self.speed_legend_down.set_text(tr("traffic.speed.download"));
        self.speed_legend_up.set_text(tr("traffic.speed.upload"));

        self.app_usage_group.set_title(tr("traffic.apps.title"));
        self.app_traffic_search.set_placeholder_text(Some(tr("traffic.apps.search")));
        let scope_sel = self.traffic_scope_filter.selected();
        self.traffic_scope_filter.set_model(Some(&gtk::StringList::new(&[
            tr("traffic.apps.filter.all"),
            tr("traffic.apps.filter.proxy"),
            tr("traffic.apps.filter.direct"),
        ])));
        self.traffic_scope_filter.set_selected(scope_sel);

        let sort_sel = self.app_traffic_sort.selected();
        self.app_traffic_sort.set_model(Some(&gtk::StringList::new(&[
            tr("traffic.apps.sort.traffic"),
            tr("traffic.apps.sort.name"),
        ])));
        self.app_traffic_sort.set_selected(sort_sel);

        self.conn_group.set_title(tr("traffic.conn.title"));
        self.conn_search.set_placeholder_text(Some(tr("traffic.conn.search")));
        self.distribution_group.set_title(tr("traffic.conn.rules_dist"));
    }
}

/// (已废弃) 流量构成比已从监控总览页移除以消除冗余并提升视觉聚焦度
#[allow(unused_variables)]
pub fn update_traffic_ratio_display(
    _up_proxy: u64,
    _down_proxy: u64,
    _direct_up: u64,
    _direct_down: u64,
    _ratio_data: &Rc<RefCell<(f64, f64)>>,
    _ratio_area: &gtk::DrawingArea,
    _ratio_proxy_label: &gtk::Label,
    _ratio_direct_label: &gtk::Label,
) {}
