use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use ssh_rocket_core::{AppConfig, RuleAction};
use std::{cell::RefCell, rc::Rc};

use crate::{
    ui::{
        dialogs::{show_rule_dialog, RefreshRules},
        widgets::{create_action_badge, create_kind_badge},
    },
    ListedRule, RuleListState, RULE_BATCH_SIZE,
};

pub struct RulesView {
    pub container: gtk::Box,
    pub rules_stack: gtk::Stack,

    // Applications tab
    pub app_search: gtk::SearchEntry,
    pub app_sort: gtk::DropDown,
    pub applications_group: adw::PreferencesGroup,
    pub app_rows: Rc<RefCell<Vec<(String, String, adw::ComboRow)>>>,

    // Domains & IPs tab
    pub domain_stack: gtk::Stack,
    pub policy_row: adw::ComboRow,
    pub ipv6_row: adw::SwitchRow,
    pub rule_status_row: adw::ActionRow,
    pub custom_summary_row: adw::ActionRow,
    pub import_button: gtk::Button,
    pub rule_source_entry: adw::EntryRow,
    pub import_trigger_btn: gtk::Button,

    // Detail view
    pub detail_back_btn: gtk::Button,
    pub detail_title_lbl: gtk::Label,
    pub source_detail_row: adw::ActionRow,
    pub update_source_btn: gtk::Button,
    pub remove_source_btn: gtk::Button,
    pub imported_summary_row: adw::ActionRow,

    // Imported rules
    pub imported_back_btn: gtk::Button,
    pub imported_title_lbl: gtk::Label,
    pub imported_search: gtk::SearchEntry,
    pub imported_rules_group: adw::PreferencesGroup,
    pub imported_load_more: gtk::Button,
    pub imported_scroller: gtk::ScrolledWindow,

    // Custom rules
    pub custom_back_btn: gtk::Button,
    pub custom_title_lbl: gtk::Label,
    pub custom_search: gtk::SearchEntry,
    pub custom_rules_group: adw::PreferencesGroup,
    pub custom_load_more: gtk::Button,
    pub custom_scroller: gtk::ScrolledWindow,
    pub add_rule_btn: gtk::Button,
    pub import_omega_btn: gtk::Button,
    pub clear_rules_btn: gtk::Button,

    // Blocked tab
    pub new_proc_row: adw::EntryRow,
    pub add_proc_btn: gtk::Button,
    pub procs_list_box: gtk::Box,
    pub new_target_row: adw::EntryRow,
    pub add_target_btn: gtk::Button,
    pub blocked_targets_list_box: gtk::Box,
    pub app_search_row: adw::EntryRow,
    pub blocked_apps_list_box: gtk::Box,
    pub app_switches: Rc<RefCell<Vec<(String, adw::ActionRow, gtk::Switch)>>>,
}

