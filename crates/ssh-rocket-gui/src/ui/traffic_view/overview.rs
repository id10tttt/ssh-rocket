use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use std::{cell::RefCell, collections::VecDeque, rc::Rc, time::Instant};

use crate::{
    i18n::tr,
    ui::widgets::format_speed,
};

pub struct OverviewWidgets {
    pub scroller: gtk::ScrolledWindow,
    pub group: adw::PreferencesGroup,
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
    pub speed_group: adw::PreferencesGroup,
    pub speed_history: Rc<RefCell<VecDeque<(Instant, u64, u64)>>>,
    pub speed_drawing_area: gtk::DrawingArea,
    pub speed_current_label: gtk::Label,
    pub speed_legend_down: gtk::Label,
    pub speed_legend_up: gtk::Label,
}

pub fn build_overview_tab() -> OverviewWidgets {
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

    OverviewWidgets {
        scroller: overview_scroller,
        group: overview_group,
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
    }
}

/// 辅助创建单一 KPI 指标卡片单元
pub fn create_kpi_tile(
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
pub fn create_legend_item(dot_class: &str, text: &str) -> (gtk::Box, gtk::Label) {
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
