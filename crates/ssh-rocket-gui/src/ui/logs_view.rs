use gtk4::{self as gtk, prelude::*};

pub struct LogsView {
    pub container: gtk::Box,
    pub log_stack: gtk::Stack,
    pub all_log_buffer: gtk::TextBuffer,
    pub all_log_view: gtk::TextView,
    pub system_log_buffer: gtk::TextBuffer,
    pub system_log_view: gtk::TextView,
    pub proxy_log_buffer: gtk::TextBuffer,
    pub proxy_log_view: gtk::TextView,
    pub direct_log_buffer: gtk::TextBuffer,
    pub direct_log_view: gtk::TextView,
    pub copy_logs_btn: gtk::Button,
    pub clear_logs_btn: gtk::Button,
}

impl LogsView {
    pub fn new() -> Self {
        let log_page = gtk::Box::new(gtk::Orientation::Vertical, 12);
        log_page.set_margin_start(18);
        log_page.set_margin_end(18);
        log_page.set_margin_top(18);
        log_page.set_margin_bottom(18);
        log_page.set_hexpand(true);
        log_page.set_vexpand(true);

        let log_header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        log_header.set_margin_bottom(4);

        let log_stack = gtk::Stack::new();
        log_stack.set_vexpand(true);
        log_stack.set_hexpand(true);

        let log_switcher = gtk::StackSwitcher::new();
        log_switcher.set_stack(Some(&log_stack));
        log_switcher.set_halign(gtk::Align::Center);
        log_switcher.set_hexpand(true);
        log_header.append(&log_switcher);

        let copy_logs = gtk::Button::from_icon_name("edit-copy-symbolic");
        copy_logs.add_css_class("flat");
        copy_logs.set_tooltip_text(Some("复制日志"));
        log_header.append(&copy_logs);

        let clear_logs = gtk::Button::from_icon_name("edit-clear-all-symbolic");
        clear_logs.add_css_class("flat");
        clear_logs.set_tooltip_text(Some("清空日志"));
        log_header.append(&clear_logs);

        log_page.append(&log_header);

        // 全部日志
        let all_log_view = gtk::TextView::builder()
            .editable(false)
            .cursor_visible(false)
            .monospace(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .css_classes(["console-view"])
            .build();
        let all_log_buffer = all_log_view.buffer();
        let all_log_scroller = gtk::ScrolledWindow::builder()
            .child(&all_log_view)
            .min_content_height(180)
            .hexpand(true)
            .vexpand(true)
            .build();
        log_stack.add_titled(&all_log_scroller, Some("all"), "全部");

        // 系统日志
        let system_log_view = gtk::TextView::builder()
            .editable(false)
            .cursor_visible(false)
            .monospace(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .css_classes(["console-view"])
            .build();
        let system_log_buffer = system_log_view.buffer();
        let system_log_scroller = gtk::ScrolledWindow::builder()
            .child(&system_log_view)
            .min_content_height(180)
            .hexpand(true)
            .vexpand(true)
            .build();
        log_stack.add_titled(&system_log_scroller, Some("system"), "系统");

        // 代理日志
        let proxy_log_view = gtk::TextView::builder()
            .editable(false)
            .cursor_visible(false)
            .monospace(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .css_classes(["console-view"])
            .build();
        let proxy_log_buffer = proxy_log_view.buffer();
        let proxy_log_scroller = gtk::ScrolledWindow::builder()
            .child(&proxy_log_view)
            .min_content_height(180)
            .hexpand(true)
            .vexpand(true)
            .build();
        log_stack.add_titled(&proxy_log_scroller, Some("proxy"), "代理");

        // 直连日志
        let direct_log_view = gtk::TextView::builder()
            .editable(false)
            .cursor_visible(false)
            .monospace(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .css_classes(["console-view"])
            .build();
        let direct_log_buffer = direct_log_view.buffer();
        let direct_log_scroller = gtk::ScrolledWindow::builder()
            .child(&direct_log_view)
            .min_content_height(180)
            .hexpand(true)
            .vexpand(true)
            .build();
        log_stack.add_titled(&direct_log_scroller, Some("direct"), "直连");

        log_page.append(&log_stack);

        Self {
            container: log_page,
            log_stack,
            all_log_buffer,
            all_log_view,
            system_log_buffer,
            system_log_view,
            proxy_log_buffer,
            proxy_log_view,
            direct_log_buffer,
            direct_log_view,
            copy_logs_btn: copy_logs,
            clear_logs_btn: clear_logs,
        }
    }
}
