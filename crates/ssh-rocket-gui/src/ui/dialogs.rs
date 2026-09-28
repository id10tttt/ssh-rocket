use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use ssh_rocket_core::{parse_rule_set, AppConfig, AuthType, Profile, RuleAction};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};

use crate::{i18n::tr, ListedRule};

pub type RefreshConnections = Rc<RefCell<Option<Rc<dyn Fn()>>>>;
pub type RefreshRules = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// 显示节点连接配置对话框
pub fn show_profile_dialog(
    parent: &adw::ApplicationWindow,
    config: Rc<RefCell<AppConfig>>,
    profile: Option<Profile>,
    refresh: RefreshConnections,
) {
    let editing = profile.is_some();
    let source = profile.unwrap_or_default();
    let dialog = adw::AlertDialog::new(
        Some(if editing { tr("dialog.profile.title_edit") } else { tr("dialog.profile.title_new") }),
        None,
    );
    let group = adw::PreferencesGroup::new();
    let name = adw::EntryRow::builder()
        .title(tr("dialog.profile.name"))
        .text(&source.name)
        .build();
    let host = adw::EntryRow::builder()
        .title(tr("dialog.profile.host"))
        .text(&source.host)
        .build();
    let port = adw::EntryRow::builder()
        .title(tr("dialog.profile.port"))
        .text(source.port.to_string())
        .build();
    let username = adw::EntryRow::builder()
        .title(tr("dialog.profile.username"))
        .text(&source.username)
        .build();

    let auth_type_row = adw::ComboRow::builder()
        .title(tr("dialog.profile.auth_type"))
        .model(&gtk::StringList::new(&[tr("dialog.profile.auth_key"), tr("dialog.profile.auth_password")]))
        .selected(match source.auth_type {
            AuthType::Key => 0,
            AuthType::Password => 1,
        })
        .build();

    // 收集可用 SSH 私钥列表
    let mut discovered = crate::ssh_key::scan_ssh_keys();
    if let Some(existing_path) = &source.identity_file {
        if !existing_path.as_os_str().is_empty() && !discovered.iter().any(|k| &k.path == existing_path) {
            let name = existing_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Custom Key")
                .to_string();
            discovered.insert(0, crate::ssh_key::DiscoveredKey {
                name,
                path: existing_path.clone(),
            });
        }
    }

    let key_items: Rc<RefCell<Vec<(String, Option<PathBuf>)>>> = Rc::new(RefCell::new(
        discovered
            .into_iter()
            .map(|k| (k.name, Some(k.path)))
            .collect(),
    ));

    let string_list = gtk::StringList::new(&[]);
    for (name, _) in key_items.borrow().iter() {
        string_list.append(name);
    }
    string_list.append(tr("dialog.profile.identity_browse"));

    let mut initial_selected: u32 = 0;
    if let Some(target) = &source.identity_file {
        if let Some(idx) = key_items.borrow().iter().position(|(_, p)| p.as_ref() == Some(target)) {
            initial_selected = idx as u32;
        }
    }

    let identity_row = adw::ComboRow::builder()
        .title(tr("dialog.profile.identity"))
        .model(&string_list)
        .selected(initial_selected)
        .build();

    let last_selected = Rc::new(Cell::new(initial_selected));
    let is_updating = Rc::new(Cell::new(false));

    let update_subtitle = {
        let identity_row = identity_row.clone();
        let key_items = key_items.clone();
        Rc::new(move || {
            let sel = identity_row.selected() as usize;
            let items = key_items.borrow();
            if let Some((_, Some(path))) = items.get(sel) {
                identity_row.set_subtitle(&path.to_string_lossy());
            } else {
                identity_row.set_subtitle("");
            }
        })
    };
    update_subtitle();

    let open_file_chooser = {
        let parent = parent.clone();
        let key_items = key_items.clone();
        let string_list = string_list.clone();
        let identity_row = identity_row.clone();
        let last_selected = last_selected.clone();
        let is_updating = is_updating.clone();
        Rc::new(move || {
            let file_dialog = gtk::FileDialog::builder()
                .title(tr("dialog.profile.identity_dialog"))
                .modal(true)
                .build();
            let key_items = key_items.clone();
            let string_list = string_list.clone();
            let identity_row = identity_row.clone();
            let last_selected = last_selected.clone();
            let is_updating = is_updating.clone();
            file_dialog.open(Some(&parent), gtk::gio::Cancellable::NONE, move |result| {
                if let Ok(file) = result {
                    if let Some(path) = file.path() {
                        let mut items = key_items.borrow_mut();
                        if let Some(existing_idx) = items.iter().position(|(_, p)| p.as_ref() == Some(&path)) {
                            is_updating.set(true);
                            identity_row.set_selected(existing_idx as u32);
                            identity_row.set_subtitle(&path.to_string_lossy());
                            last_selected.set(existing_idx as u32);
                            is_updating.set(false);
                        } else {
                            let name = path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("Custom Key")
                                .to_string();
                            let insert_pos = items.len();
                            items.push((name.clone(), Some(path.clone())));
                            string_list.splice(insert_pos as u32, 0, &[&name]);
                            is_updating.set(true);
                            identity_row.set_selected(insert_pos as u32);
                            identity_row.set_subtitle(&path.to_string_lossy());
                            last_selected.set(insert_pos as u32);
                            is_updating.set(false);
                        }
                        return;
                    }
                }
                is_updating.set(true);
                identity_row.set_selected(last_selected.get());
                is_updating.set(false);
            });
        })
    };

    {
        let key_items = key_items.clone();
        let open_file_chooser = open_file_chooser.clone();
        let last_selected = last_selected.clone();
        let is_updating = is_updating.clone();
        identity_row.connect_selected_notify(move |row| {
            if is_updating.get() {
                return;
            }
            let sel = row.selected() as usize;
            let items = key_items.borrow();
            if sel < items.len() {
                if let Some((_, Some(path))) = items.get(sel) {
                    row.set_subtitle(&path.to_string_lossy());
                    last_selected.set(sel as u32);
                }
            } else {
                open_file_chooser();
            }
        });
    }

    let browse_btn = gtk::Button::from_icon_name("document-open-symbolic");
    browse_btn.add_css_class("flat");
    browse_btn.set_valign(gtk::Align::Center);
    browse_btn.set_tooltip_text(Some(tr("dialog.profile.identity_btn")));
    {
        let open_file_chooser = open_file_chooser.clone();
        browse_btn.connect_clicked(move |_| {
            open_file_chooser();
        });
    }
    identity_row.add_suffix(&browse_btn);

    let password_row = adw::PasswordEntryRow::builder()
        .title(tr("dialog.profile.password"))
        .text(source.password.as_deref().unwrap_or_default())
        .build();

    let update_auth_visibility = {
        let identity_row = identity_row.clone();
        let password_row = password_row.clone();
        let auth_type_row = auth_type_row.clone();
        Rc::new(move || {
            let is_key = auth_type_row.selected() == 0;
            identity_row.set_visible(is_key);
            password_row.set_visible(!is_key);
        })
    };
    update_auth_visibility();
    {
        let update = update_auth_visibility.clone();
        auth_type_row.connect_selected_notify(move |_| {
            update();
        });
    }

    group.add(&name);
    group.add(&host);
    group.add(&port);
    group.add(&username);
    group.add(&auth_type_row);
    group.add(&identity_row);
    group.add(&password_row);
    dialog.set_extra_child(Some(&group));

    dialog.add_response("cancel", tr("dialog.cancel"));
    dialog.add_response("save", tr("dialog.save"));
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);

    dialog.connect_response(None, move |dialog, response| {
        if response != "save" {
            return;
        }
        let host_text = host.text().trim().to_string();
        if host_text.is_empty() {
            dialog.set_body(tr("dialog.profile.empty_host"));
            return;
        }
        let mut saved = source.clone();
        saved.name = if name.text().trim().is_empty() {
            tr("dialog.profile.default_name").into()
        } else {
            name.text().trim().to_string()
        };
        saved.host = host_text;
        saved.port = port.text().parse::<u16>().unwrap_or(22);
        saved.username = username.text().trim().to_string();
        let is_key = auth_type_row.selected() == 0;
        saved.auth_type = if is_key { AuthType::Key } else { AuthType::Password };
        if is_key {
            let sel = identity_row.selected() as usize;
            let items = key_items.borrow();
            saved.identity_file = items.get(sel).and_then(|(_, p)| p.clone());
            saved.password = None;
        } else {
            let pwd_text = password_row.text().to_string();
            saved.password = (!pwd_text.is_empty()).then_some(pwd_text);
            saved.identity_file = None;
        }

        let mut current = config.borrow_mut();
        if let Some(existing) = current.profiles.iter_mut().find(|item| item.id == saved.id) {
            *existing = saved.clone();
        } else {
            current.profiles.push(saved.clone());
        }
        if current.active_profile.is_none() {
            current.active_profile = Some(saved.id);
        }
        if current.save().is_ok() {
            drop(current);
            if let Some(refresh) = refresh.borrow().as_ref() {
                refresh();
            }
        }
    });
    dialog.present(Some(parent));
}

