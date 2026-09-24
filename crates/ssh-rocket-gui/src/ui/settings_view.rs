use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use ssh_rocket_core::{AppConfig, Language, ThemeMode};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use crate::i18n::tr;

pub struct SettingsView {
    pub container: gtk::ScrolledWindow,
    pub appearance_group: adw::PreferencesGroup,
    pub theme_combo: adw::ComboRow,
    pub language_group: adw::PreferencesGroup,
    pub lang_combo: adw::ComboRow,
    pub tools_group: adw::PreferencesGroup,
    pub floating_switch: adw::SwitchRow,
    pub opacity_spin: adw::SpinRow,
    pub delay_spin: adw::SpinRow,
    pub decimals_spin: adw::SpinRow,
    pub config: Rc<RefCell<AppConfig>>,
    pub is_updating: Rc<Cell<bool>>,
}

impl SettingsView {
    pub fn new(
        config: &Rc<RefCell<AppConfig>>,
        on_theme_changed: Rc<dyn Fn(ThemeMode)>,
        on_lang_changed: Rc<dyn Fn(Language)>,
        on_floating_changed: Rc<dyn Fn()>,
    ) -> Self {
        let is_updating = Rc::new(Cell::new(false));

        let main_box = gtk::Box::new(gtk::Orientation::Vertical, 18);
        main_box.set_margin_start(24);
        main_box.set_margin_end(24);
        main_box.set_margin_top(24);
        main_box.set_margin_bottom(24);

        // 左右布局容器 (外观 / 语言)
        let columns_box = gtk::Box::new(gtk::Orientation::Horizontal, 18);
        columns_box.set_homogeneous(true);

        // 1. 外观与主题 (左侧)
        let appearance_group = adw::PreferencesGroup::builder()
            .title(tr("settings.appearance.group"))
            .build();

        let theme_model = gtk::StringList::new(&[
            tr("settings.theme.auto"),
            tr("settings.theme.light"),
            tr("settings.theme.dark"),
        ]);

        let current_theme = config.borrow().settings.theme_mode;
        let selected_theme_idx = match current_theme {
            ThemeMode::Auto => 0,
            ThemeMode::Light => 1,
            ThemeMode::Dark => 2,
        };

        let theme_icon = gtk::Image::from_icon_name("weather-clear-night-symbolic");
        theme_icon.set_pixel_size(18);

        let theme_combo = adw::ComboRow::builder()
            .title(tr("settings.theme.title"))
            .model(&theme_model)
            .selected(selected_theme_idx)
            .build();
        theme_combo.add_prefix(&theme_icon);

        {
            let config = config.clone();
            let on_theme_changed = on_theme_changed.clone();
            let is_updating = is_updating.clone();
            theme_combo.connect_selected_notify(move |row| {
                if is_updating.get() {
                    return;
                }
                let mode = match row.selected() {
                    1 => ThemeMode::Light,
                    2 => ThemeMode::Dark,
                    _ => ThemeMode::Auto,
                };
                if config.borrow().settings.theme_mode == mode {
                    return;
                }
                config.borrow_mut().settings.theme_mode = mode;
                let _ = config.borrow().save();
                on_theme_changed(mode);
            });
        }

        appearance_group.add(&theme_combo);
        columns_box.append(&appearance_group);

        // 2. 语言与区域 (右侧)
        let language_group = adw::PreferencesGroup::builder()
            .title(tr("settings.language.group"))
            .build();

        let lang_model = gtk::StringList::new(&[
            tr("settings.language.auto"),
            tr("settings.language.zh"),
            tr("settings.language.en"),
        ]);

        let current_lang = config.borrow().settings.language;
        let selected_lang_idx = match current_lang {
            Language::Auto => 0,
            Language::Chinese => 1,
            Language::English => 2,
        };

        let lang_icon = gtk::Image::from_icon_name("preferences-desktop-locale-symbolic");
        lang_icon.set_pixel_size(18);

        let lang_combo = adw::ComboRow::builder()
            .title(tr("settings.language.title"))
            .model(&lang_model)
            .selected(selected_lang_idx)
            .build();
        lang_combo.add_prefix(&lang_icon);

        {
            let config = config.clone();
            let on_lang_changed = on_lang_changed.clone();
            let is_updating = is_updating.clone();
            lang_combo.connect_selected_notify(move |row| {
                if is_updating.get() {
                    return;
                }
                let lang = match row.selected() {
                    1 => Language::Chinese,
                    2 => Language::English,
                    _ => Language::Auto,
                };
                if config.borrow().settings.language == lang {
                    return;
                }
                config.borrow_mut().settings.language = lang;
                let _ = config.borrow().save();
                on_lang_changed(lang);
            });
        }

        language_group.add(&lang_combo);
        columns_box.append(&language_group);
        main_box.append(&columns_box);

        // 3. 小工具插件 (Tools & Plugins)
        let tools_group = adw::PreferencesGroup::builder()
            .title(tr("settings.tools.group"))
            .description(tr("settings.tools.desc"))
            .build();

        let floating_cfg = config.borrow().settings.floating_widget.clone();

        let floating_icon = gtk::Image::from_icon_name("utilities-system-monitor-symbolic");
        floating_icon.set_pixel_size(18);

        let floating_switch = adw::SwitchRow::builder()
            .title(tr("settings.floating.title"))
            .subtitle(tr("settings.floating.subtitle"))
            .active(floating_cfg.enabled)
            .build();
        floating_switch.add_prefix(&floating_icon);

        {
            let config = config.clone();
            let on_floating_changed = on_floating_changed.clone();
            let is_updating = is_updating.clone();
            floating_switch.connect_active_notify(move |row| {
                if is_updating.get() {
                    return;
                }
                config.borrow_mut().settings.floating_widget.enabled = row.is_active();
                let _ = config.borrow().save();
                on_floating_changed();
            });
        }
        tools_group.add(&floating_switch);

        let opacity_spin = adw::SpinRow::with_range(10.0, 90.0, 5.0);
        opacity_spin.set_title(tr("settings.floating.opacity"));
        opacity_spin.set_subtitle("10% ~ 90%");
        opacity_spin.set_value(floating_cfg.idle_opacity * 100.0);
        {
            let config = config.clone();
            let on_floating_changed = on_floating_changed.clone();
            let is_updating = is_updating.clone();
            opacity_spin.connect_value_notify(move |row| {
                if is_updating.get() {
                    return;
                }
                config.borrow_mut().settings.floating_widget.idle_opacity = row.value() / 100.0;
                let _ = config.borrow().save();
                on_floating_changed();
            });
        }
        tools_group.add(&opacity_spin);

        let delay_spin = adw::SpinRow::with_range(1.0, 30.0, 1.0);
        delay_spin.set_title(tr("settings.floating.delay"));
        delay_spin.set_value(floating_cfg.fade_delay_secs as f64);
        {
            let config = config.clone();
            let on_floating_changed = on_floating_changed.clone();
            let is_updating = is_updating.clone();
            delay_spin.connect_value_notify(move |row| {
                if is_updating.get() {
                    return;
                }
                config.borrow_mut().settings.floating_widget.fade_delay_secs = row.value() as u32;
                let _ = config.borrow().save();
                on_floating_changed();
            });
        }
        tools_group.add(&delay_spin);

        let decimals_spin = adw::SpinRow::with_range(0.0, 3.0, 1.0);
        decimals_spin.set_title(tr("settings.floating.decimals"));
        decimals_spin.set_subtitle(tr("settings.floating.decimals.sub"));
        decimals_spin.set_value(floating_cfg.speed_decimals as f64);
        {
            let config = config.clone();
            let on_floating_changed = on_floating_changed.clone();
            let is_updating = is_updating.clone();
            decimals_spin.connect_value_notify(move |row| {
                if is_updating.get() {
                    return;
                }
                config.borrow_mut().settings.floating_widget.speed_decimals = row.value() as u32;
                let _ = config.borrow().save();
                on_floating_changed();
            });
        }
        tools_group.add(&decimals_spin);

        main_box.append(&tools_group);

        let container = gtk::ScrolledWindow::builder()
            .child(&main_box)
            .vexpand(true)
            .hexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();

        Self {
            container,
            appearance_group,
            theme_combo,
            language_group,
            lang_combo,
            tools_group,
            floating_switch,
            opacity_spin,
            delay_spin,
            decimals_spin,
            config: config.clone(),
            is_updating,
        }
    }

