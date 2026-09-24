use adw::prelude::*;
use gtk4::{self as gtk, glib};
use libadwaita as adw;
use ssh_rocket_core::{AppConfig, RuleAction};
use std::{cell::RefCell, collections::VecDeque, rc::Rc, time::Instant};

use crate::{
    i18n::tr,
    scan_desktop_apps,
    ui::widgets::{create_app_icon, format_bytes, format_speed},
    ActiveConnectionStat, AppTrafficStat,
};

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
        let overview_box = gtk::Box::new(gtk::Orientation::Vertical, 16);
        overview_box.set_margin_start(18);
        overview_box.set_margin_end(18);
        overview_box.set_margin_top(12);
        overview_box.set_margin_bottom(18);

        // 1.1 会话与传输总览卡片 (3 列 KPI)
        let overview_group = adw::PreferencesGroup::builder()
            .title(tr("traffic.overview.title"))
            .description(tr("status.disconnected"))
            .build();

        let overview_card = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        overview_card.add_css_class("card");
        overview_card.set_homogeneous(true);

        let (tile_total, total_title_label, total_hero_label, total_up_label, total_down_label) =
            create_kpi_tile(tr("traffic.overview.total"), "0 B");
        overview_card.append(&tile_total);

        let (tile_proxy, proxy_title_label, proxy_hero_label, proxy_up_label, proxy_down_label) =
            create_kpi_tile(tr("traffic.overview.proxy"), "0 B");
        overview_card.append(&tile_proxy);

        let (tile_direct, direct_title_label, direct_hero_label, direct_up_label, direct_down_label) =
            create_kpi_tile(tr("traffic.overview.direct"), "0 B");
        overview_card.append(&tile_direct);

        overview_group.add(&overview_card);
        overview_box.append(&overview_group);

        // 1.2 Speed
        let speed_group = adw::PreferencesGroup::builder()
            .title(tr("traffic.speed.title"))
            .build();

        let speed_current_label = gtk::Label::builder()
            .label("↓ 0 B/s   ↑ 0 B/s")
            .css_classes(["dim-label", "numeric"])
            .build();
        speed_group.set_header_suffix(Some(&speed_current_label));

        let speed_card = gtk::Box::new(gtk::Orientation::Vertical, 10);
        speed_card.add_css_class("card");
        speed_card.set_margin_top(4);
        speed_card.set_margin_bottom(4);
        speed_card.set_margin_start(4);
        speed_card.set_margin_end(4);

        let speed_history = Rc::new(RefCell::new(VecDeque::<(Instant, u64, u64)>::with_capacity(60)));
        let display_peak = Rc::new(RefCell::new(1024.0_f64));

        let speed_drawing_area = gtk::DrawingArea::builder()
            .content_height(185)
            .hexpand(true)
            .build();

        speed_drawing_area.add_tick_callback(|area, _| {
            if area.is_mapped() {
                area.queue_draw();
            }
            gtk::glib::ControlFlow::Continue
        });

        let draw_speed_history = speed_history.clone();
        let draw_display_peak = display_peak.clone();
        speed_drawing_area.set_draw_func(move |area, cr, width, height| {
            if !area.is_mapped() {
                return;
            }
            let w = width as f64;
            let h = height as f64;
            if w <= 0.0 || h <= 0.0 {
                return;
            }

            let is_dark = adw::StyleManager::default().is_dark();

            // 卡片背景微底色自适应
            let bg_c = if is_dark { 1.0 } else { 0.0 };
            let bg_alpha = if is_dark { 0.015 } else { 0.02 };
            cr.set_source_rgba(bg_c, bg_c, bg_c, bg_alpha);
            let _ = cr.paint();

            // 坐标与内边距定义：右侧预留 66px 显示速率 Y 轴，底部预留 22px 显示时间 X 轴
            let pad_left = 10.0;
            let pad_right = 66.0;
            let pad_top = 16.0;
            let pad_bottom = 22.0;
            let chart_x0 = pad_left;
            let chart_x1 = (w - pad_right).max(chart_x0 + 40.0);
            let chart_y0 = pad_top;
            let chart_y1 = (h - pad_bottom).max(chart_y0 + 20.0);
            let chart_w = chart_x1 - chart_x0;
            let chart_h = chart_y1 - chart_y0;

            let history = draw_speed_history.borrow();
            let now = Instant::now();
            let window_sec = 40.0_f64;

            // 1. 计算时间窗口内的最高速率，并规范化量程刻度
            let mut max_speed = 1024.0_f64;
            for (t, u, d) in history.iter() {
                let dt = now.checked_duration_since(*t).map(|dur| dur.as_secs_f64()).unwrap_or(0.0);
                if dt <= window_sec + 2.0 {
                    max_speed = max_speed.max(*u as f64).max(*d as f64);
                }
            }

            let target_peak = if max_speed <= 100.0 * 1024.0 {
                let step = 20.0 * 1024.0;
                ((max_speed / step).ceil() * step).max(step)
            } else if max_speed <= 1024.0 * 1024.0 {
                let step = 100.0 * 1024.0;
                (max_speed / step).ceil() * step
            } else if max_speed <= 10.0 * 1024.0 * 1024.0 {
                let step = 1024.0 * 1024.0;
                (max_speed / step).ceil() * step
            } else {
                let step = 5.0 * 1024.0 * 1024.0;
                (max_speed / step).ceil() * step
            };

            let mut current_peak = *draw_display_peak.borrow();
            current_peak += (target_peak - current_peak) * 0.08;
            if (current_peak - target_peak).abs() < 200.0 {
                current_peak = target_peak;
            }
            *draw_display_peak.borrow_mut() = current_peak;
            let peak_f = current_peak.max(1024.0);

            cr.set_font_size(9.5);

            // 主题自适应颜色
            let line_c = if is_dark { 1.0 } else { 0.0 };
            let grid_alpha = if is_dark { 0.06 } else { 0.08 };
            let text_alpha = if is_dark { 0.40 } else { 0.60 };
            let border_alpha = if is_dark { 0.09 } else { 0.12 };
            let xtick_alpha = if is_dark { 0.04 } else { 0.06 };

            // 2. 绘制水平网格线与 Y 轴刻度 (100%、50%、0%)
            let y_steps = [
                (0.0, peak_f),
                (0.5, peak_f * 0.5),
                (1.0, 0.0),
            ];
            for (ratio, val) in y_steps {
                let y = chart_y0 + chart_h * ratio;

                cr.set_source_rgba(line_c, line_c, line_c, grid_alpha);
                cr.set_line_width(1.0);
                cr.move_to(chart_x0, y);
                cr.line_to(chart_x1, y);
                let _ = cr.stroke();

                let label = format_speed(val.round() as u64);
                cr.set_source_rgba(line_c, line_c, line_c, text_alpha);
                cr.move_to(chart_x1 + 6.0, y + 3.5);
                let _ = cr.show_text(&label);
            }

            // 3. 绘制垂直网格线与 X 轴时间刻度 (0s, -10s, -20s, -30s, -40s)
            let now_label = tr("traffic.speed.now");
            let x_ticks = [
                (0.0, now_label),
                (10.0, "-10s"),
                (20.0, "-20s"),
                (30.0, "-30s"),
                (40.0, "-40s"),
            ];
            for (t_sec, label) in x_ticks {
                let x = chart_x1 - (t_sec / window_sec) * chart_w;

                cr.set_source_rgba(line_c, line_c, line_c, xtick_alpha);
                cr.set_line_width(1.0);
                cr.move_to(x, chart_y0);
                cr.line_to(x, chart_y1);
                let _ = cr.stroke();

                cr.set_source_rgba(line_c, line_c, line_c, text_alpha);
                let text_w = cr.text_extents(label).map(|e| e.width()).unwrap_or(20.0);
                let tx = (x - text_w / 2.0).clamp(0.0, w - text_w - 2.0);
                cr.move_to(tx, chart_y1 + 15.0);
                let _ = cr.show_text(label);
            }

            // 边框
            cr.set_source_rgba(line_c, line_c, line_c, border_alpha);
            cr.set_line_width(1.0);
            cr.rectangle(chart_x0, chart_y0, chart_w, chart_h);
            let _ = cr.stroke();

            if history.is_empty() {
                return;
            }

            // 4. 收集各采样点在当前画面的实时线性连续坐标
            let mut points: Vec<(f64, f64, f64)> = Vec::with_capacity(history.len() + 2);
            for (t, up, down) in history.iter() {
                let dt = now.checked_duration_since(*t).map(|dur| dur.as_secs_f64()).unwrap_or(0.0);
                if dt > window_sec + 5.0 {
                    continue;
                }
                let x = chart_x1 - (dt / window_sec) * chart_w;
                let y_down = chart_y1 - ((*down as f64 / peak_f) * chart_h).clamp(0.0, chart_h);
                let y_up = chart_y1 - ((*up as f64 / peak_f) * chart_h).clamp(0.0, chart_h);
                points.push((x, y_down, y_up));
            }

            if points.is_empty() {
                return;
            }

            // 按 X 从左到右升序排序
            points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

            // 连接最新点到右侧当前边缘
            if let Some(&(_last_x, last_y_down, last_y_up)) = points.last() {
                points.push((chart_x1, last_y_down, last_y_up));
            }

            // 裁剪至图表绘图区
            let _ = cr.save();
            cr.rectangle(chart_x0, chart_y0, chart_w, chart_h);
            let _ = cr.clip();

            // 平滑贝塞尔曲线绘制闭包
            let draw_wave = |cr: &gtk::cairo::Context, is_down: bool| {
                let pts: Vec<(f64, f64)> = points
                    .iter()
                    .map(|(x, yd, yu)| (*x, if is_down { *yd } else { *yu }))
                    .collect();

                if pts.is_empty() {
                    return;
                }

                let first = pts[0];
                let last = *pts.last().unwrap();

                // 填充渐变/半透明区域
                cr.new_path();
                cr.move_to(first.0, chart_y1);
                cr.line_to(first.0, first.1);
                for i in 0..(pts.len() - 1) {
                    let p0 = pts[i];
                    let p1 = pts[i + 1];
                    let dx = p1.0 - p0.0;
                    let cp1_x = p0.0 + dx * 0.45;
                    let cp1_y = p0.1;
                    let cp2_x = p1.0 - dx * 0.45;
                    let cp2_y = p1.1;
                    cr.curve_to(cp1_x, cp1_y, cp2_x, cp2_y, p1.0, p1.1);
                }
                cr.line_to(last.0, chart_y1);
                cr.close_path();

                if is_down {
                    cr.set_source_rgba(0.18, 0.76, 0.49, 0.18);
                } else {
                    cr.set_source_rgba(0.88, 0.11, 0.14, 0.18);
                }
                let _ = cr.fill();

                // 描边主曲线
                cr.new_path();
                cr.move_to(first.0, first.1);
                for i in 0..(pts.len() - 1) {
                    let p0 = pts[i];
                    let p1 = pts[i + 1];
                    let dx = p1.0 - p0.0;
                    let cp1_x = p0.0 + dx * 0.45;
                    let cp1_y = p0.1;
                    let cp2_x = p1.0 - dx * 0.45;
                    let cp2_y = p1.1;
                    cr.curve_to(cp1_x, cp1_y, cp2_x, cp2_y, p1.0, p1.1);
                }

                if is_down {
                    cr.set_source_rgb(0.18, 0.76, 0.49);
                } else {
                    cr.set_source_rgb(0.88, 0.11, 0.14);
                }
                cr.set_line_width(2.0);
                let _ = cr.stroke();
            };

            // 绘制下载曲线 (绿) 与上传曲线 (红)
            draw_wave(cr, true);
            draw_wave(cr, false);

            let _ = cr.restore();
        });

        speed_card.append(&speed_drawing_area);

        let speed_legend_box = gtk::Box::new(gtk::Orientation::Horizontal, 20);
        speed_legend_box.set_margin_start(16);
        speed_legend_box.set_margin_end(16);
        speed_legend_box.set_margin_bottom(12);
        speed_legend_box.set_halign(gtk::Align::End);

        let (legend_down_item, speed_legend_down) = create_legend_item("distribution-seg-proxy", tr("traffic.speed.download"));
        speed_legend_box.append(&legend_down_item);

        let (legend_up_item, speed_legend_up) = create_legend_item("distribution-seg-reject", tr("traffic.speed.upload"));
        speed_legend_box.append(&legend_up_item);

        speed_card.append(&speed_legend_box);
        speed_group.add(&speed_card);
        overview_box.append(&speed_group);

        let overview_scroller = gtk::ScrolledWindow::builder()
            .child(&overview_box)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        stack.add_titled(&overview_scroller, Some("overview"), tr("traffic.tab.overview"));

        // ==========================================
        // Tab 2: 应用统计 (App Traffic)
        // ==========================================
        let apps_box = gtk::Box::new(gtk::Orientation::Vertical, 16);
        apps_box.set_margin_start(18);
        apps_box.set_margin_end(18);
        apps_box.set_margin_top(12);
        apps_box.set_margin_bottom(18);

        let app_usage_group = adw::PreferencesGroup::builder()
            .title(tr("traffic.apps.title"))
            .build();

        let app_traffic_toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        app_traffic_toolbar.set_margin_bottom(8);

        let app_traffic_search = gtk::SearchEntry::builder()
            .placeholder_text(tr("traffic.apps.search"))
            .hexpand(true)
            .build();
        app_traffic_toolbar.append(&app_traffic_search);

        let traffic_scope_filter =
            gtk::DropDown::from_strings(&[
                tr("traffic.apps.filter.all"),
                tr("traffic.apps.filter.proxy"),
                tr("traffic.apps.filter.direct"),
            ]);
        app_traffic_toolbar.append(&traffic_scope_filter);

        let app_traffic_sort = gtk::DropDown::from_strings(&[
            tr("traffic.apps.sort.traffic"),
            tr("traffic.apps.sort.name"),
        ]);
        app_traffic_toolbar.append(&app_traffic_sort);

        app_usage_group.add(&app_traffic_toolbar);

        let app_traffic_list_box = gtk::ListBox::new();
        app_traffic_list_box.add_css_class("boxed-list");
        app_traffic_list_box.set_selection_mode(gtk::SelectionMode::None);
        app_usage_group.add(&app_traffic_list_box);
        apps_box.append(&app_usage_group);

        let apps_scroller = gtk::ScrolledWindow::builder()
            .child(&apps_box)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        stack.add_titled(&apps_scroller, Some("apps"), tr("traffic.tab.apps"));

        // ==========================================
        // Tab 3: 实时连接 (Active Connections) & 规则分布
        // ==========================================
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
        stack.add_titled(&conn_scroller, Some("connections"), tr("traffic.tab.connections"));

        page.append(&stack);

        Self {
            page,
            stack,
            stack_switcher,
            overview_scroller,
            overview_group,
            total_title_label,
            total_hero_label,
            total_up_label,
            total_down_label,
            proxy_title_label,
            proxy_hero_label,
            proxy_up_label,
            proxy_down_label,
            direct_title_label,
            direct_hero_label,
            direct_up_label,
            direct_down_label,
            speed_group,
            speed_history,
            speed_drawing_area,
            speed_current_label,
            speed_legend_down,
            speed_legend_up,
            apps_scroller,
            app_usage_group,
            app_traffic_search,
            traffic_scope_filter,
            app_traffic_sort,
            app_traffic_list_box,
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

/// 辅助创建单一 KPI 指标卡片单元
fn create_kpi_tile(
    title: &str,
    initial_hero: &str,
) -> (gtk::Box, gtk::Label, gtk::Label, gtk::Label, gtk::Label) {
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

    (tile, title_lbl, hero_lbl, up_lbl, down_lbl)
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

/// 刷新进程流量列表（支持全部 / 仅代理 / 本地与直连 范围过滤）
pub fn refresh_app_traffic_list(
    app_traffic_data: &Rc<RefCell<Vec<AppTrafficStat>>>,
    app_traffic_search: &gtk::SearchEntry,
    traffic_scope_filter: &gtk::DropDown,
    app_traffic_sort: &gtk::DropDown,
    app_traffic_list_box: &gtk::ListBox,
) {
    let query = app_traffic_search.text().trim().to_lowercase();
    let scope_mode = traffic_scope_filter.selected(); // 0: 全部, 1: 仅代理, 2: 本地与直连

    let mut items: Vec<AppTrafficStat> = app_traffic_data
        .borrow()
        .iter()
        .filter(|item| {
            let matches_query = query.is_empty()
                || item.name.to_lowercase().contains(&query)
                || item.id.to_lowercase().contains(&query);
            if !matches_query {
                return false;
            }

            match scope_mode {
                1 => item.proxy_upload + item.proxy_download > 0,
                2 => {
                    (item.direct_upload + item.direct_download)
                        + (item.local_upload + item.local_download)
                        > 0
                }
                _ => true,
            }
        })
        .cloned()
        .collect();

    if app_traffic_sort.selected() == 0 {
        items.sort_by(|a, b| {
            let val_a = match scope_mode {
                1 => a.proxy_upload + a.proxy_download,
                2 => (a.direct_upload + a.direct_download) + (a.local_upload + a.local_download),
                _ => a.upload + a.download,
            };
            let val_b = match scope_mode {
                1 => b.proxy_upload + b.proxy_download,
                2 => (b.direct_upload + b.direct_download) + (b.local_upload + b.local_download),
                _ => b.upload + b.download,
            };
            val_b.cmp(&val_a)
        });
    } else {
        items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    }
    items.truncate(80);

    while let Some(child) = app_traffic_list_box.first_child() {
        app_traffic_list_box.remove(&child);
    }

    if items.is_empty() {
        let is_en = crate::i18n::current_language() == ssh_rocket_core::Language::English;
        let empty_row = adw::ActionRow::builder()
            .title(if is_en { "No matching process traffic" } else { "暂无符合条件的进程流量记录" })
            .build();
        app_traffic_list_box.append(&empty_row);
        return;
    }

    for item in items {
        let (up_bytes, down_bytes, total_bytes) = match scope_mode {
            1 => (
                item.proxy_upload,
                item.proxy_download,
                item.proxy_upload + item.proxy_download,
            ),
            2 => {
                let u = item.direct_upload + item.local_upload;
                let d = item.direct_download + item.local_download;
                (u, d, u + d)
            }
            _ => (item.upload, item.download, item.upload + item.download),
        };

        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&item.name))
            .subtitle(&format!(
                "↑ {}   ↓ {}",
                format_bytes(up_bytes),
                format_bytes(down_bytes),
            ))
            .build();
        row.add_prefix(&create_app_icon(&item.icon));

        // 右侧高亮总流量（左列：固定宽度右对齐）
        let total_lbl = gtk::Label::builder()
            .label(format_bytes(total_bytes))
            .css_classes(["process-traffic-total", "numeric"])
            .valign(gtk::Align::Center)
            .halign(gtk::Align::End)
            .width_request(85)
            .build();
        row.add_suffix(&total_lbl);

        // 路由走向胶囊徽标（最右侧：固定宽度居中对齐）
        let badge = gtk::Label::builder()
            .label(item.primary_type.label())
            .css_classes([item.primary_type.badge_class()])
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Center)
            .width_request(58)
            .build();
        row.add_suffix(&badge);

        app_traffic_list_box.append(&row);
    }
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
