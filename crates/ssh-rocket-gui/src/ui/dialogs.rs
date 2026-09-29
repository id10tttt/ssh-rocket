use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use ssh_rocket_core::{
    default_forward_host, parse_rule_set, AppConfig, AuthType, ForwardType, PortForwardRule,
    Profile, RuleAction,
};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};
use uuid::Uuid;

use crate::{i18n::tr, ListedRule};

pub type RefreshConnections = Rc<RefCell<Option<Rc<dyn Fn()>>>>;
pub type RefreshRules = Rc<RefCell<Option<Rc<dyn Fn()>>>>;
pub type RefreshForwards = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

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
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("card");
    card.set_width_request(460);

    let info = gtk::Box::new(gtk::Orientation::Vertical, 10);
    info.set_margin_start(16);
    info.set_margin_end(16);
    info.set_margin_top(14);
    info.set_margin_bottom(14);

    let make_row = |label_text: &str, widget: &gtk::Widget| -> gtk::Box {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row.set_valign(gtk::Align::Center);
        let label = gtk::Label::new(Some(label_text));
        label.add_css_class("dim-label");
        label.set_halign(gtk::Align::Start);
        label.set_width_request(100);
        label.set_xalign(0.0);
        row.append(&label);
        row.append(widget);
        row
    };

    let name = gtk::Entry::builder()
        .text(&source.name)
        .hexpand(true)
        .build();
    let name_row = make_row(tr("dialog.profile.name"), name.upcast_ref());

    let host = gtk::Entry::builder()
        .text(&source.host)
        .hexpand(true)
        .build();
    let host_row = make_row(tr("dialog.profile.host"), host.upcast_ref());

    let port = gtk::Entry::builder()
        .text(source.port.to_string())
        .hexpand(true)
        .build();
    let port_row = make_row(tr("dialog.profile.port"), port.upcast_ref());

    let username = gtk::Entry::builder()
        .text(&source.username)
        .hexpand(true)
        .build();
    let username_row = make_row(tr("dialog.profile.username"), username.upcast_ref());

    let auth_model = gtk::StringList::new(&[tr("dialog.profile.auth_key"), tr("dialog.profile.auth_password")]);
    let auth_dropdown = gtk::DropDown::builder()
        .model(&auth_model)
        .selected(match source.auth_type {
            AuthType::Key => 0,
            AuthType::Password => 1,
        })
        .hexpand(true)
        .build();
    let auth_row = make_row(tr("dialog.profile.auth_type"), auth_dropdown.upcast_ref());

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

    let key_dropdown = gtk::DropDown::builder()
        .model(&string_list)
        .selected(initial_selected)
        .hexpand(true)
        .build();

    let last_selected = Rc::new(Cell::new(initial_selected));
    let is_updating = Rc::new(Cell::new(false));

    let update_key_tooltip = {
        let key_dropdown = key_dropdown.clone();
        let key_items = key_items.clone();
        Rc::new(move || {
            let sel = key_dropdown.selected() as usize;
            let items = key_items.borrow();
            if let Some((_, Some(path))) = items.get(sel) {
                key_dropdown.set_tooltip_text(Some(&path.to_string_lossy()));
            } else {
                key_dropdown.set_tooltip_text(None);
            }
        })
    };
    update_key_tooltip();

    let open_file_chooser = {
        let parent = parent.clone();
        let key_items = key_items.clone();
        let string_list = string_list.clone();
        let key_dropdown = key_dropdown.clone();
        let last_selected = last_selected.clone();
        let is_updating = is_updating.clone();
        Rc::new(move || {
            let file_dialog = gtk::FileDialog::builder()
                .title(tr("dialog.profile.identity_dialog"))
                .modal(true)
                .build();
            let key_items = key_items.clone();
            let string_list = string_list.clone();
            let key_dropdown = key_dropdown.clone();
            let last_selected = last_selected.clone();
            let is_updating = is_updating.clone();
            file_dialog.open(Some(&parent), gtk::gio::Cancellable::NONE, move |result| {
                if let Ok(file) = result {
                    if let Some(path) = file.path() {
                        let mut items = key_items.borrow_mut();
                        if let Some(existing_idx) = items.iter().position(|(_, p)| p.as_ref() == Some(&path)) {
                            is_updating.set(true);
                            key_dropdown.set_selected(existing_idx as u32);
                            key_dropdown.set_tooltip_text(Some(&path.to_string_lossy()));
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
                            key_dropdown.set_selected(insert_pos as u32);
                            key_dropdown.set_tooltip_text(Some(&path.to_string_lossy()));
                            last_selected.set(insert_pos as u32);
                            is_updating.set(false);
                        }
                        return;
                    }
                }
                is_updating.set(true);
                key_dropdown.set_selected(last_selected.get());
                is_updating.set(false);
            });
        })
    };

    {
        let key_items = key_items.clone();
        let open_file_chooser = open_file_chooser.clone();
        let last_selected = last_selected.clone();
        let is_updating = is_updating.clone();
        key_dropdown.connect_selected_notify(move |dropdown| {
            if is_updating.get() {
                return;
            }
            let sel = dropdown.selected() as usize;
            let items = key_items.borrow();
            if sel < items.len() {
                if let Some((_, Some(path))) = items.get(sel) {
                    dropdown.set_tooltip_text(Some(&path.to_string_lossy()));
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

    let key_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    key_box.set_hexpand(true);
    key_dropdown.set_hexpand(true);
    key_box.append(&key_dropdown);
    key_box.append(&browse_btn);
    let key_row = make_row(tr("dialog.profile.identity"), key_box.upcast_ref());

    let password_entry = gtk::PasswordEntry::builder()
        .text(source.password.as_deref().unwrap_or_default())
        .show_peek_icon(true)
        .hexpand(true)
        .build();
    let password_row = make_row(tr("dialog.profile.password"), password_entry.upcast_ref());

    let update_auth_visibility = {
        let key_row = key_row.clone();
        let password_row = password_row.clone();
        let auth_dropdown = auth_dropdown.clone();
        Rc::new(move || {
            let is_key = auth_dropdown.selected() == 0;
            key_row.set_visible(is_key);
            password_row.set_visible(!is_key);
        })
    };
    update_auth_visibility();
    {
        let update = update_auth_visibility.clone();
        auth_dropdown.connect_selected_notify(move |_| {
            update();
        });
    }

    info.append(&name_row);
    info.append(&host_row);
    info.append(&port_row);
    info.append(&username_row);
    info.append(&auth_row);
    info.append(&key_row);
    info.append(&password_row);
    card.append(&info);
    dialog.set_extra_child(Some(&card));

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
        let is_key = auth_dropdown.selected() == 0;
        saved.auth_type = if is_key { AuthType::Key } else { AuthType::Password };
        if is_key {
            let sel = key_dropdown.selected() as usize;
            let items = key_items.borrow();
            saved.identity_file = items.get(sel).and_then(|(_, p)| p.clone());
            saved.password = None;
        } else {
            let pwd_text = password_entry.text().to_string();
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

/// 显示端口转发规则配置对话框
pub fn show_forward_dialog(
    parent: &adw::ApplicationWindow,
    config: Rc<RefCell<AppConfig>>,
    rule: Option<PortForwardRule>,
    refresh: RefreshForwards,
) {
    let editing = rule.is_some();
    let source = rule.unwrap_or_default();
    let dialog = adw::AlertDialog::new(
        Some(if editing {
            tr("dialog.forward.title_edit")
        } else {
            tr("dialog.forward.title_new")
        }),
        None,
    );

    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.set_width_request(520);

    let make_field = |label_text: &str, widget: &gtk::Widget| -> gtk::Box {
        let field = gtk::Box::new(gtk::Orientation::Vertical, 6);
        field.set_hexpand(true);
        let label = gtk::Label::new(Some(label_text));
        label.add_css_class("heading");
        label.set_halign(gtk::Align::Start);
        label.set_xalign(0.0);
        label.set_mnemonic_widget(Some(widget));
        field.append(&label);
        field.append(widget);
        field
    };

    // SSH 连接
    let profiles = config.borrow().profiles.clone();
    let profile_strings: Vec<String> = profiles
        .iter()
        .map(|p| format!("{} ({}:{})", p.name, p.host, p.port))
        .collect();
    let profile_str_slices: Vec<&str> = profile_strings.iter().map(String::as_str).collect();
    let initial_profile_index = profiles
        .iter()
        .position(|p| p.id == source.profile_id)
        .unwrap_or(0) as u32;
    let profile_dropdown = gtk::DropDown::from_strings(&profile_str_slices);
    profile_dropdown.set_selected(initial_profile_index);
    profile_dropdown.set_hexpand(true);
    profile_dropdown.set_sensitive(!profiles.is_empty());
    content.append(&make_field(
        tr("dialog.forward.connection"),
        profile_dropdown.upcast_ref(),
    ));

    // 映射方向
    let direction_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let direction_label = gtk::Label::new(Some(tr("dialog.forward.direction")));
    direction_label.add_css_class("heading");
    direction_label.set_halign(gtk::Align::Start);
    direction_box.append(&direction_label);

    let direction_buttons = gtk::Box::new(gtk::Orientation::Horizontal, 10);

    let make_direction_button = |title: &str, description: &str| {
        let button = gtk::ToggleButton::new();
        button.add_css_class("forward-direction-button");
        button.set_hexpand(true);

        let button_content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        button_content.set_margin_start(12);
        button_content.set_margin_end(12);
        button_content.set_margin_top(10);
        button_content.set_margin_bottom(10);

        let labels = gtk::Box::new(gtk::Orientation::Vertical, 2);
        labels.set_hexpand(true);
        let title_label = gtk::Label::new(Some(title));
        title_label.add_css_class("heading");
        title_label.set_halign(gtk::Align::Start);
        let description_label = gtk::Label::new(Some(description));
        description_label.add_css_class("dim-label");
        description_label.set_halign(gtk::Align::Start);
        description_label.set_wrap(true);
        description_label.set_xalign(0.0);
        labels.append(&title_label);
        labels.append(&description_label);

        let check = gtk::Image::from_icon_name("object-select-symbolic");
        check.set_visible(false);
        check.set_valign(gtk::Align::Center);
        button_content.append(&labels);
        button_content.append(&check);
        button.set_child(Some(&button_content));
        (button, check)
    };

    let (local_direction, local_check) = make_direction_button(
        tr("forward.type.local"),
        tr("forward.type.local_desc"),
    );
    let (remote_direction, remote_check) = make_direction_button(
        tr("forward.type.remote"),
        tr("forward.type.remote_desc"),
    );
    remote_direction.set_group(Some(&local_direction));
    match source.forward_type {
        ForwardType::Local => local_direction.set_active(true),
        ForwardType::Remote => remote_direction.set_active(true),
    }
    local_check.set_visible(local_direction.is_active());
    remote_check.set_visible(remote_direction.is_active());
    {
        let check = local_check.clone();
        local_direction.connect_toggled(move |button| check.set_visible(button.is_active()));
    }
    {
        let check = remote_check.clone();
        remote_direction.connect_toggled(move |button| check.set_visible(button.is_active()));
    }
    direction_buttons.append(&local_direction);
    direction_buttons.append(&remote_direction);
    direction_box.append(&direction_buttons);
    content.append(&direction_box);

    // 端口映射
    let mapping_box = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let mapping_title = gtk::Label::new(Some(tr("dialog.forward.mapping")));
    mapping_title.add_css_class("heading");
    mapping_title.set_halign(gtk::Align::Start);
    mapping_box.append(&mapping_title);

    let mapping_fields = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    mapping_fields.set_valign(gtk::Align::Center);
    let local_port_entry = gtk::Entry::builder()
        .text(if source.local_port > 0 {
            source.local_port.to_string()
        } else {
            String::new()
        })
        .placeholder_text("8080")
        .hexpand(true)
        .build();
    local_port_entry.set_input_purpose(gtk::InputPurpose::Digits);
    let local_port_error = gtk::Label::new(Some(tr("dialog.forward.invalid_port")));
    local_port_error.add_css_class("error");
    local_port_error.set_halign(gtk::Align::Start);
    local_port_error.set_visible(false);
    let local_port_field = make_field(
        tr("dialog.forward.local_port"),
        local_port_entry.upcast_ref(),
    );
    local_port_field.append(&local_port_error);

    let arrow = gtk::Label::new(Some("→"));
    arrow.add_css_class("forward-mapping-arrow");
    arrow.set_accessible_role(gtk::AccessibleRole::Presentation);
    arrow.set_valign(gtk::Align::Center);

    let remote_port_entry = gtk::Entry::builder()
        .text(if source.remote_port > 0 {
            source.remote_port.to_string()
        } else {
            String::new()
        })
        .placeholder_text("80")
        .hexpand(true)
        .build();
    remote_port_entry.set_input_purpose(gtk::InputPurpose::Digits);
    let remote_port_error = gtk::Label::new(Some(tr("dialog.forward.invalid_port")));
    remote_port_error.add_css_class("error");
    remote_port_error.set_halign(gtk::Align::Start);
    remote_port_error.set_visible(false);
    let remote_port_field = make_field(
        tr("dialog.forward.server_port"),
        remote_port_entry.upcast_ref(),
    );
    remote_port_field.append(&remote_port_error);
    mapping_fields.append(&local_port_field);
    mapping_fields.append(&arrow);
    mapping_fields.append(&remote_port_field);
    mapping_box.append(&mapping_fields);
    content.append(&mapping_box);

    // 高级地址设置
    let local_host_entry = gtk::Entry::builder()
        .text(if source.local_host.is_empty() {
            default_forward_host()
        } else {
            source.local_host.clone()
        })
        .hexpand(true)
        .build();
    let remote_host_entry = gtk::Entry::builder()
        .text(if source.remote_host.is_empty() {
            default_forward_host()
        } else {
            source.remote_host.clone()
        })
        .hexpand(true)
        .build();
    let local_host_label = gtk::Label::new(None);
    local_host_label.add_css_class("heading");
    local_host_label.set_halign(gtk::Align::Start);
    local_host_label.set_mnemonic_widget(Some(&local_host_entry));
    let local_host_field = gtk::Box::new(gtk::Orientation::Vertical, 6);
    local_host_field.append(&local_host_label);
    local_host_field.append(&local_host_entry);
    let remote_host_label = gtk::Label::new(None);
    remote_host_label.add_css_class("heading");
    remote_host_label.set_halign(gtk::Align::Start);
    remote_host_label.set_mnemonic_widget(Some(&remote_host_entry));
    let remote_host_field = gtk::Box::new(gtk::Orientation::Vertical, 6);
    remote_host_field.append(&remote_host_label);
    remote_host_field.append(&remote_host_entry);
    let advanced_fields = gtk::Box::new(gtk::Orientation::Vertical, 12);
    advanced_fields.set_margin_top(10);
    advanced_fields.append(&local_host_field);
    advanced_fields.append(&remote_host_field);
    let advanced = gtk::Expander::builder()
        .label(tr("dialog.forward.advanced"))
        .child(&advanced_fields)
        .build();
    content.append(&advanced);

    let update_direction = {
        let local_host_label = local_host_label.clone();
        let remote_host_label = remote_host_label.clone();
        let mapping_fields = mapping_fields.clone();
        let local_port_field = local_port_field.clone();
        let remote_port_field = remote_port_field.clone();
        let arrow = arrow.clone();
        move |is_remote: bool| {
            if is_remote {
                local_host_label.set_text(tr("dialog.forward.local_target_host"));
                remote_host_label.set_text(tr("dialog.forward.server_bind_host"));
                mapping_fields.reorder_child_after(&remote_port_field, None::<&gtk::Widget>);
                mapping_fields.reorder_child_after(&arrow, Some(&remote_port_field));
                mapping_fields.reorder_child_after(&local_port_field, Some(&arrow));
            } else {
                local_host_label.set_text(tr("dialog.forward.local_bind_host"));
                remote_host_label.set_text(tr("dialog.forward.server_target_host"));
                mapping_fields.reorder_child_after(&local_port_field, None::<&gtk::Widget>);
                mapping_fields.reorder_child_after(&arrow, Some(&local_port_field));
                mapping_fields.reorder_child_after(&remote_port_field, Some(&arrow));
            }
        }
    };
    update_direction(remote_direction.is_active());
    remote_direction.connect_toggled(move |button| update_direction(button.is_active()));

    // 可选名称
    let name_entry = gtk::Entry::builder()
        .text(&source.name)
        .placeholder_text(tr("dialog.forward.name_placeholder"))
        .hexpand(true)
        .build();
    content.append(&make_field(
        tr("dialog.forward.name"),
        name_entry.upcast_ref(),
    ));
    dialog.set_extra_child(Some(&content));

    dialog.add_response("cancel", tr("dialog.cancel"));
    dialog.add_response("save", tr("dialog.save"));
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);

    let saved_id = source.id;
    let profiles_clone = profiles.clone();

    dialog.connect_response(None, move |dialog, response| {
        if response != "save" {
            return;
        }
        if profiles_clone.is_empty() {
            dialog.set_body(tr("dialog.forward.no_connection"));
            return;
        }

        local_port_error.set_visible(false);
        remote_port_error.set_visible(false);

        let l_port = match local_port_entry.text().trim().parse::<u16>() {
            Ok(p) if p > 0 => p,
            _ => {
                local_port_error.set_visible(true);
                local_port_entry.add_css_class("error");
                local_port_entry.grab_focus();
                return;
            }
        };
        local_port_entry.remove_css_class("error");

        let r_port = match remote_port_entry.text().trim().parse::<u16>() {
            Ok(p) if p > 0 => p,
            _ => {
                remote_port_error.set_visible(true);
                remote_port_entry.add_css_class("error");
                remote_port_entry.grab_focus();
                return;
            }
        };
        remote_port_entry.remove_css_class("error");

        let selected_profile_idx = profile_dropdown.selected() as usize;
        let selected_profile_id = match profiles_clone.get(selected_profile_idx) {
            Some(p) => p.id,
            None => {
                dialog.set_body(tr("dialog.forward.no_connection"));
                return;
            }
        };

        let forward_type = if remote_direction.is_active() {
            ForwardType::Remote
        } else {
            ForwardType::Local
        };

        let mut name = name_entry.text().trim().to_string();
        if name.is_empty() {
            name = match forward_type {
                ForwardType::Local => format!("{}:{} -> {}:{}", local_host_entry.text().trim(), l_port, remote_host_entry.text().trim(), r_port),
                ForwardType::Remote => format!("{}:{} -> {}:{}", remote_host_entry.text().trim(), r_port, local_host_entry.text().trim(), l_port),
            };
        }

        let updated_rule = PortForwardRule {
            id: if editing { saved_id } else { Uuid::new_v4() },
            name,
            profile_id: selected_profile_id,
            forward_type,
            local_host: {
                let h = local_host_entry.text().trim().to_string();
                if h.is_empty() { default_forward_host() } else { h }
            },
            local_port: l_port,
            remote_host: {
                let h = remote_host_entry.text().trim().to_string();
                if h.is_empty() { default_forward_host() } else { h }
            },
            remote_port: r_port,
            enabled: source.enabled,
        };

        let mut current = config.borrow_mut();
        if let Some(pos) = current.port_forwards.iter().position(|r| r.id == updated_rule.id) {
            current.port_forwards[pos] = updated_rule;
        } else {
            current.port_forwards.push(updated_rule);
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