/// 显示单条规则编辑或新增对话框
pub fn show_rule_dialog(
    parent: &adw::ApplicationWindow,
    config: Rc<RefCell<AppConfig>>,
    existing: Option<ListedRule>,
    refresh_rules: RefreshRules,
) {
    let dialog = adw::AlertDialog::new(
        Some(if existing.is_some() { tr("dialog.rule.title_edit") } else { tr("dialog.rule.title_new") }),
        None,
    );
    let group = adw::PreferencesGroup::new();
    let pattern = adw::EntryRow::builder()
        .title(tr("dialog.rule.pattern"))
        .text(existing.as_ref().map(ListedRule::value).unwrap_or_default())
        .build();
    let rule_type = adw::ComboRow::builder()
        .title(tr("dialog.rule.kind"))
        .model(&gtk::StringList::new(&[
            "DOMAIN-SUFFIX",
            "DOMAIN",
            "DOMAIN-KEYWORD",
            "IP-CIDR",
        ]))
        .selected(match existing.as_ref().map(ListedRule::kind_label) {
            Some("DOMAIN") => 1,
            Some("DOMAIN-KEYWORD") => 2,
            Some("IP-CIDR") => 3,
            _ => 0,
        })
        .build();
    let action = adw::ComboRow::builder()
        .title(tr("dialog.rule.action"))
        .model(&gtk::StringList::new(&[tr("action.direct"), tr("action.proxy"), tr("action.block")]))
        .selected(match existing.as_ref().map(ListedRule::action) {
            Some(RuleAction::Direct) => 0,
            Some(RuleAction::Block) => 2,
            _ => 1,
        })
        .build();

    group.add(&pattern);
    group.add(&rule_type);
    group.add(&action);
    dialog.set_extra_child(Some(&group));

    dialog.add_response("cancel", tr("dialog.cancel"));
    dialog.add_response("save", tr("dialog.save"));
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);

    dialog.connect_response(None, move |dialog, response| {
        if response != "save" {
            return;
        }
        let value = pattern.text().trim().to_string();
        if value.is_empty() {
            dialog.set_body(tr("dialog.rule.empty_pattern"));
            return;
        }
        let rule_type_text = match rule_type.selected() {
            1 => "DOMAIN",
            2 => "DOMAIN-KEYWORD",
            3 => "IP-CIDR",
            _ => "DOMAIN-SUFFIX",
        };
        let selected_action = match action.selected() {
            0 => RuleAction::Direct,
            2 => RuleAction::Block,
            _ => RuleAction::Proxy,
        };
        let mut parsed = parse_rule_set(&format!("{rule_type_text},{value}"), selected_action);
        if parsed.rule_count() != 1 {
            dialog.set_body("规则格式无效");
            return;
        }

        let mut current = config.borrow_mut();
        if let Some(existing) = &existing {
            crate::remove_listed_rule(&mut current, existing);
        }
        if let Some(rule) = parsed.domain_rules.pop() {
            current.settings.domain_rules.retain(|item| {
                !(item.pattern == rule.pattern && item.kind == rule.kind)
            });
            current.settings.domain_rules.push(rule);
        } else if let Some(rule) = parsed.ip_rules.pop() {
            current.settings.ip_rules.retain(|item| item.network != rule.network);
            current.settings.ip_rules.push(rule);
        }
        if current.save().is_ok() {
            drop(current);
            if let Some(refresh) = refresh_rules.borrow().as_ref() {
                refresh();
            }
        }
    });
    dialog.present(Some(parent));
}
