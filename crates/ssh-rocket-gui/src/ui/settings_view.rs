use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use ssh_rocket_core::{AppConfig, Language, ThemeMode};
use std::{cell::{Cell, RefCell}, rc::Rc};

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
        let page = adw::PreferencesPage::new();

        // 1. 外观与主题
        let appearance_group = adw::PreferencesGroup::builder()
            .title(tr("settings.appearance.group"))
            .description(tr("settings.appearance.desc"))
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
            .subtitle(tr("settings.theme.subtitle"))
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
        page.add(&appearance_group);

        // 2. 语言与区域
        let language_group = adw::PreferencesGroup::builder()
            .title(tr("settings.language.group"))
            .description(tr("settings.language.desc"))
            .build();

        let lang_model = gtk::StringList::new(&[
            tr("settings.language.zh"),
            tr("settings.language.en"),
        ]);

        let current_lang = config.borrow().settings.language;
        let selected_lang_idx = match current_lang {
            Language::Chinese => 0,
            Language::English => 1,
        };

        let lang_icon = gtk::Image::from_icon_name("preferences-desktop-locale-symbolic");
        lang_icon.set_pixel_size(18);

        let lang_combo = adw::ComboRow::builder()
            .title(tr("settings.language.title"))
            .subtitle(tr("settings.language.subtitle"))
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
                    1 => Language::English,
                    _ => Language::Chinese,
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
        page.add(&language_group);

        let container = gtk::ScrolledWindow::builder()
            .child(&page)
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
        self.appearance_group.set_description(Some(tr("settings.appearance.desc")));
        self.theme_combo.set_title(tr("settings.theme.title"));
        self.theme_combo.set_subtitle(tr("settings.theme.subtitle"));

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
        self.language_group.set_description(Some(tr("settings.language.desc")));
        self.lang_combo.set_title(tr("settings.language.title"));
        self.lang_combo.set_subtitle(tr("settings.language.subtitle"));

        let lang_model = gtk::StringList::new(&[
            tr("settings.language.zh"),
            tr("settings.language.en"),
        ]);
        let selected_lang_idx = match self.config.borrow().settings.language {
            Language::Chinese => 0,
            Language::English => 1,
        };
        self.lang_combo.set_model(Some(&lang_model));
        self.lang_combo.set_selected(selected_lang_idx);

        self.is_updating.set(false);
    }
}
