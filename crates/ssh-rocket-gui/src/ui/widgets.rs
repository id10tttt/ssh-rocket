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

use std::{cell::RefCell, collections::HashMap};

thread_local! {
    static ICON_CACHE: RefCell<HashMap<String, Option<gtk::gdk::Paintable>>> = RefCell::new(HashMap::new());
}

/// 创建应用桌面图标（带内存缓存，避免主线程重复磁盘 I/O 与解码）
pub fn create_app_icon(icon_name: &str) -> gtk::Image {
    let icon = if !icon_name.is_empty() {
        if icon_name.starts_with('/') {
            let cached_paintable = ICON_CACHE.with(|cache| cache.borrow().get(icon_name).cloned());
            match cached_paintable {
                Some(Some(paintable)) => gtk::Image::from_paintable(Some(&paintable)),
                Some(None) => gtk::Image::from_icon_name("application-x-executable-symbolic"),
                None => {
                    let img = gtk::Image::from_file(icon_name);
                    let paintable = img.paintable();
                    ICON_CACHE.with(|cache| {
                        cache.borrow_mut().insert(icon_name.to_string(), paintable.clone());
                    });
                    if paintable.is_some() {
                        img
                    } else {
                        gtk::Image::from_icon_name("application-x-executable-symbolic")
                    }
                }
            }
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
