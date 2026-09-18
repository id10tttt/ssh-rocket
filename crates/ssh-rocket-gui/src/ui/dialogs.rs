use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use ssh_rocket_core::{parse_rule_set, AppConfig, Profile, RuleAction};
use std::{cell::RefCell, path::PathBuf, rc::Rc};

use crate::ListedRule;

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
        Some(if editing { "编辑连接" } else { "新建连接" }),
        None,
    );
    let group = adw::PreferencesGroup::new();
    let name = adw::EntryRow::builder()
        .title("名称")
        .text(&source.name)
        .build();
    let host = adw::EntryRow::builder()
        .title("服务器地址")
        .text(&source.host)
        .build();
    let port = adw::EntryRow::builder()
        .title("端口")
        .text(source.port.to_string())
        .build();
    let username = adw::EntryRow::builder()
        .title("用户名")
        .text(&source.username)
        .build();
    let identity = adw::EntryRow::builder()
        .title("私钥路径")
        .text(
            source
                .identity_file
                .as_ref()
                .map(|path| path.to_string_lossy())
                .unwrap_or_default(),
        )
        .build();

    group.add(&name);
    group.add(&host);
    group.add(&port);
    group.add(&username);
    group.add(&identity);
    dialog.set_extra_child(Some(&group));

    dialog.add_response("cancel", "取消");
    dialog.add_response("save", "保存");
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);

    dialog.connect_response(None, move |dialog, response| {
        if response != "save" {
            return;
        }
        let host_text = host.text().trim().to_string();
        if host_text.is_empty() {
            dialog.set_body("服务器地址不能为空");
            return;
        }
        let mut saved = source.clone();
        saved.name = if name.text().trim().is_empty() {
            "未命名".into()
        } else {
            name.text().trim().to_string()
        };
        saved.host = host_text;
        saved.port = port.text().parse::<u16>().unwrap_or(22);
        saved.username = username.text().trim().to_string();
        let identity_text = identity.text();
        let identity_text = identity_text.trim();
        saved.identity_file = (!identity_text.is_empty()).then(|| PathBuf::from(identity_text));

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
        Some(if existing.is_some() { "编辑规则" } else { "添加规则" }),
        None,
    );
    let group = adw::PreferencesGroup::new();
    let pattern = adw::EntryRow::builder()
        .title("域名、IP 或 CIDR")
        .text(existing.as_ref().map(ListedRule::value).unwrap_or_default())
        .build();
    let rule_type = adw::ComboRow::builder()
        .title("规则类型")
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
        .title("动作")
        .model(&gtk::StringList::new(&["直连 (DIRECT)", "代理 (PROXY)", "拦截 (REJECT)"]))
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

    dialog.add_response("cancel", "取消");
    dialog.add_response("save", "保存");
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);

    dialog.connect_response(None, move |dialog, response| {
        if response != "save" {
            return;
        }
        let value = pattern.text().trim().to_string();
        if value.is_empty() {
            dialog.set_body("请输入域名、IP 或 CIDR");
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
