use adw::prelude::*;
use gtk4::{self as gtk, gio, glib};
use libadwaita as adw;
use ssh_rocket_core::{
    parse_omega_rules, parse_rule_set, AppConfig, AppRule, DomainRule, IpRule, RuleAction,
};
use std::{cell::RefCell, fs, path::PathBuf, rc::Rc};

use crate::{
    app_scanner::{current_app_action, scan_desktop_apps},
    controller::RuntimeController,
    i18n::tr,
    rule_manager::{
        custom_rules, domain_kind_label, imported_rules,
        RuleListState,
    },
    ui::{
        dialogs::{show_rule_dialog, RefreshRules},
        rules_view::{append_rule_batch, refresh_rule_list, RulesView},
        widgets::create_app_icon,
        window::MainWindowWidgets,
    },
};

pub fn setup_rules_logic(
    win: &MainWindowWidgets,
    rules_view: &RulesView,
    config: &Rc<RefCell<AppConfig>>,
    controller: &Rc<RefCell<RuntimeController>>,
    refresh_rule_views: &RefreshRules,
    refresh_blocked_views: &RefreshRules,
    refresh_traffic_rule_counts_fn: &Rc<RefCell<Option<Rc<dyn Fn()>>>>,
) {
    // 1. 默认策略与 IPv6 设置
    {
        let config = config.clone();
        rules_view.policy_row.connect_selected_notify(move |row| {
            let mut current = config.borrow_mut();
            current.settings.default_policy = match row.selected() {
                1 => RuleAction::Direct,
                2 => RuleAction::Block,
                _ => RuleAction::Proxy,
            };
            let _ = current.save();
        });
    }
    {
        let config = config.clone();
        rules_view.ipv6_row.connect_active_notify(move |row| {
            let mut current = config.borrow_mut();
            current.settings.ipv6 = row.is_active();
            let _ = current.save();
        });
    }

    // 2. 规则列表分页状态与加载
    let imported_state = Rc::new(RefCell::new(RuleListState::default()));
    let custom_state = Rc::new(RefCell::new(RuleListState::default()));

    {
        let group = rules_view.imported_rules_group.clone();
        let state = imported_state.clone();
        let load_more = rules_view.imported_load_more.clone();
        let parent = win.window.clone();
        let config = config.clone();
        let refresh = refresh_rule_views.clone();
        rules_view.imported_load_more.connect_clicked(move |_| {
            append_rule_batch(&group, &state, &load_more, false, &parent, &config, &refresh);
        });
    }
    {
        let group = rules_view.custom_rules_group.clone();
        let state = custom_state.clone();
        let load_more = rules_view.custom_load_more.clone();
        let parent = win.window.clone();
        let config = config.clone();
        let refresh = refresh_rule_views.clone();
        rules_view.custom_load_more.connect_clicked(move |_| {
            append_rule_batch(&group, &state, &load_more, true, &parent, &config, &refresh);
        });
    }
    {
        let group = rules_view.imported_rules_group.clone();
        let state = imported_state.clone();
        let load_more = rules_view.imported_load_more.clone();
        let parent = win.window.clone();
        let config = config.clone();
        let refresh = refresh_rule_views.clone();
        rules_view
            .imported_scroller
            .vadjustment()
            .connect_value_changed(move |adjustment| {
                if adjustment.value() + adjustment.page_size() >= adjustment.upper() - 160.0 {
                    append_rule_batch(&group, &state, &load_more, false, &parent, &config, &refresh);
                }
            });
    }
    {
        let group = rules_view.custom_rules_group.clone();
        let state = custom_state.clone();
        let load_more = rules_view.custom_load_more.clone();
        let parent = win.window.clone();
        let config = config.clone();
        let refresh = refresh_rule_views.clone();
        rules_view
            .custom_scroller
            .vadjustment()
            .connect_value_changed(move |adjustment| {
                if adjustment.value() + adjustment.page_size() >= adjustment.upper() - 160.0 {
                    append_rule_batch(&group, &state, &load_more, true, &parent, &config, &refresh);
                }
            });
    }

    let refresh_rule_views_impl: Rc<dyn Fn()> = {
        let config = config.clone();
        let rule_status = rules_view.rule_status_row.clone();
        let custom_summary = rules_view.custom_summary_row.clone();
        let detail_title = rules_view.detail_title_lbl.clone();
        let source_detail = rules_view.source_detail_row.clone();
        let imported_summary = rules_view.imported_summary_row.clone();
        let imported_search = rules_view.imported_search.clone();
        let imported_rules_group = rules_view.imported_rules_group.clone();
        let imported_state = imported_state.clone();
        let imported_load_more = rules_view.imported_load_more.clone();
        let custom_search = rules_view.custom_search.clone();
        let custom_rules_group = rules_view.custom_rules_group.clone();
        let custom_state = custom_state.clone();
        let custom_load_more = rules_view.custom_load_more.clone();
        let clear_rules = rules_view.clear_rules_btn.clone();
        let parent = win.window.clone();
        let refresh_rule_views = refresh_rule_views.clone();
        let refresh_blocked_views = refresh_blocked_views.clone();
        let refresh_traffic_counts = refresh_traffic_rule_counts_fn.clone();
        Rc::new(move || {
            let current = config.borrow();
            let imported = imported_rules(&current);
            let custom = custom_rules(&current);
            let source_name = if current.settings.rule_source_name.is_empty() {
                "订阅规则"
            } else {
                &current.settings.rule_source_name
            };
            if imported.is_empty() {
                rule_status.set_title("未配置远程规则");
                rule_status.set_subtitle("");
                rule_status.set_activatable(false);
            } else {
                rule_status.set_title(source_name);
                rule_status.set_subtitle(&format!("共 {} 条规则", imported.len()));
                rule_status.set_activatable(true);
            }
            custom_summary.set_subtitle(&format!("共 {} 条规则", custom.len()));
            detail_title.set_text(source_name);
            source_detail.set_title(source_name);
            source_detail.set_subtitle(&current.settings.rule_source_url);
            let (direct, proxy, reject) = imported.iter().fold((0, 0, 0), |counts, rule| {
                match rule.action() {
                    RuleAction::Direct => (counts.0 + 1, counts.1, counts.2),
                    RuleAction::Proxy => (counts.0, counts.1 + 1, counts.2),
                    RuleAction::Block => (counts.0, counts.1, counts.2 + 1),
                }
            });
            imported_summary.set_subtitle(&format!(
                "共 {} 条 · 直连 {direct} · 代理 {proxy} · 拦截 {reject}",
                imported.len()
            ));
            clear_rules.set_visible(!custom.is_empty());
            let imported_query = imported_search.text().to_string();
            let custom_query = custom_search.text().to_string();
            drop(current);
            refresh_rule_list(
                &imported_rules_group,
                &imported_state,
                &imported_load_more,
                imported,
                &imported_query,
                false,
                &parent,
                &config,
                &refresh_rule_views,
            );
            refresh_rule_list(
                &custom_rules_group,
                &custom_state,
                &custom_load_more,
                custom,
                &custom_query,
                true,
                &parent,
                &config,
                &refresh_rule_views,
            );
            if let Some(refresh) = refresh_blocked_views.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_traffic_counts.borrow().as_ref() {
                refresh();
            }
        })
    };
    *refresh_rule_views.borrow_mut() = Some(refresh_rule_views_impl.clone());
    refresh_rule_views_impl();

    {
        let refresh = refresh_rule_views_impl.clone();
        rules_view
            .imported_search
            .connect_search_changed(move |_| refresh());
    }
    {
        let refresh = refresh_rule_views_impl.clone();
        rules_view
            .custom_search
            .connect_search_changed(move |_| refresh());
    }

    // 3. 规则分流子页面导航与返回按钮
    {
        let stack = rules_view.domain_stack.clone();
        let header = win.header.clone();
        let page_title = win.page_title.clone();
        let back_btn = win.back_button.clone();
        let source_title = rules_view.detail_title_lbl.clone();
        let rules_switcher_box = rules_view.switcher_box.clone();
        rules_view
            .rule_status_row
            .connect_activated(move |_| {
                stack.set_visible_child_name("detail");
                rules_switcher_box.set_visible(false);
                back_btn.set_visible(true);
                let title = source_title.text();
                page_title.set_text(if title.is_empty() { "订阅配置" } else { title.as_str() });
                header.set_title_widget(Some(&page_title));
            });
    }
    {
        let stack = rules_view.domain_stack.clone();
        let header = win.header.clone();
        let page_title = win.page_title.clone();
        let back_btn = win.back_button.clone();
        let rules_switcher_box = rules_view.switcher_box.clone();
        rules_view
            .custom_summary_row
            .connect_activated(move |_| {
                stack.set_visible_child_name("custom");
                rules_switcher_box.set_visible(false);
                back_btn.set_visible(true);
                page_title.set_text("自定义分流规则");
                header.set_title_widget(Some(&page_title));
            });
    }
    {
        let stack = rules_view.domain_stack.clone();
        let header = win.header.clone();
        let page_title = win.page_title.clone();
        let back_btn = win.back_button.clone();
        let rules_switcher_box = rules_view.switcher_box.clone();
        rules_view
            .imported_summary_row
            .connect_activated(move |_| {
                stack.set_visible_child_name("imported");
                rules_switcher_box.set_visible(false);
                back_btn.set_visible(true);
                page_title.set_text("订阅规则条目");
                header.set_title_widget(Some(&page_title));
            });
    }
    {
        let stack = rules_view.domain_stack.clone();
        let header = win.header.clone();
        let page_title = win.page_title.clone();
        let back_btn = win.back_button.clone();
        let rules_switcher_box = rules_view.switcher_box.clone();
        let source_title = rules_view.detail_title_lbl.clone();
        win.back_button.connect_clicked(move |_| {
            match stack.visible_child_name().as_deref() {
                Some("imported") => {
                    stack.set_visible_child_name("detail");
                    let title = source_title.text();
                    page_title.set_text(if title.is_empty() { "订阅配置" } else { title.as_str() });
                    header.set_title_widget(Some(&page_title));
                    back_btn.set_visible(true);
                    rules_switcher_box.set_visible(false);
                }
                _ => {
                    stack.set_visible_child_name("overview");
                    back_btn.set_visible(false);
                    page_title.set_text(crate::i18n::tr("nav.rules"));
                    header.set_title_widget(Some(&page_title));
                    rules_switcher_box.set_visible(true);
                }
            }
        });
    }
    {
        let back_btn = win.back_button.clone();
        rules_view.detail_back_btn.connect_clicked(move |_| {
            back_btn.emit_clicked();
        });
    }
    {
        let back_btn = win.back_button.clone();
        rules_view.imported_back_btn.connect_clicked(move |_| {
            back_btn.emit_clicked();
        });
    }
    {
        let back_btn = win.back_button.clone();
        rules_view.custom_back_btn.connect_clicked(move |_| {
            back_btn.emit_clicked();
        });
    }

    // 4. 规则增删与订阅操作
    {
        let parent = win.window.clone();
        let config = config.clone();
        let refresh = refresh_rule_views.clone();
        rules_view.quick_add_rule_btn.connect_clicked(move |_| {
            show_rule_dialog(&parent, config.clone(), None, refresh.clone());
        });
    }
    {
        let parent = win.window.clone();
        let trigger = rules_view.import_trigger_btn.clone();
        let rule_source = rules_view.rule_source_entry.clone();
        rules_view.import_button.connect_clicked(move |_| {
            let dialog = adw::AlertDialog::new(Some("导入远程规则"), None);
            let group = adw::PreferencesGroup::new();
            let url = adw::EntryRow::builder()
                .title("HTTPS 订阅地址")
                .text(rule_source.text())
                .build();
            group.add(&url);
            dialog.set_extra_child(Some(&group));
            dialog.add_response("cancel", "取消");
            dialog.add_response("import", "导入");
            dialog.set_response_appearance("import", adw::ResponseAppearance::Suggested);
            let trigger = trigger.clone();
            let rule_source = rule_source.clone();
            dialog.connect_response(None, move |_, response| {
                if response == "import" {
                    rule_source.set_text(url.text().trim());
                    trigger.emit_clicked();
                }
            });
            dialog.present(Some(&parent));
        });
    }
    {
        let trigger = rules_view.import_trigger_btn.clone();
        let rule_source = rules_view.rule_source_entry.clone();
        let config = config.clone();
        rules_view.update_source_btn.connect_clicked(move |_| {
            rule_source.set_text(&config.borrow().settings.rule_source_url);
            trigger.emit_clicked();
        });
    }
    {
        let parent = win.window.clone();
        let config = config.clone();
        let stack = rules_view.domain_stack.clone();
        let refresh = refresh_rule_views.clone();
        let back_btn = win.back_button.clone();
        let header = win.header.clone();
        let page_title = win.page_title.clone();
        let rules_switcher_box = rules_view.switcher_box.clone();
        rules_view.remove_source_btn.connect_clicked(move |_| {
            let dialog = adw::AlertDialog::new(Some("确认删除该订阅配置？"), None);
            dialog.add_response("cancel", "取消");
            dialog.add_response("remove", "删除");
            dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
            let config = config.clone();
            let stack = stack.clone();
            let refresh = refresh.clone();
            let back_btn = back_btn.clone();
            let header = header.clone();
            let page_title = page_title.clone();
            let rules_switcher_box = rules_switcher_box.clone();
            dialog.connect_response(None, move |_, response| {
                if response == "remove" {
                    let mut current = config.borrow_mut();
                    current.settings.imported_domain_rules.clear();
                    current.settings.imported_ip_rules.clear();
                    current.settings.rule_source_url.clear();
                    current.settings.rule_source_name.clear();
                    current.settings.rule_source_updated_at = 0;
                    if current.save().is_ok() {
                        drop(current);
                        stack.set_visible_child_name("overview");
                        back_btn.set_visible(false);
                        page_title.set_text(crate::i18n::tr("nav.rules"));
                        header.set_title_widget(Some(&page_title));
                        rules_switcher_box.set_visible(true);
                        if let Some(refresh) = refresh.borrow().as_ref() {
                            refresh();
                        }
                    }
                }
            });
            dialog.present(Some(&parent));
        });
    }
    {
        let parent = win.window.clone();
        let config = config.clone();
        let refresh = refresh_rule_views.clone();
        rules_view.clear_rules_btn.connect_clicked(move |_| {
            let dialog = adw::AlertDialog::new(Some("确认清空所有自定义规则？"), None);
            dialog.add_response("cancel", "取消");
            dialog.add_response("clear", "清空");
            dialog.set_response_appearance("clear", adw::ResponseAppearance::Destructive);
            let config = config.clone();
            let refresh = refresh.clone();
            dialog.connect_response(None, move |_, response| {
                if response == "clear" {
                    let mut current = config.borrow_mut();
                    current.settings.domain_rules.clear();
                    current.settings.ip_rules.clear();
                    if current.save().is_ok() {
                        drop(current);
                        if let Some(refresh) = refresh.borrow().as_ref() {
                            refresh();
                        }
                    }
                }
            });
            dialog.present(Some(&parent));
        });
    }
    {
        let parent = win.window.clone();
        let config = config.clone();
        let refresh = refresh_rule_views.clone();
        rules_view.add_rule_btn.connect_clicked(move |_| {
            show_rule_dialog(&parent, config.clone(), None, refresh.clone());
        });
    }
    {
        let parent = win.window.clone();
        let config = config.clone();
        let refresh = refresh_rule_views.clone();
        rules_view.import_omega_btn.connect_clicked(move |_| {
            let file_dialog = gtk::FileDialog::builder()
                .title("导入 SwitchyOmega 规则备份")
                .accept_label("打开")
                .build();

            let filter = gtk::FileFilter::new();
            filter.add_pattern("*.bak");
            filter.add_pattern("*.json");
            filter.set_name(Some("Omega 备份文件 (*.bak, *.json)"));

            let all_filter = gtk::FileFilter::new();
            all_filter.add_pattern("*");
            all_filter.set_name(Some("所有文件"));

            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            filters.append(&all_filter);
            file_dialog.set_filters(Some(&filters));

            let dialog_parent = parent.clone();
            let config = config.clone();
            let refresh = refresh.clone();
            file_dialog.open(Some(&parent), gio::Cancellable::NONE, move |result| {
                let parent = dialog_parent;
                let Ok(file) = result else {
                    return;
                };
                let Some(path) = file.path() else {
                    return;
                };
                let content = match fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        let dialog = adw::AlertDialog::new(
                            Some("读取失败"),
                            Some(&format!("无法读取备份文件: {e}")),
                        );
                        dialog.add_response("ok", "确定");
                        dialog.present(Some(&parent));
                        return;
                    }
                };

                let parsed = match parse_omega_rules(&content) {
                    Ok(p) => p,
                    Err(e) => {
                        let dialog = adw::AlertDialog::new(
                            Some("解析失败"),
                            Some(&format!("备份文件格式无效: {e}")),
                        );
                        dialog.add_response("ok", "确定");
                        dialog.present(Some(&parent));
                        return;
                    }
                };

                let rule_count = parsed.rule_count();
                let domain_count = parsed.domain_rules.len();
                let ip_count = parsed.ip_rules.len();
                let file_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("备份文件");

                let dialog = adw::AlertDialog::new(
                    Some("导入 Omega 规则"),
                    Some(&format!(
                        "在 \"{file_name}\" 中解析出 {rule_count} 条规则 ({domain_count} 域名, {ip_count} IP)。\n\n请选择导入方式:",
                    )),
                );
                dialog.add_response("cancel", "取消");
                dialog.add_response("replace", "完全替换");
                dialog.set_response_appearance("replace", adw::ResponseAppearance::Destructive);
                dialog.add_response("merge", "增量合并");
                dialog.set_response_appearance("merge", adw::ResponseAppearance::Suggested);

                let config = config.clone();
                let refresh = refresh.clone();
                let err_parent = parent.clone();
                dialog.connect_response(None, move |_, response| {
                    if response == "cancel" {
                        return;
                    }
                    let mut current = config.borrow_mut();
                    if response == "replace" {
                        current.settings.domain_rules = parsed.domain_rules.clone();
                        current.settings.ip_rules = parsed.ip_rules.clone();
                    } else if response == "merge" {
                        for new_domain in &parsed.domain_rules {
                            if let Some(existing) = current
                                .settings
                                .domain_rules
                                .iter_mut()
                                .find(|item| item.pattern == new_domain.pattern && item.kind == new_domain.kind)
                            {
                                existing.action = new_domain.action;
                            } else {
                                current.settings.domain_rules.push(new_domain.clone());
                            }
                        }
                        for new_ip in &parsed.ip_rules {
                            if let Some(existing) = current
                                .settings
                                .ip_rules
                                .iter_mut()
                                .find(|item| item.network == new_ip.network)
                            {
                                existing.action = new_ip.action;
                            } else {
                                current.settings.ip_rules.push(new_ip.clone());
                            }
                        }
                    }

                    if let Err(e) = current.save() {
                        let err_dialog = adw::AlertDialog::new(
                            Some("保存失败"),
                            Some(&format!("规则写入失败: {e}")),
                        );
                        err_dialog.add_response("ok", "确定");
                        err_dialog.present(Some(&err_parent));
                        return;
                    }
                    drop(current);
                    if let Some(refresh) = refresh.borrow().as_ref() {
                        refresh();
                    }
                });

                dialog.present(Some(&parent));
            });
        });
    }

    // 5. 黑名单逻辑
    setup_blacklist_logic(
        rules_view,
        config,
        controller,
        refresh_blocked_views,
        refresh_rule_views,
        refresh_traffic_rule_counts_fn,
    );
}