impl RulesView {
    pub fn new(config: &Rc<RefCell<AppConfig>>) -> Self {
        let rules_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let rules_stack = gtk::Stack::new();
        rules_stack.set_vexpand(true);
        let rules_switcher = gtk::StackSwitcher::new();
        rules_switcher.set_stack(Some(&rules_stack));
        rules_switcher.set_halign(gtk::Align::Center);
        rules_switcher.set_margin_top(12);
        rules_switcher.set_margin_bottom(12);
        rules_page.append(&rules_switcher);
        rules_page.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        rules_page.append(&rules_stack);

        // --- 1. 应用分流 (Applications) ---
        let applications_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let app_toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        app_toolbar.set_margin_start(18);
        app_toolbar.set_margin_end(18);
        app_toolbar.set_margin_top(12);
        app_toolbar.set_margin_bottom(12);
        let app_search = gtk::SearchEntry::builder()
            .placeholder_text("搜索已安装应用")
            .hexpand(true)
            .build();
        app_toolbar.append(&app_search);
        let sort_label = gtk::Label::new(Some("排序"));
        sort_label.add_css_class("dim-label");
        app_toolbar.append(&sort_label);
        let app_sort = gtk::DropDown::from_strings(&["按名称", "按分流规则"]);
        app_toolbar.append(&app_sort);
        applications_page.append(&app_toolbar);
        applications_page.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        let applications_preferences = adw::PreferencesPage::new();
        let applications_group = adw::PreferencesGroup::builder().title("桌面应用程序").build();
        let app_rows = Rc::new(RefCell::new(Vec::<(String, String, adw::ComboRow)>::new()));
        applications_preferences.add(&applications_group);

        let app_scroller = gtk::ScrolledWindow::builder()
            .child(&applications_preferences)
            .vexpand(true)
            .build();
        applications_page.append(&app_scroller);
        rules_stack.add_titled(&applications_page, Some("applications"), "应用分流");

        // --- 2. 域名与 IP 规则 (Domains & IPs) ---
        let routing_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let domain_stack = gtk::Stack::new();
        domain_stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
        domain_stack.set_vexpand(true);
        routing_page.append(&domain_stack);

        // 2.1 规则总览 (Overview)
        let overview_page = adw::PreferencesPage::new();
        let routing_group = adw::PreferencesGroup::builder().title("全局默认策略").build();
        let policy_row = adw::ComboRow::builder()
            .title("未匹配流量")
            .model(&gtk::StringList::new(&["代理 (PROXY)", "直连 (DIRECT)", "拦截 (REJECT)"]))
            .selected(match config.borrow().settings.default_policy {
                RuleAction::Proxy => 0,
                RuleAction::Direct => 1,
                RuleAction::Block => 2,
            })
            .build();
        let ipv6_row = adw::SwitchRow::builder()
            .title("IPv6 路由分流")
            .active(config.borrow().settings.ipv6)
            .build();
        routing_group.add(&policy_row);
        routing_group.add(&ipv6_row);
        overview_page.add(&routing_group);

        let initial_rule_source = {
            let current = config.borrow();
            if current.settings.rule_source_url.is_empty() {
                crate::DEFAULT_RULE_SOURCE.to_string()
            } else {
                current.settings.rule_source_url.clone()
            }
        };
        let rule_source_entry = adw::EntryRow::builder()
            .title("Shadowrocket 规则订阅地址")
            .text(&initial_rule_source)
            .build();
        let import_trigger_btn = gtk::Button::new();
        import_trigger_btn.set_visible(false);

        let configurations_group = adw::PreferencesGroup::builder().title("远程规则订阅").build();
        let import_button = gtk::Button::with_label("导入…");
        import_button.set_valign(gtk::Align::Center);
        import_button.add_css_class("suggested-action");
        configurations_group.set_header_suffix(Some(&import_button));
        let rule_status_row = adw::ActionRow::new();
        rule_status_row.set_activatable(true);
        rule_status_row.add_prefix(&gtk::Image::from_icon_name("object-select-symbolic"));
        rule_status_row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        configurations_group.add(&rule_status_row);
        overview_page.add(&configurations_group);

        let custom_summary_group = adw::PreferencesGroup::builder().title("用户自定义规则").build();
        let custom_summary_row = adw::ActionRow::builder()
            .title("自定义分流列表")
            .activatable(true)
            .build();
        custom_summary_row.add_prefix(&gtk::Image::from_icon_name("document-edit-symbolic"));
        custom_summary_row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        custom_summary_group.add(&custom_summary_row);
        overview_page.add(&custom_summary_group);

        let overview_scroller = gtk::ScrolledWindow::builder()
            .child(&overview_page)
            .vexpand(true)
            .build();
        domain_stack.add_named(&overview_scroller, Some("overview"));

        // 2.2 订阅详情 (Detail)
        let detail_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let detail_header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        detail_header.set_margin_start(12);
        detail_header.set_margin_end(12);
        detail_header.set_margin_top(8);
        detail_header.set_margin_bottom(8);
        let detail_back_btn = gtk::Button::from_icon_name("go-previous-symbolic");
        detail_back_btn.add_css_class("flat");
        detail_back_btn.set_tooltip_text(Some("返回"));
        detail_header.append(&detail_back_btn);
        let detail_title_lbl = gtk::Label::new(Some("订阅配置"));
        detail_title_lbl.add_css_class("title-4");
        detail_title_lbl.set_halign(gtk::Align::Start);
        detail_header.append(&detail_title_lbl);
        detail_page.append(&detail_header);
        detail_page.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        let detail_preferences = adw::PreferencesPage::new();
        let source_group = adw::PreferencesGroup::builder().title("订阅源").build();
        let source_detail_row = adw::ActionRow::new();
        source_detail_row.add_prefix(&gtk::Image::from_icon_name("folder-download-symbolic"));
        let update_source_btn = gtk::Button::from_icon_name("view-refresh-symbolic");
        update_source_btn.add_css_class("flat");
        update_source_btn.set_tooltip_text(Some("更新订阅"));
        source_detail_row.add_suffix(&update_source_btn);
        let remove_source_btn = gtk::Button::from_icon_name("user-trash-symbolic");
        remove_source_btn.add_css_class("flat");
        remove_source_btn.set_tooltip_text(Some("删除配置"));
        source_detail_row.add_suffix(&remove_source_btn);
        source_group.add(&source_detail_row);
        detail_preferences.add(&source_group);

        let contents_group = adw::PreferencesGroup::builder().title("规则条目").build();
        let imported_summary_row = adw::ActionRow::builder()
            .title("查看订阅规则")
            .activatable(true)
            .build();
        imported_summary_row.add_prefix(&gtk::Image::from_icon_name("view-list-symbolic"));
        imported_summary_row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        contents_group.add(&imported_summary_row);
        detail_preferences.add(&contents_group);

        let detail_scroller = gtk::ScrolledWindow::builder()
            .child(&detail_preferences)
            .vexpand(true)
            .build();
        detail_page.append(&detail_scroller);
        domain_stack.add_named(&detail_page, Some("detail"));

        // 2.3 订阅规则列表 (Imported)
        let imported_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let imported_header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        imported_header.set_margin_start(12);
        imported_header.set_margin_end(12);
        imported_header.set_margin_top(8);
        imported_header.set_margin_bottom(8);
        let imported_back_btn = gtk::Button::from_icon_name("go-previous-symbolic");
        imported_back_btn.add_css_class("flat");
        imported_back_btn.set_tooltip_text(Some("返回"));
        imported_header.append(&imported_back_btn);
        let imported_title_lbl = gtk::Label::new(Some("订阅规则条目"));
        imported_title_lbl.add_css_class("title-4");
        imported_header.append(&imported_title_lbl);
        imported_page.append(&imported_header);
        imported_page.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        let imported_search = gtk::SearchEntry::builder()
            .placeholder_text("搜索订阅规则")
            .build();
        imported_search.set_margin_start(18);
        imported_search.set_margin_end(18);
        imported_search.set_margin_top(12);
        imported_search.set_margin_bottom(12);
        imported_page.append(&imported_search);

        let imported_body = gtk::Box::new(gtk::Orientation::Vertical, 12);
        imported_body.set_margin_start(18);
        imported_body.set_margin_end(18);
        imported_body.set_margin_bottom(18);
        let imported_rules_group = adw::PreferencesGroup::builder().title("已导入条目").build();
        imported_body.append(&imported_rules_group);
        let imported_load_more = gtk::Button::with_label("加载更多");
        imported_load_more.set_halign(gtk::Align::Center);
        imported_body.append(&imported_load_more);

        let imported_scroller = gtk::ScrolledWindow::builder()
            .child(&imported_body)
            .vexpand(true)
            .build();
        imported_page.append(&imported_scroller);
        domain_stack.add_named(&imported_page, Some("imported"));

        // 2.4 用户自定义规则列表 (Custom)
        let custom_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let custom_header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        custom_header.set_margin_start(12);
        custom_header.set_margin_end(12);
        custom_header.set_margin_top(8);
        custom_header.set_margin_bottom(8);
        let custom_back_btn = gtk::Button::from_icon_name("go-previous-symbolic");
        custom_back_btn.add_css_class("flat");
        custom_back_btn.set_tooltip_text(Some("返回"));
        custom_header.append(&custom_back_btn);
        let custom_title_lbl = gtk::Label::new(Some("自定义分流规则"));
        custom_title_lbl.add_css_class("title-4");
        custom_header.append(&custom_title_lbl);
        custom_page.append(&custom_header);
        custom_page.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        let custom_search = gtk::SearchEntry::builder()
            .placeholder_text("搜索自定义规则")
            .build();
        custom_search.set_margin_start(18);
        custom_search.set_margin_end(18);
        custom_search.set_margin_top(12);
        custom_search.set_margin_bottom(12);
        custom_page.append(&custom_search);

        let custom_body = gtk::Box::new(gtk::Orientation::Vertical, 12);
        custom_body.set_margin_start(18);
        custom_body.set_margin_end(18);
        custom_body.set_margin_bottom(18);
        let custom_rules_group = adw::PreferencesGroup::builder().title("规则列表").build();
        let custom_actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let import_omega_btn = gtk::Button::with_label("导入 Omega 备份");
        custom_actions.append(&import_omega_btn);
        let clear_rules_btn = gtk::Button::with_label("清空");
        clear_rules_btn.add_css_class("destructive-action");
        custom_actions.append(&clear_rules_btn);
        let add_rule_btn = gtk::Button::with_label("添加规则");
        add_rule_btn.add_css_class("suggested-action");
        custom_actions.append(&add_rule_btn);
        custom_rules_group.set_header_suffix(Some(&custom_actions));
        custom_body.append(&custom_rules_group);
        let custom_load_more = gtk::Button::with_label("加载更多");
        custom_load_more.set_halign(gtk::Align::Center);
        custom_body.append(&custom_load_more);

        let custom_scroller = gtk::ScrolledWindow::builder()
            .child(&custom_body)
            .vexpand(true)
            .build();
        custom_page.append(&custom_scroller);
        domain_stack.add_named(&custom_page, Some("custom"));

        domain_stack.set_visible_child_name("overview");
        rules_stack.add_titled(&routing_page, Some("routing"), "域名与 IP");

        // --- 3. 黑名单策略 (Blocked) ---
        let blocked_page = adw::PreferencesPage::new();

        let procs_group = adw::PreferencesGroup::builder()
            .title("指定进程拦截")
            .description("完全禁止指定执行文件名建立外部网络连接")
            .build();
        let new_proc_row = adw::EntryRow::builder().title("进程名称").build();
        let add_proc_btn = gtk::Button::from_icon_name("list-add-symbolic");
        add_proc_btn.add_css_class("flat");
        add_proc_btn.set_valign(gtk::Align::Center);
        new_proc_row.add_suffix(&add_proc_btn);
        procs_group.add(&new_proc_row);

        let procs_list_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        procs_group.add(&procs_list_box);
        blocked_page.add(&procs_group);

        let blocked_targets_group = adw::PreferencesGroup::builder()
            .title("黑名单域名与 IP")
            .description("设置为 REJECT 动作的自定义规则")
            .build();
        let new_target_row = adw::EntryRow::builder().title("域名或 IP").build();
        let add_target_btn = gtk::Button::from_icon_name("list-add-symbolic");
        add_target_btn.add_css_class("flat");
        add_target_btn.set_valign(gtk::Align::Center);
        new_target_row.add_suffix(&add_target_btn);
        blocked_targets_group.add(&new_target_row);

        let blocked_targets_list_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        blocked_targets_group.add(&blocked_targets_list_box);
        blocked_page.add(&blocked_targets_group);

        let blocked_apps_group = adw::PreferencesGroup::builder()
            .title("应用程序网络拦截")
            .description("一键禁止已安装桌面应用联网")
            .build();
        let app_search_row = adw::EntryRow::builder().title("搜索应用").build();
        blocked_apps_group.add(&app_search_row);

        let blocked_apps_list_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        blocked_apps_group.add(&blocked_apps_list_box);
        blocked_page.add(&blocked_apps_group);

        let blocked_scroller = gtk::ScrolledWindow::builder()
            .child(&blocked_page)
            .vexpand(true)
            .build();
        rules_stack.add_titled(&blocked_scroller, Some("blocked"), "黑名单");

        let app_switches = Rc::new(RefCell::new(Vec::<(String, adw::ActionRow, gtk::Switch)>::new()));

        Self {
            container: rules_page,
            rules_stack,
            app_search,
            app_sort,
            applications_group,
            app_rows,
            domain_stack,
            policy_row,
            ipv6_row,
            rule_status_row,
            custom_summary_row,
            import_button,
            rule_source_entry,
            import_trigger_btn,
            detail_back_btn,
            detail_title_lbl,
            source_detail_row,
            update_source_btn,
            remove_source_btn,
            imported_summary_row,
            imported_back_btn,
            imported_title_lbl,
            imported_search,
            imported_rules_group,
            imported_load_more,
            imported_scroller,
            custom_back_btn,
            custom_title_lbl,
            custom_search,
            custom_rules_group,
            custom_load_more,
            custom_scroller,
            add_rule_btn,
            import_omega_btn,
            clear_rules_btn,
            new_proc_row,
            add_proc_btn,
            procs_list_box,
            new_target_row,
            add_target_btn,
            blocked_targets_list_box,
            app_search_row,
            blocked_apps_list_box,
            app_switches,
        }
    }
}

