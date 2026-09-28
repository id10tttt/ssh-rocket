use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;

use crate::i18n::tr;

pub struct MainWindowWidgets {
    pub window: adw::ApplicationWindow,
    pub sidebar: gtk::Box,
    pub navigation: gtk::ListBox,
    pub connect_nav: gtk::ListBoxRow,
    pub connect_nav_label: gtk::Label,
    pub rules_nav: gtk::ListBoxRow,
    pub rules_nav_label: gtk::Label,
    pub traffic_nav: gtk::ListBoxRow,
    pub traffic_nav_label: gtk::Label,
    pub logs_nav: gtk::ListBoxRow,
    pub logs_nav_label: gtk::Label,
    pub forward_nav: gtk::ListBoxRow,
    pub forward_nav_label: gtk::Label,
    pub settings_nav: gtk::ListBoxRow,
    pub settings_nav_label: gtk::Label,
    pub header: adw::HeaderBar,
    pub back_button: gtk::Button,
    pub page_title: gtk::Label,
    pub add_connection: gtk::Button,
    pub view_stack: gtk::Stack,
    pub bottom_status_dot: gtk::Box,
    pub bottom_status: gtk::Label,
    pub speed_label: gtk::Label,
}

pub fn create_main_window(app: &adw::Application) -> MainWindowWidgets {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("SSH Rocket")
        .default_width(920)
        .default_height(640)
        .build();

    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);

    // 侧边栏
    let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sidebar.set_width_request(200);
    sidebar.add_css_class("sidebar");

    // 应用标题
    let title_box = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    title_box.set_margin_start(18);
    title_box.set_margin_end(18);
    title_box.set_margin_top(18);
    title_box.set_margin_bottom(14);
    let logo_icon = gtk::Image::from_icon_name("ssh-rocket-symbolic");
    logo_icon.set_pixel_size(24);
    title_box.append(&logo_icon);
    let app_title = gtk::Label::new(Some("SSH Rocket"));
    app_title.add_css_class("title-2");
    title_box.append(&app_title);
    sidebar.append(&title_box);

    // 导航列表
    let navigation = gtk::ListBox::new();
    navigation.add_css_class("navigation-sidebar");
    navigation.set_selection_mode(gtk::SelectionMode::Single);
    navigation.set_activate_on_single_click(true);
    navigation.set_vexpand(true);

    let (connect_nav, connect_nav_label) = create_navigation_row("ssh-rocket-connect-symbolic", tr("nav.connect"));
    let (rules_nav, rules_nav_label) = create_navigation_row("ssh-rocket-rules-symbolic", tr("nav.rules"));
    let (traffic_nav, traffic_nav_label) = create_navigation_row("ssh-rocket-traffic-symbolic", tr("nav.traffic"));
    let (logs_nav, logs_nav_label) = create_navigation_row("ssh-rocket-logs-symbolic", tr("nav.logs"));
    let (forward_nav, forward_nav_label) = create_navigation_row("network-transmit-receive-symbolic", tr("nav.forward"));
    let (settings_nav, settings_nav_label) = create_navigation_row("preferences-system-symbolic", tr("nav.settings"));

    navigation.append(&connect_nav);
    navigation.append(&rules_nav);
    navigation.append(&traffic_nav);
    navigation.append(&logs_nav);
    navigation.append(&forward_nav);
    navigation.append(&settings_nav);
    sidebar.append(&navigation);
    root.append(&sidebar);
    root.append(&gtk::Separator::new(gtk::Orientation::Vertical));

    // 主内容区
    let toolbar = adw::ToolbarView::new();
    toolbar.set_hexpand(true);

    let header = adw::HeaderBar::new();
    let back_button = gtk::Button::from_icon_name("go-previous-symbolic");
    back_button.add_css_class("flat");
    back_button.set_tooltip_text(Some(tr("btn.back")));
    back_button.set_visible(false);
    header.pack_start(&back_button);

    let page_title = gtk::Label::new(Some(tr("nav.connect")));
    page_title.add_css_class("title-3");
    header.set_title_widget(Some(&page_title));

    let add_connection = gtk::Button::from_icon_name("list-add-symbolic");
    add_connection.add_css_class("flat");
    add_connection.set_tooltip_text(Some(tr("btn.add_connection")));
    header.pack_end(&add_connection);
    toolbar.add_top_bar(&header);

    let view_stack = gtk::Stack::new();
    view_stack.set_hexpand(true);
    view_stack.set_vexpand(true);

    // 底部全局状态栏
    let bottom_bar = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    bottom_bar.set_margin_start(16);
    bottom_bar.set_margin_end(16);
    bottom_bar.set_margin_top(8);
    bottom_bar.set_margin_bottom(8);

    let bottom_status_dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    bottom_status_dot.add_css_class("status-dot");
    bottom_status_dot.add_css_class("status-dot-disconnected");
    bottom_status_dot.set_valign(gtk::Align::Center);
    bottom_bar.append(&bottom_status_dot);

    let bottom_status = gtk::Label::new(Some(tr("status.disconnected")));
    bottom_status.add_css_class("dim-label");
    bottom_status.set_halign(gtk::Align::Start);
    bottom_status.set_hexpand(true);
    bottom_bar.append(&bottom_status);

    let speed_label = gtk::Label::new(Some("↑ 0 B/s   ↓ 0 B/s"));
    speed_label.add_css_class("dim-label");
    speed_label.add_css_class("numeric");
    speed_label.set_halign(gtk::Align::End);
    bottom_bar.append(&speed_label);

    toolbar.set_content(Some(&view_stack));
    toolbar.add_bottom_bar(&bottom_bar);
    root.append(&toolbar);

    window.set_content(Some(&root));

    MainWindowWidgets {
        window,
        sidebar,
        navigation,
        connect_nav,
        connect_nav_label,
        rules_nav,
        rules_nav_label,
        traffic_nav,
        traffic_nav_label,
        logs_nav,
        logs_nav_label,
        forward_nav,
        forward_nav_label,
        settings_nav,
        settings_nav_label,
        header,
        back_button,
        page_title,
        add_connection,
        view_stack,
        bottom_status_dot,
        bottom_status,
        speed_label,
    }
}

fn create_navigation_row(icon_name: &str, title: &str) -> (gtk::ListBoxRow, gtk::Label) {
    let row = gtk::ListBoxRow::new();
    row.set_height_request(44);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content.set_margin_start(14);
    content.set_margin_end(14);
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    let icon = gtk::Image::from_icon_name(icon_name);
    icon.set_pixel_size(18);
    content.append(&icon);
    let label = gtk::Label::new(Some(title));
    label.set_halign(gtk::Align::Start);
    label.set_hexpand(true);
    content.append(&label);
    row.set_child(Some(&content));
    (row, label)
}