    pub fn refresh_labels(&self) {
        self.is_updating.set(true);

        self.appearance_group.set_title(tr("settings.appearance.group"));
        self.theme_combo.set_title(tr("settings.theme.title"));

        let theme_model = gtk::StringList::new(&[
            tr("settings.theme.auto"),
            tr("settings.theme.light"),
            tr("settings.theme.dark"),
        ]);
        let selected_theme_idx = match self.config.borrow().settings.theme_mode {
            ThemeMode::Auto => 0,
            ThemeMode::Light => 1,
            ThemeMode::Dark => 2,
        };
        self.theme_combo.set_model(Some(&theme_model));
        self.theme_combo.set_selected(selected_theme_idx);

        self.language_group.set_title(tr("settings.language.group"));
        self.lang_combo.set_title(tr("settings.language.title"));

        let lang_model = gtk::StringList::new(&[
            tr("settings.language.auto"),
            tr("settings.language.zh"),
            tr("settings.language.en"),
        ]);
        let selected_lang_idx = match self.config.borrow().settings.language {
            Language::Auto => 0,
            Language::Chinese => 1,
            Language::English => 2,
        };
        self.lang_combo.set_model(Some(&lang_model));
        self.lang_combo.set_selected(selected_lang_idx);

        self.tools_group.set_title(tr("settings.tools.group"));
        self.tools_group.set_description(Some(tr("settings.tools.desc")));
        self.floating_switch.set_title(tr("settings.floating.title"));
        self.floating_switch.set_subtitle(tr("settings.floating.subtitle"));
        self.opacity_spin.set_title(tr("settings.floating.opacity"));
        self.delay_spin.set_title(tr("settings.floating.delay"));
        self.decimals_spin.set_title(tr("settings.floating.decimals"));
        self.decimals_spin.set_subtitle(tr("settings.floating.decimals.sub"));

        self.is_updating.set(false);
    }
}