/// 填充一批规则到 UI 行
pub fn append_rule_batch(
    group: &adw::PreferencesGroup,
    state: &Rc<RefCell<RuleListState>>,
    load_more: &gtk::Button,
    editable: bool,
    parent: &adw::ApplicationWindow,
    config: &Rc<RefCell<AppConfig>>,
    refresh_rules: &RefreshRules,
) {
    let items = {
        let mut state = state.borrow_mut();
        let end = (state.loaded + RULE_BATCH_SIZE).min(state.filtered.len());
        let items = state.filtered[state.loaded..end].to_vec();
        state.loaded = end;
        items
    };
    for item in items {
        let row = adw::ActionRow::builder()
            .title(item.value())
            .build();
        row.set_use_markup(false);

        let icon_name = match item.action() {
            RuleAction::Proxy => "ssh-rocket-symbolic",
            RuleAction::Direct => "network-wired-symbolic",
            RuleAction::Block => "network-offline-symbolic",
        };
        row.add_prefix(&gtk::Image::from_icon_name(icon_name));

        // 类型与动作胶囊
        row.add_suffix(&create_kind_badge(item.kind_label()));
        row.add_suffix(&create_action_badge(item.action()));

        if editable {
            let edit = gtk::Button::from_icon_name("document-edit-symbolic");
            edit.add_css_class("flat");
            edit.set_tooltip_text(Some("编辑规则"));
            let edit_parent = parent.clone();
            let edit_config = config.clone();
            let edit_rule = item.clone();
            let edit_refresh = refresh_rules.clone();
            edit.connect_clicked(move |_| {
                show_rule_dialog(
                    &edit_parent,
                    edit_config.clone(),
                    Some(edit_rule.clone()),
                    edit_refresh.clone(),
                );
            });
            row.add_suffix(&edit);

            let remove = gtk::Button::from_icon_name("user-trash-symbolic");
            remove.add_css_class("flat");
            remove.set_tooltip_text(Some("删除规则"));
            let remove_config = config.clone();
            let remove_rule = item.clone();
            let remove_refresh = refresh_rules.clone();
            remove.connect_clicked(move |_| {
                let mut current = remove_config.borrow_mut();
                crate::remove_listed_rule(&mut current, &remove_rule);
                if current.save().is_ok() {
                    drop(current);
                    if let Some(refresh) = remove_refresh.borrow().as_ref() {
                        refresh();
                    }
                }
            });
            row.add_suffix(&remove);
        }
        group.add(&row);
        state.borrow_mut().rendered_rows.push(row);
    }
    let state = state.borrow();
    load_more.set_visible(state.loaded < state.filtered.len());
}

/// 重新过滤并刷新规则列表
pub fn refresh_rule_list(
    group: &adw::PreferencesGroup,
    state: &Rc<RefCell<RuleListState>>,
    load_more: &gtk::Button,
    rules: Vec<ListedRule>,
    query: &str,
    editable: bool,
    parent: &adw::ApplicationWindow,
    config: &Rc<RefCell<AppConfig>>,
    refresh_rules: &RefreshRules,
) {
    {
        let mut state = state.borrow_mut();
        for row in state.rendered_rows.drain(..) {
            group.remove(&row);
        }
        let query = query.trim().to_lowercase();
        state.filtered = rules.into_iter().filter(|rule| rule.matches(&query)).collect();
        state.loaded = 0;
    }
    append_rule_batch(
        group,
        state,
        load_more,
        editable,
        parent,
        config,
        refresh_rules,
    );
}
