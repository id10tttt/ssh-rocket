use gtk4::{self as gtk, prelude::*};
use ssh_rocket_core::RuleAction;

/// 格式化存储大小为可读字符串
pub fn format_bytes(bytes: u64) -> String {
    let value = bytes as f64;
    if value < 1024.0 {
        format!("{bytes} B")
    } else if value < 1024.0 * 1024.0 {
        format!("{:.1} KB", value / 1024.0)
    } else if value < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", value / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", value / (1024.0 * 1024.0 * 1024.0))
    }
}

/// 格式化传输速率
pub fn format_speed(bytes_per_second: u64) -> String {
    let value = bytes_per_second as f64;
    if value < 1024.0 {
        format!("{value:.0} B/s")
    } else if value < 1024.0 * 1024.0 {
        format!("{:.1} KB/s", value / 1024.0)
    } else if value < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB/s", value / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB/s", value / (1024.0 * 1024.0 * 1024.0))
    }
}

/// 格式化秒数为 HH:MM:SS
pub fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    format!("{hours:02}:{minutes:02}:{secs:02}")
}

/// 创建应用桌面图标
pub fn create_app_icon(icon_name: &str) -> gtk::Image {
    let icon = if !icon_name.is_empty() {
        if icon_name.starts_with('/') {
            gtk::Image::from_file(icon_name)
        } else {
            gtk::Image::from_icon_name(icon_name)
        }
    } else {
        gtk::Image::from_icon_name("application-x-executable-symbolic")
    };
    icon.set_pixel_size(24);
    icon
}

/// 创建分流动作胶囊徽标 (PROXY / DIRECT / REJECT)
pub fn create_action_badge(action: RuleAction) -> gtk::Label {
    let (label_text, css_class) = match action {
        RuleAction::Proxy => ("代理", "badge-proxy"),
        RuleAction::Direct => ("直连", "badge-direct"),
        RuleAction::Block => ("拦截", "badge-reject"),
    };
    let label = gtk::Label::new(Some(label_text));
    label.add_css_class(css_class);
    label
}

/// 创建规则类型徽标 (DOMAIN / IP-CIDR 等)
pub fn create_kind_badge(kind_label: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(kind_label));
    label.add_css_class("badge-kind");
    label.add_css_class("dim-label");
    label
}

/// 构造流量数据列组件
pub fn create_traffic_stat_column(
    title: &str,
    up_label: &gtk::Label,
    down_label: &gtk::Label,
) -> gtk::Box {
    let col = gtk::Box::new(gtk::Orientation::Vertical, 6);
    col.set_margin_start(16);
    col.set_margin_end(16);
    col.set_margin_top(14);
    col.set_margin_bottom(14);

    let title_lbl = gtk::Label::builder()
        .label(title)
        .halign(gtk::Align::Start)
        .css_classes(["dim-label", "heading"])
        .build();
    col.append(&title_lbl);

    let up_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let up_arrow = gtk::Label::builder()
        .label("↑")
        .css_classes(["stat-arrow-up", "heading"])
        .build();
    up_box.append(&up_arrow);
    up_label.set_halign(gtk::Align::Start);
    up_label.add_css_class("numeric");
    up_label.add_css_class("heading");
    up_box.append(up_label);
    col.append(&up_box);

    let down_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let down_arrow = gtk::Label::builder()
        .label("↓")
        .css_classes(["stat-arrow-down", "heading"])
        .build();
    down_box.append(&down_arrow);
    down_label.set_halign(gtk::Align::Start);
    down_label.add_css_class("numeric");
    down_label.add_css_class("heading");
    down_box.append(down_label);
    col.append(&down_box);

    col
}

/// 构造统计柱状图列组件
pub fn create_chart_column(
    title: &str,
    count_label: &gtk::Label,
    fill_box: &gtk::Box,
    fill_class: &str,
) -> gtk::Box {
    let col = gtk::Box::new(gtk::Orientation::Vertical, 6);
    col.set_halign(gtk::Align::Center);
    col.set_margin_top(16);
    col.set_margin_bottom(16);
    col.set_margin_start(12);
    col.set_margin_end(12);

    count_label.add_css_class("title-3");
    count_label.add_css_class("numeric");
    count_label.set_halign(gtk::Align::Center);
    col.append(count_label);

    let track = gtk::Box::new(gtk::Orientation::Vertical, 0);
    track.add_css_class("chart-track");
    track.set_width_request(42);
    track.set_height_request(100);
    track.set_halign(gtk::Align::Center);

    let spacer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    spacer.set_vexpand(true);
    track.append(&spacer);

    fill_box.set_valign(gtk::Align::End);
    fill_box.add_css_class(fill_class);
    fill_box.set_height_request(0);
    track.append(fill_box);
    col.append(&track);

    let title_lbl = gtk::Label::builder()
        .label(title)
        .halign(gtk::Align::Center)
        .css_classes(["heading", "dim-label"])
        .build();
    col.append(&title_lbl);

    col
}
