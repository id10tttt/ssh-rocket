use adw::prelude::*;
use gtk4::{self as gtk, gdk, gio, glib};
use libadwaita as adw;
use ssh_rocket_core::FloatingWidgetConfig;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use crate::{
    i18n::tr,
    sys_monitor::SystemMetrics,
    ui::widgets::format_speed_fixed,
};

#[derive(Clone)]
pub struct FloatingWidget {
    pub window: gtk::Window,
    pub root_box: gtk::Box,
    // 代理
    pub proxy_icon: gtk::Image,
    pub proxy_up_label: gtk::Label,
    pub proxy_down_label: gtk::Label,
    // 直连
    pub direct_icon: gtk::Image,
    pub direct_up_label: gtk::Label,
    pub direct_down_label: gtk::Label,
    // 硬件 (单列 3 行: CPU / RAM / GPU)
    pub cpu_label: gtk::Label,
    pub ram_label: gtk::Label,
    pub gpu_label: gtk::Label,
    // 状态与定时器
    pub idle_opacity: Rc<Cell<f64>>,
    pub fade_delay_secs: Rc<Cell<u32>>,
    pub speed_decimals: Rc<Cell<u32>>,
    fade_source_id: Rc<RefCell<Option<glib::SourceId>>>,
}

impl FloatingWidget {
    pub fn new(
        app: &adw::Application,
        config: &FloatingWidgetConfig,
        on_show_main_window: Rc<dyn Fn()>,
        on_toggle_proxy: Rc<dyn Fn()>,
        on_open_settings: Rc<dyn Fn()>,
    ) -> Self {
        let window = gtk::Window::builder()
            .application(app)
            .title("SSH Rocket Floating HUD")
            .decorated(false)
            .resizable(false)
            .deletable(false)
            .css_classes(["floating-hud-window"])
            .build();

        let root_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        root_box.add_css_class("floating-hud-box");

        let speed_decimals = Rc::new(Cell::new(config.speed_decimals.clamp(0, 3)));
        let decimals = speed_decimals.get();
        let width_chars = (8 + if decimals > 0 { 1 + decimals } else { 0 }) as i32;
        let init_speed = format_speed_fixed(0, decimals as usize);

        // --- 左侧：网络流量区 (代理 / 直连，使用网格布局保证列对齐) ---
        let net_grid = gtk::Grid::builder()
            .column_spacing(8)
            .row_spacing(4)
            .valign(gtk::Align::Center)
            .build();

        // 代理行 (图标 + 上传 + 下载)
        let proxy_icon = gtk::Image::from_icon_name("ssh-rocket-symbolic");
        proxy_icon.set_pixel_size(14);
        proxy_icon.add_css_class("floating-icon-off");
        proxy_icon.set_tooltip_text(Some(tr("action.proxy")));

        let proxy_up_box = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        let proxy_up_arrow = gtk::Label::builder().label("↑").css_classes(["stat-arrow-up"]).build();
        let proxy_up_label = gtk::Label::builder()
            .label(&init_speed)
            .css_classes(["numeric", "floating-stat-num"])
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .width_chars(width_chars)
            .build();
        proxy_up_box.append(&proxy_up_arrow);
        proxy_up_box.append(&proxy_up_label);

        let proxy_down_box = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        let proxy_down_arrow = gtk::Label::builder().label("↓").css_classes(["stat-arrow-down"]).build();
        let proxy_down_label = gtk::Label::builder()
            .label(&init_speed)
            .css_classes(["numeric", "floating-stat-num"])
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .width_chars(width_chars)
            .build();
        proxy_down_box.append(&proxy_down_arrow);
        proxy_down_box.append(&proxy_down_label);

        net_grid.attach(&proxy_icon, 0, 0, 1, 1);
        net_grid.attach(&proxy_up_box, 1, 0, 1, 1);
        net_grid.attach(&proxy_down_box, 2, 0, 1, 1);

        // 直连行 (图标 + 上传 + 下载)
        let direct_icon = gtk::Image::from_icon_name("applications-internet-symbolic");
        direct_icon.set_pixel_size(14);
        direct_icon.add_css_class("floating-icon-direct");
        direct_icon.set_tooltip_text(Some(tr("action.direct")));

        let direct_up_box = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        let direct_up_arrow = gtk::Label::builder().label("↑").css_classes(["stat-arrow-up"]).build();
        let direct_up_label = gtk::Label::builder()
            .label(&init_speed)
            .css_classes(["numeric", "floating-stat-num"])
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .width_chars(width_chars)
            .build();
        direct_up_box.append(&direct_up_arrow);
        direct_up_box.append(&direct_up_label);

        let direct_down_box = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        let direct_down_arrow = gtk::Label::builder().label("↓").css_classes(["stat-arrow-down"]).build();
        let direct_down_label = gtk::Label::builder()
            .label(&init_speed)
            .css_classes(["numeric", "floating-stat-num"])
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .width_chars(width_chars)
            .build();
        direct_down_box.append(&direct_down_arrow);
        direct_down_box.append(&direct_down_label);

        net_grid.attach(&direct_icon, 0, 1, 1, 1);
        net_grid.attach(&direct_up_box, 1, 1, 1, 1);
        net_grid.attach(&direct_down_box, 2, 1, 1, 1);

        root_box.append(&net_grid);

        // 分隔线
        let sep = gtk::Separator::new(gtk::Orientation::Vertical);
        sep.add_css_class("floating-separator");
        root_box.append(&sep);

        // --- 右侧：系统硬件资源监控 (网格布局保证硬件名和数值左对齐) ---
        let sys_grid = gtk::Grid::builder()
            .column_spacing(4)
            .row_spacing(2)
            .valign(gtk::Align::Center)
            .build();

        // 1. CPU
        let cpu_title = gtk::Label::builder().label("CPU").css_classes(["dim-label", "floating-hw-label"]).build();
        let cpu_label = gtk::Label::builder()
            .label("0%")
            .css_classes(["numeric", "floating-hw-val"])
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .width_chars(4)
            .build();
        sys_grid.attach(&cpu_title, 0, 0, 1, 1);
        sys_grid.attach(&cpu_label, 1, 0, 1, 1);

        // 2. RAM
        let ram_title = gtk::Label::builder().label("RAM").css_classes(["dim-label", "floating-hw-label"]).build();
        let ram_label = gtk::Label::builder()
            .label("0%")
            .css_classes(["numeric", "floating-hw-val"])
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .width_chars(4)
            .build();
        sys_grid.attach(&ram_title, 0, 1, 1, 1);
        sys_grid.attach(&ram_label, 1, 1, 1, 1);

        // 3. GPU
        let gpu_title = gtk::Label::builder().label("GPU").css_classes(["dim-label", "floating-hw-label"]).build();
        let gpu_label = gtk::Label::builder()
            .label("0%")
            .css_classes(["numeric", "floating-hw-val"])
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .width_chars(4)
            .build();
        sys_grid.attach(&gpu_title, 0, 2, 1, 1);
        sys_grid.attach(&gpu_label, 1, 2, 1, 1);

        root_box.append(&sys_grid);
        window.set_child(Some(&root_box));

        // --- 交互 1: 原生 Wayland 拖拽移动 (GestureDrag) ---
        let drag = gtk::GestureDrag::new();
        let win_weak = window.downgrade();
        drag.connect_drag_begin(move |gesture, start_x, start_y| {
            if gesture.current_button() != gdk::BUTTON_PRIMARY {
                return;
            }
            if let Some(win) = win_weak.upgrade() {
                if let Some(surface) = win.surface() {
                    if let Some(toplevel) = surface.downcast_ref::<gdk::Toplevel>() {
                        if let Some(device) = gesture.device() {
                            toplevel.begin_move(
                                &device,
                                gdk::BUTTON_PRIMARY as i32,
                                start_x,
                                start_y,
                                gdk::CURRENT_TIME,
                            );
                        }
                    }
                }
            }
        });
        root_box.add_controller(drag);

        // --- 交互 2: 鼠标悬停高亮与离开 5 秒虚化 ---
        let idle_opacity = Rc::new(Cell::new(config.idle_opacity.clamp(0.1, 1.0)));
        let fade_delay_secs = Rc::new(Cell::new(config.fade_delay_secs.max(1)));
        let fade_source_id = Rc::new(RefCell::new(None::<glib::SourceId>));

        let motion = gtk::EventControllerMotion::new();
        {
            let win_weak = window.downgrade();
            let box_weak = root_box.downgrade();
            let fade_source_id = fade_source_id.clone();
            motion.connect_enter(move |_ctrl, _x, _y| {
                if let Some(source) = fade_source_id.borrow_mut().take() {
                    source.remove();
                }
                if let Some(win) = win_weak.upgrade() {
                    win.set_opacity(1.0);
                }
                if let Some(bx) = box_weak.upgrade() {
                    bx.add_css_class("floating-hud-hover");
                }
            });
        }
        {
            let win_weak = window.downgrade();
            let box_weak = root_box.downgrade();
            let fade_source_id = fade_source_id.clone();
            let idle_opacity = idle_opacity.clone();
            let fade_delay_secs = fade_delay_secs.clone();
            motion.connect_leave(move |_ctrl| {
                if let Some(bx) = box_weak.upgrade() {
                    bx.remove_css_class("floating-hud-hover");
                }
                if let Some(source) = fade_source_id.borrow_mut().take() {
                    source.remove();
                }

                let win_weak = win_weak.clone();
                let fade_source_id_clone = fade_source_id.clone();
                let target_opacity = idle_opacity.get();
                let delay = Duration::from_secs(fade_delay_secs.get() as u64);

                let id = glib::timeout_add_local_once(delay, move || {
                    fade_source_id_clone.borrow_mut().take();
                    if let Some(win) = win_weak.upgrade() {
                        win.set_opacity(target_opacity);
                    }
                });
                *fade_source_id.borrow_mut() = Some(id);
            });
        }
        root_box.add_controller(motion);

        // --- 交互 3: 右键菜单 (PopoverMenu) ---
        let menu_model = gio::Menu::new();
        menu_model.append(Some(tr("tray.show_window")), Some("hud.show_main"));
        menu_model.append(Some(tr("connect.btn.connect")), Some("hud.toggle_proxy"));
        menu_model.append(Some(tr("nav.settings")), Some("hud.open_settings"));
        menu_model.append(Some(tr("floating.menu.hide")), Some("hud.hide_hud"));

        let popover = gtk::PopoverMenu::from_model(Some(&menu_model));
        popover.set_parent(&root_box);
        popover.set_has_arrow(false);

        // Actions
        let action_group = gio::SimpleActionGroup::new();
        {
            let on_show = on_show_main_window.clone();
            let action = gio::SimpleAction::new("show_main", None);
            action.connect_activate(move |_, _| on_show());
            action_group.add_action(&action);
        }
        {
            let on_toggle = on_toggle_proxy.clone();
            let action = gio::SimpleAction::new("toggle_proxy", None);
            action.connect_activate(move |_, _| on_toggle());
            action_group.add_action(&action);
        }
        {
            let on_settings = on_open_settings.clone();
            let action = gio::SimpleAction::new("open_settings", None);
            action.connect_activate(move |_, _| on_settings());
            action_group.add_action(&action);
        }
        {
            let win_weak = window.downgrade();
            let action = gio::SimpleAction::new("hide_hud", None);
            action.connect_activate(move |_, _| {
                if let Some(win) = win_weak.upgrade() {
                    win.set_visible(false);
                }
            });
            action_group.add_action(&action);
        }
        root_box.insert_action_group("hud", Some(&action_group));

        let click = gtk::GestureClick::new();
        click.set_button(gdk::BUTTON_SECONDARY);
        let popover_clone = popover.clone();
        click.connect_pressed(move |_, _, x, y| {
            let rect = gdk::Rectangle::new(x as i32, y as i32, 1, 1);
            popover_clone.set_pointing_to(Some(&rect));
            popover_clone.popup();
        });
        root_box.add_controller(click);

        // 双击悬浮球打开主窗口
        let double_click = gtk::GestureClick::new();
        double_click.set_button(gdk::BUTTON_PRIMARY);
        let on_show_main = on_show_main_window.clone();
        double_click.connect_pressed(move |gesture, n_press, _, _| {
            if n_press == 2 {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                on_show_main();
            }
        });
        root_box.add_controller(double_click);

        // 默认初始化透明度
        window.set_opacity(idle_opacity.get());

        Self {
            window,
            root_box,
            proxy_icon,
            proxy_up_label,
            proxy_down_label,
            direct_icon,
            direct_up_label,
            direct_down_label,
            cpu_label,
            ram_label,
            gpu_label,
            idle_opacity,
            fade_delay_secs,
            speed_decimals,
            fade_source_id,
        }
    }