fn setup_blacklist_logic(
    rules_view: &RulesView,
    config: &Rc<RefCell<AppConfig>>,
    controller: &Rc<RefCell<RuntimeController>>,
    refresh_blocked_views: &RefreshRules,
    refresh_rule_views: &RefreshRules,
    refresh_traffic_rule_counts_fn: &Rc<RefCell<Option<Rc<dyn Fn()>>>>,
) {
    let on_add_proc = {
        let new_proc_row = rules_view.new_proc_row.clone();
        let config = config.clone();
        let controller = controller.clone();
        let refresh_blocked = refresh_blocked_views.clone();
        let refresh_rules = refresh_rule_views.clone();
        let refresh_counts = refresh_traffic_rule_counts_fn.clone();
        Rc::new(move || {
            let proc_name = new_proc_row.text().trim().to_string();
            if proc_name.is_empty() {
                return;
            }
            let mut current = config.borrow_mut();
            current.settings.app_rules.retain(|r| {
                r.executable.file_name().and_then(|n| n.to_str()) != Some(&proc_name)
            });
            current.settings.app_rules.push(AppRule {
                executable: PathBuf::from(&proc_name),
                action: RuleAction::Block,
            });
            let _ = current.save();
            drop(current);
            controller.borrow().sync_rules();
            new_proc_row.set_text("");
            if let Some(refresh) = refresh_blocked.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_rules.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_counts.borrow().as_ref() {
                refresh();
            }
        })
    };
    {
        let on_add = on_add_proc.clone();
        rules_view
            .add_proc_btn
            .connect_clicked(move |_| on_add());
    }
    {
        let on_add = on_add_proc.clone();
        rules_view
            .new_proc_row
            .connect_entry_activated(move |_| on_add());
    }

    let on_add_target = {
        let new_target_row = rules_view.new_target_row.clone();
        let config = config.clone();
        let controller = controller.clone();
        let refresh_blocked = refresh_blocked_views.clone();
        let refresh_rules = refresh_rule_views.clone();
        let refresh_counts = refresh_traffic_rule_counts_fn.clone();
        Rc::new(move || {
            let target = new_target_row.text().trim().to_string();
            if target.is_empty() {
                return;
            }
            let mut current = config.borrow_mut();
            if target.contains('/') || target.parse::<std::net::IpAddr>().is_ok() {
                let parsed = parse_rule_set(&format!("IP-CIDR,{target}"), RuleAction::Block);
                for rule in parsed.ip_rules {
                    current
                        .settings
                        .ip_rules
                        .retain(|r| r.network != rule.network);
                    current.settings.ip_rules.push(rule);
                }
            } else {
                let parsed = parse_rule_set(&format!("DOMAIN-SUFFIX,{target}"), RuleAction::Block);
                for rule in parsed.domain_rules {
                    current
                        .settings
                        .domain_rules
                        .retain(|r| !(r.pattern == rule.pattern && r.kind == rule.kind));
                    current.settings.domain_rules.push(rule);
                }
            }
            let _ = current.save();
            drop(current);
            controller.borrow().sync_rules();
            new_target_row.set_text("");
            if let Some(refresh) = refresh_blocked.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_rules.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_counts.borrow().as_ref() {
                refresh();
            }
        })
    };
    {
        let on_add = on_add_target.clone();
        rules_view
            .add_target_btn
            .connect_clicked(move |_| on_add());
    }
    {
        let on_add = on_add_target.clone();
        rules_view
            .new_target_row
            .connect_entry_activated(move |_| on_add());
    }

    for app in &scan_desktop_apps() {
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&app.name))
            .subtitle(glib::markup_escape_text(&app.executable))
            .build();
        row.add_prefix(&create_app_icon(&app.icon));

        let sw = gtk::Switch::builder().valign(gtk::Align::Center).build();
        let is_blocked = current_app_action(&config.borrow(), &app.executable) == RuleAction::Block;
        sw.set_active(is_blocked);

        let config_ref = config.clone();
        let controller_ref = controller.clone();
        let executable = app.executable.clone();
        let refresh_rules = refresh_rule_views.clone();
        let refresh_counts = refresh_traffic_rule_counts_fn.clone();
        sw.connect_active_notify(move |sw| {
            let currently_blocked =
                current_app_action(&config_ref.borrow(), &executable) == RuleAction::Block;
            if sw.is_active() == currently_blocked {
                return;
            }
            let mut current = config_ref.borrow_mut();
            if sw.is_active() {
                current.settings.app_rules.retain(|r| {
                    r.executable.file_name().and_then(|n| n.to_str()) != Some(&executable)
                });
                current.settings.app_rules.push(AppRule {
                    executable: PathBuf::from(&executable),
                    action: RuleAction::Block,
                });
            } else {
                current.settings.app_rules.retain(|r| {
                    r.executable.file_name().and_then(|n| n.to_str()) != Some(&executable)
                });
            }
            let _ = current.save();
            drop(current);
            controller_ref.borrow().sync_rules();
            if let Some(refresh) = refresh_rules.borrow().as_ref() {
                refresh();
            }
            if let Some(refresh) = refresh_counts.borrow().as_ref() {
                refresh();
            }
        });

        row.add_suffix(&sw);
        row.set_activatable_widget(Some(&sw));
        rules_view.blocked_apps_group.add(&row);

        rules_view.app_switches.borrow_mut().push((
            format!("{} {}", app.name, app.executable).to_lowercase(),
            row,
            sw,
        ));
    }

    {
        let app_switches = rules_view.app_switches.clone();
        rules_view.app_search_row.connect_changed(move |entry| {
            let query = entry.text().trim().to_lowercase();
            for (key, row, _) in app_switches.borrow().iter() {
                row.set_visible(query.is_empty() || key.contains(&query));
            }
        });
    }

    let refresh_blocked_impl: Rc<dyn Fn()> = {
        let procs_group = rules_view.procs_group.clone();
        let procs_rows = rules_view.procs_rows.clone();
        let blocked_targets_group = rules_view.blocked_targets_group.clone();
        let blocked_targets_rows = rules_view.blocked_targets_rows.clone();
        let app_switches = rules_view.app_switches.clone();
        let config = config.clone();
        let controller = controller.clone();
        let refresh_blocked = refresh_blocked_views.clone();
        let refresh_rules = refresh_rule_views.clone();
        let refresh_counts = refresh_traffic_rule_counts_fn.clone();

        Rc::new(move || {
            for row in procs_rows.borrow_mut().drain(..) {
                procs_group.remove(&row);
            }
            let current = config.borrow();
            let procs: Vec<String> = current
                .settings
                .app_rules
                .iter()
                .filter(|r| r.action == RuleAction::Block)
                .filter_map(|r| {
                    r.executable
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(ToString::to_string)
                })
                .collect();

            for row in blocked_targets_rows.borrow_mut().drain(..) {
                blocked_targets_group.remove(&row);
            }
            let blocked_domains: Vec<DomainRule> = current
                .settings
                .domain_rules
                .iter()
                .filter(|r| r.action == RuleAction::Block)
                .cloned()
                .collect();
            let blocked_ips: Vec<IpRule> = current
                .settings
                .ip_rules
                .iter()
                .filter(|r| r.action == RuleAction::Block)
                .cloned()
                .collect();
            drop(current);

            for proc_name in procs {
                let row = adw::ActionRow::builder()
                    .title(&proc_name)
                    .build();
                let icon = gtk::Image::from_icon_name("network-offline-symbolic");
                icon.set_valign(gtk::Align::Center);
                row.add_prefix(&icon);

                let del_btn = gtk::Button::from_icon_name("user-trash-symbolic");
                del_btn.add_css_class("flat");
                del_btn.add_css_class("destructive-action");
                del_btn.set_valign(gtk::Align::Center);
                del_btn.set_tooltip_text(Some(tr("rules.domain.remove_source")));

                let target = proc_name.clone();
                let config_ref = config.clone();
                let controller_ref = controller.clone();
                let refresh_blocked_ref = refresh_blocked.clone();
                let refresh_rules_ref = refresh_rules.clone();
                let refresh_counts_ref = refresh_counts.clone();
                del_btn.connect_clicked(move |_| {
                    let mut current = config_ref.borrow_mut();
                    current.settings.app_rules.retain(|r| {
                        r.executable.file_name().and_then(|n| n.to_str()) != Some(&target)
                    });
                    let _ = current.save();
                    drop(current);
                    controller_ref.borrow().sync_rules();
                    if let Some(refresh) = refresh_blocked_ref.borrow().as_ref() {
                        refresh();
                    }
                    if let Some(refresh) = refresh_rules_ref.borrow().as_ref() {
                        refresh();
                    }
                    if let Some(refresh) = refresh_counts_ref.borrow().as_ref() {
                        refresh();
                    }
                });
                row.add_suffix(&del_btn);
                procs_group.add(&row);
                procs_rows.borrow_mut().push(row);
            }

            for rule in blocked_domains {
                let row = adw::ActionRow::builder()
                    .title(&rule.pattern)
                    .subtitle(domain_kind_label(rule.kind))
                    .build();
                let icon = gtk::Image::from_icon_name("network-server-symbolic");
                icon.set_valign(gtk::Align::Center);
                row.add_prefix(&icon);

                let del_btn = gtk::Button::from_icon_name("user-trash-symbolic");
                del_btn.add_css_class("flat");
                del_btn.add_css_class("destructive-action");
                del_btn.set_valign(gtk::Align::Center);
                del_btn.set_tooltip_text(Some(tr("rules.domain.remove_source")));

                let pattern = rule.pattern.clone();
                let kind = rule.kind;
                let config_ref = config.clone();
                let controller_ref = controller.clone();
                let refresh_blocked_ref = refresh_blocked.clone();
                let refresh_rules_ref = refresh_rules.clone();
                let refresh_counts_ref = refresh_counts.clone();
                del_btn.connect_clicked(move |_| {
                    let mut current = config_ref.borrow_mut();
                    current
                        .settings
                        .domain_rules
                        .retain(|r| !(r.pattern == pattern && r.kind == kind));
                    let _ = current.save();
                    drop(current);
                    controller_ref.borrow().sync_rules();
                    if let Some(refresh) = refresh_blocked_ref.borrow().as_ref() {
                        refresh();
                    }
                    if let Some(refresh) = refresh_rules_ref.borrow().as_ref() {
                        refresh();
                    }
                    if let Some(refresh) = refresh_counts_ref.borrow().as_ref() {
                        refresh();
                    }
                });
                row.add_suffix(&del_btn);
                blocked_targets_group.add(&row);
                blocked_targets_rows.borrow_mut().push(row);
            }

            for rule in blocked_ips {
                let row = adw::ActionRow::builder()
                    .title(&rule.network.to_string())
                    .subtitle("IP-CIDR")
                    .build();
                let icon = gtk::Image::from_icon_name("network-server-symbolic");
                icon.set_valign(gtk::Align::Center);
                row.add_prefix(&icon);

                let del_btn = gtk::Button::from_icon_name("user-trash-symbolic");
                del_btn.add_css_class("flat");
                del_btn.add_css_class("destructive-action");
                del_btn.set_valign(gtk::Align::Center);
                del_btn.set_tooltip_text(Some(tr("rules.domain.remove_source")));

                let network = rule.network;
                let config_ref = config.clone();
                let controller_ref = controller.clone();
                let refresh_blocked_ref = refresh_blocked.clone();
                let refresh_rules_ref = refresh_rules.clone();
                let refresh_counts_ref = refresh_counts.clone();
                del_btn.connect_clicked(move |_| {
                    let mut current = config_ref.borrow_mut();
                    current.settings.ip_rules.retain(|r| r.network != network);
                    let _ = current.save();
                    drop(current);
                    controller_ref.borrow().sync_rules();
                    if let Some(refresh) = refresh_blocked_ref.borrow().as_ref() {
                        refresh();
                    }
                    if let Some(refresh) = refresh_rules_ref.borrow().as_ref() {
                        refresh();
                    }
                    if let Some(refresh) = refresh_counts_ref.borrow().as_ref() {
                        refresh();
                    }
                });
                row.add_suffix(&del_btn);
                blocked_targets_group.add(&row);
                blocked_targets_rows.borrow_mut().push(row);
            }

            for (_, row, sw) in app_switches.borrow().iter() {
                if let Some(exec) = row.subtitle().map(|s| s.to_string()) {
                    let is_blocked =
                        current_app_action(&config.borrow(), &exec) == RuleAction::Block;
                    if sw.is_active() != is_blocked {
                        sw.set_active(is_blocked);
                    }
                }
            }
        })
    };
    *refresh_blocked_views.borrow_mut() = Some(refresh_blocked_impl.clone());
    refresh_blocked_impl();
}
