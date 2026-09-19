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
    pub config: Rc<RefCell<AppConfig>>,
    pub is_updating: Rc<Cell<bool>>,
}

impl SettingsView {
    pub fn new(
        config: &Rc<RefCell<AppConfig>>,
        on_theme_changed: Rc<dyn Fn(ThemeMode)>,
        on_lang_changed: Rc<dyn Fn(Language)>,
    ) -> Self {
        let is_updating = Rc::new(Cell::new(false));

        // 左右布局容器
        let columns_box = gtk::Box::new(gtk::Orientation::Horizontal, 18);
        columns_box.set_homogeneous(true);
        columns_box.set_margin_start(24);
        columns_box.set_margin_end(24);
        columns_box.set_margin_top(24);
        columns_box.set_margin_bottom(24);

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

        let container = gtk::ScrolledWindow::builder()
            .child(&columns_box)
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

        self.is_updating.set(false);
    }
}