    /// 呈现或隐藏悬浮窗
    pub fn set_shown(&self, shown: bool) {
        if let Some(source) = self.fade_source_id.borrow_mut().take() {
            source.remove();
        }
        self.window.set_visible(shown);
        if shown {
            self.window.present();
            self.window.set_opacity(self.idle_opacity.get());
        }
    }

    /// 更新透明度及网速小数位配置
    pub fn update_config(&self, opacity: f64, fade_delay_secs: u32, speed_decimals: u32) {
        if let Some(source) = self.fade_source_id.borrow_mut().take() {
            source.remove();
        }
        self.idle_opacity.set(opacity.clamp(0.1, 1.0));
        self.fade_delay_secs.set(fade_delay_secs.max(1));
        let dec = speed_decimals.clamp(0, 3);
        self.speed_decimals.set(dec);
        let width_chars = (8 + if dec > 0 { 1 + dec } else { 0 }) as i32;
        self.proxy_up_label.set_width_chars(width_chars);
        self.proxy_down_label.set_width_chars(width_chars);
        self.direct_up_label.set_width_chars(width_chars);
        self.direct_down_label.set_width_chars(width_chars);
        self.window.set_opacity(self.idle_opacity.get());
    }

    /// 刷新所有数据
    pub fn update_stats(
        &self,
        is_proxy_connected: bool,
        proxy_up: u64,
        proxy_down: u64,
        direct_up: u64,
        direct_down: u64,
        metrics: &SystemMetrics,
    ) {
        let dec = self.speed_decimals.get() as usize;
        if is_proxy_connected {
            self.proxy_icon.remove_css_class("floating-icon-off");
            self.proxy_icon.add_css_class("floating-icon-proxy");
            self.proxy_up_label.set_text(&format_speed_fixed(proxy_up, dec));
            self.proxy_down_label.set_text(&format_speed_fixed(proxy_down, dec));
        } else {
            self.proxy_icon.remove_css_class("floating-icon-proxy");
            self.proxy_icon.add_css_class("floating-icon-off");
            let zero = format_speed_fixed(0, dec);
            self.proxy_up_label.set_text(&zero);
            self.proxy_down_label.set_text(&zero);
        }

        self.direct_up_label.set_text(&format_speed_fixed(direct_up, dec));
        self.direct_down_label.set_text(&format_speed_fixed(direct_down, dec));

        self.cpu_label.set_text(&format!("{:.0}%", metrics.cpu_percent));
        self.ram_label.set_text(&format!("{:.0}%", metrics.ram_percent));
        self.gpu_label.set_text(&format!("{:.0}%", metrics.gpu_percent));
    }

    /// 刷新国际化文本
    pub fn refresh_labels(&self) {
        self.proxy_icon.set_tooltip_text(Some(tr("action.proxy")));
        self.direct_icon.set_tooltip_text(Some(tr("action.direct")));
    }
}
