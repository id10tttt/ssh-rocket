use adw::prelude::*;
use gtk4 as gtk;
use libadwaita as adw;
use ssh_rocket_core::{AppConfig, RuleAction};
use std::{cell::RefCell, rc::Rc};

use crate::{
    i18n::tr,
    ui::{
        dialogs::{show_rule_dialog, RefreshRules},
        widgets::{create_action_badge, create_kind_badge},
    },
    ListedRule, RuleListState, RULE_BATCH_SIZE,
};

#[derive(Clone)]
pub struct RulesView {
    pub container: gtk::Box,
    pub rules_stack: gtk::Stack,
    pub rules_switcher: gtk::StackSwitcher,

    // Applications tab
    pub applications_page: gtk::Box,
    pub app_search: gtk::SearchEntry,
    pub sort_label: gtk::Label,
    pub app_sort: gtk::DropDown,
    pub applications_group: adw::PreferencesGroup,
    pub app_rows: Rc<RefCell<Vec<(String, String, adw::ComboRow)>>>,

    // Domains & IPs tab
    pub routing_page: gtk::Box,
    pub domain_stack: gtk::Stack,
    pub routing_group: adw::PreferencesGroup,
    pub policy_row: adw::ComboRow,
    pub ipv6_row: adw::SwitchRow,
    pub rule_status_row: adw::ActionRow,
    pub configurations_group: adw::PreferencesGroup,
    pub custom_summary_group: adw::PreferencesGroup,
    pub custom_summary_row: adw::ActionRow,
    pub quick_add_rule_btn: gtk::Button,
    pub import_button: gtk::Button,
    pub rule_source_entry: adw::EntryRow,
    pub import_trigger_btn: gtk::Button,

    // Detail view
    pub detail_back_btn: gtk::Button,
    pub detail_title_lbl: gtk::Label,
    pub source_group: adw::PreferencesGroup,
    pub source_detail_row: adw::ActionRow,
    pub update_source_btn: gtk::Button,
    pub remove_source_btn: gtk::Button,
    pub contents_group: adw::PreferencesGroup,
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
    pub blocked_scroller: gtk::ScrolledWindow,
    pub procs_group: adw::PreferencesGroup,
    pub new_proc_row: adw::EntryRow,
    pub add_proc_btn: gtk::Button,
    pub procs_list_box: gtk::Box,
    pub blocked_targets_group: adw::PreferencesGroup,
    pub new_target_row: adw::EntryRow,
    pub add_target_btn: gtk::Button,
    pub blocked_targets_list_box: gtk::Box,
    pub blocked_apps_group: adw::PreferencesGroup,
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
        rules_page.append(&rules_stack);

        // --- 1. 应用分流 (Applications) ---
        let applications_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let app_toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        app_toolbar.set_margin_start(18);
        app_toolbar.set_margin_end(18);
        app_toolbar.set_margin_top(12);
        app_toolbar.set_margin_bottom(12);
        let app_search = gtk::SearchEntry::builder()
            .placeholder_text(tr("rules.apps.search"))
            .hexpand(true)
            .build();
        app_toolbar.append(&app_search);
        let sort_label = gtk::Label::new(Some(tr("rules.apps.sort")));
        sort_label.add_css_class("dim-label");
        app_toolbar.append(&sort_label);
        let app_sort = gtk::DropDown::from_strings(&[tr("rules.apps.sort_name"), tr("rules.apps.sort_rule")]);
        app_toolbar.append(&app_sort);
        applications_page.append(&app_toolbar);
        applications_page.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        let applications_preferences = adw::PreferencesPage::new();
        let applications_group = adw::PreferencesGroup::builder().title(tr("rules.apps.group")).build();
        let app_rows = Rc::new(RefCell::new(Vec::<(String, String, adw::ComboRow)>::new()));
        applications_preferences.add(&applications_group);

        let app_scroller = gtk::ScrolledWindow::builder()
            .child(&applications_preferences)
            .vexpand(true)
            .build();
        applications_page.append(&app_scroller);
        rules_stack.add_titled(&applications_page, Some("applications"), tr("rules.tab.apps"));

        // --- 2. 域名与 IP 规则 (Domains & IPs) ---
        let routing_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let domain_stack = gtk::Stack::new();
        domain_stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);
        domain_stack.set_vexpand(true);
        routing_page.append(&domain_stack);

        // 2.1 规则总览 (Overview)
        let overview_page = adw::PreferencesPage::new();
        let routing_group = adw::PreferencesGroup::builder().title(tr("rules.domain.default_policy")).build();
        let policy_row = adw::ComboRow::builder()
            .title(tr("rules.domain.unmatched"))
            .model(&gtk::StringList::new(&[tr("action.proxy"), tr("action.direct"), tr("action.block")]))
            .selected(match config.borrow().settings.default_policy {
                RuleAction::Proxy => 0,
                RuleAction::Direct => 1,
                RuleAction::Block => 2,
            })
            .build();
        policy_row.add_prefix(&gtk::Image::from_icon_name("network-workgroup-symbolic"));
        let ipv6_row = adw::SwitchRow::builder()
            .title(tr("rules.domain.ipv6"))
            .active(config.borrow().settings.ipv6)
            .build();
        ipv6_row.add_prefix(&gtk::Image::from_icon_name("network-wired-symbolic"));
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
            .title(tr("rules.domain.source_addr"))
            .text(&initial_rule_source)
            .build();
        let import_trigger_btn = gtk::Button::new();
        import_trigger_btn.set_visible(false);

        let configurations_group = adw::PreferencesGroup::builder().title(tr("rules.domain.remote_group")).build();
        let import_button = gtk::Button::with_label(tr("rules.domain.import_btn"));
        import_button.set_valign(gtk::Align::Center);
        import_button.add_css_class("flat");
        configurations_group.set_header_suffix(Some(&import_button));
        let rule_status_row = adw::ActionRow::new();
        rule_status_row.set_activatable(true);
        rule_status_row.add_prefix(&gtk::Image::from_icon_name("folder-download-symbolic"));
        rule_status_row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        configurations_group.add(&rule_status_row);
        overview_page.add(&configurations_group);

        let custom_summary_group = adw::PreferencesGroup::builder().title(tr("rules.domain.custom_group")).build();
        let quick_add_rule_btn = gtk::Button::from_icon_name("list-add-symbolic");
        quick_add_rule_btn.add_css_class("flat");
        quick_add_rule_btn.set_valign(gtk::Align::Center);
        quick_add_rule_btn.set_tooltip_text(Some(tr("rules.domain.quick_add")));
        custom_summary_group.set_header_suffix(Some(&quick_add_rule_btn));

        let custom_summary_row = adw::ActionRow::builder()
            .title(tr("rules.domain.custom_list"))
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
        detail_header.set_visible(false);
        let detail_back_btn = gtk::Button::from_icon_name("go-previous-symbolic");
        let detail_title_lbl = gtk::Label::new(Some(tr("rules.domain.source_cfg")));
        detail_header.append(&detail_back_btn);
        detail_header.append(&detail_title_lbl);

        let detail_preferences = adw::PreferencesPage::new();
        let source_group = adw::PreferencesGroup::builder().title(tr("rules.domain.source_provider")).build();
        let source_detail_row = adw::ActionRow::new();
        source_detail_row.add_prefix(&gtk::Image::from_icon_name("folder-download-symbolic"));
        let update_source_btn = gtk::Button::from_icon_name("view-refresh-symbolic");
        update_source_btn.add_css_class("flat");
        update_source_btn.set_tooltip_text(Some(tr("rules.domain.update_source")));
        source_detail_row.add_suffix(&update_source_btn);
        let remove_source_btn = gtk::Button::from_icon_name("user-trash-symbolic");
        remove_source_btn.add_css_class("flat");
        remove_source_btn.set_tooltip_text(Some(tr("rules.domain.remove_source")));
        source_detail_row.add_suffix(&remove_source_btn);
        source_group.add(&source_detail_row);
        detail_preferences.add(&source_group);

        let contents_group = adw::PreferencesGroup::builder().title(tr("rules.domain.entries")).build();
        let imported_summary_row = adw::ActionRow::builder()
            .title(tr("rules.domain.view_entries"))
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
        imported_header.set_visible(false);
        let imported_back_btn = gtk::Button::from_icon_name("go-previous-symbolic");
        let imported_title_lbl = gtk::Label::new(Some(tr("rules.domain.imported_title")));
        imported_header.append(&imported_back_btn);
        imported_header.append(&imported_title_lbl);

        let imported_search = gtk::SearchEntry::builder()
            .placeholder_text(tr("rules.domain.imported_search"))
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
        let imported_rules_group = adw::PreferencesGroup::builder().title(tr("rules.domain.imported_group")).build();
        imported_body.append(&imported_rules_group);
        let imported_load_more = gtk::Button::with_label(tr("rules.domain.load_more"));
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
        custom_header.set_visible(false);
        let custom_back_btn = gtk::Button::from_icon_name("go-previous-symbolic");
        let custom_title_lbl = gtk::Label::new(Some(tr("rules.domain.custom_title")));
        custom_header.append(&custom_back_btn);
        custom_header.append(&custom_title_lbl);

        let custom_search = gtk::SearchEntry::builder()
            .placeholder_text(tr("rules.domain.custom_search"))
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
        let custom_rules_group = adw::PreferencesGroup::builder().title(tr("rules.domain.custom_title")).build();
        let custom_actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let import_omega_btn = gtk::Button::with_label(tr("rules.domain.import_omega"));
        import_omega_btn.add_css_class("flat");
        custom_actions.append(&import_omega_btn);
        let clear_rules_btn = gtk::Button::with_label(tr("rules.domain.clear_rules"));
        clear_rules_btn.add_css_class("flat");
        clear_rules_btn.add_css_class("destructive-action");
        custom_actions.append(&clear_rules_btn);
        let add_rule_btn = gtk::Button::with_label(tr("rules.domain.quick_add"));
        add_rule_btn.add_css_class("suggested-action");
        custom_actions.append(&add_rule_btn);
        custom_rules_group.set_header_suffix(Some(&custom_actions));
        custom_body.append(&custom_rules_group);
        let custom_load_more = gtk::Button::with_label(tr("rules.domain.load_more"));
        custom_load_more.set_halign(gtk::Align::Center);
        custom_body.append(&custom_load_more);

        let custom_scroller = gtk::ScrolledWindow::builder()
            .child(&custom_body)
            .vexpand(true)
            .build();
        custom_page.append(&custom_scroller);
        domain_stack.add_named(&custom_page, Some("custom"));

        domain_stack.set_visible_child_name("overview");
        rules_stack.add_titled(&routing_page, Some("routing"), tr("rules.tab.domain"));

        // --- 3. 黑名单策略 (Blocked) ---
        let blocked_page = adw::PreferencesPage::new();

        let procs_group = adw::PreferencesGroup::builder()
            .title(tr("rules.blocked.proc_group"))
            .build();
        let new_proc_row = adw::EntryRow::builder().title(tr("rules.blocked.proc_add")).build();
        let add_proc_btn = gtk::Button::from_icon_name("list-add-symbolic");
        add_proc_btn.add_css_class("flat");
        add_proc_btn.set_valign(gtk::Align::Center);
        new_proc_row.add_suffix(&add_proc_btn);
        procs_group.add(&new_proc_row);

        let procs_list_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        procs_group.add(&procs_list_box);
        blocked_page.add(&procs_group);

        let blocked_targets_group = adw::PreferencesGroup::builder()
            .title(tr("rules.blocked.target_group"))
            .build();
        let new_target_row = adw::EntryRow::builder().title(tr("rules.blocked.target_add")).build();
        let add_target_btn = gtk::Button::from_icon_name("list-add-symbolic");
        add_target_btn.add_css_class("flat");
        add_target_btn.set_valign(gtk::Align::Center);
        new_target_row.add_suffix(&add_target_btn);
        blocked_targets_group.add(&new_target_row);

        let blocked_targets_list_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        blocked_targets_group.add(&blocked_targets_list_box);
        blocked_page.add(&blocked_targets_group);

        let blocked_apps_group = adw::PreferencesGroup::builder()
            .title(tr("rules.blocked.app_group"))
            .build();
        let app_search_row = adw::EntryRow::builder().title(tr("rules.blocked.app_search")).build();
        blocked_apps_group.add(&app_search_row);

        let blocked_apps_list_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        blocked_apps_group.add(&blocked_apps_list_box);
        blocked_page.add(&blocked_apps_group);

        let blocked_scroller = gtk::ScrolledWindow::builder()
            .child(&blocked_page)
            .vexpand(true)
            .build();
        rules_stack.add_titled(&blocked_scroller, Some("blocked"), tr("rules.tab.blocked"));

        let app_switches = Rc::new(RefCell::new(Vec::<(String, adw::ActionRow, gtk::Switch)>::new()));

        Self {
            container: rules_page,
            rules_stack,
            rules_switcher,
            applications_page,
            app_search,
            sort_label,
            app_sort,
            applications_group,
            app_rows,
            routing_page,
            domain_stack,
            routing_group,
            policy_row,
            ipv6_row,
            rule_status_row,
            configurations_group,
            custom_summary_group,
            custom_summary_row,
            quick_add_rule_btn,
            import_button,
            rule_source_entry,
            import_trigger_btn,
            detail_back_btn,
            detail_title_lbl,
            source_group,
            source_detail_row,
            update_source_btn,
            remove_source_btn,
            contents_group,
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
            blocked_scroller,
            procs_group,
            new_proc_row,
            add_proc_btn,
            procs_list_box,
            blocked_targets_group,
            new_target_row,
            add_target_btn,
            blocked_targets_list_box,
            blocked_apps_group,
            app_search_row,
            blocked_apps_list_box,
            app_switches,
        }
    }

    pub fn refresh_labels(&self) {
        self.rules_stack.page(&self.applications_page).set_title(tr("rules.tab.apps"));
        self.rules_stack.page(&self.routing_page).set_title(tr("rules.tab.domain"));
        self.rules_stack.page(&self.blocked_scroller).set_title(tr("rules.tab.blocked"));

        self.app_search.set_placeholder_text(Some(tr("rules.apps.search")));
        self.sort_label.set_text(tr("rules.apps.sort"));
        let sort_sel = self.app_sort.selected();
        self.app_sort.set_model(Some(&gtk::StringList::new(&[
            tr("rules.apps.sort_name"),
            tr("rules.apps.sort_rule"),
        ])));
        self.app_sort.set_selected(sort_sel);
        self.applications_group.set_title(tr("rules.apps.group"));

        for (_, _, row) in self.app_rows.borrow().iter() {
            let sel = row.selected();
            row.set_model(Some(&gtk::StringList::new(&[
                tr("action.direct"),
                tr("action.proxy"),
                tr("action.block"),
            ])));
            row.set_selected(sel);
        }

        self.routing_group.set_title(tr("rules.domain.default_policy"));
        self.policy_row.set_title(tr("rules.domain.unmatched"));
        let pol_sel = self.policy_row.selected();
        self.policy_row.set_model(Some(&gtk::StringList::new(&[
            tr("action.proxy"),
            tr("action.direct"),
            tr("action.block"),
        ])));
        self.policy_row.set_selected(pol_sel);
        self.ipv6_row.set_title(tr("rules.domain.ipv6"));
        self.rule_source_entry.set_title(tr("rules.domain.source_addr"));

        self.configurations_group.set_title(tr("rules.domain.remote_group"));
        self.import_button.set_label(tr("rules.domain.import_btn"));
        self.custom_summary_group.set_title(tr("rules.domain.custom_group"));
        self.quick_add_rule_btn.set_tooltip_text(Some(tr("rules.domain.quick_add")));
        self.custom_summary_row.set_title(tr("rules.domain.custom_list"));

        self.detail_title_lbl.set_text(tr("rules.domain.source_cfg"));
        self.source_group.set_title(tr("rules.domain.source_provider"));
        self.update_source_btn.set_tooltip_text(Some(tr("rules.domain.update_source")));
        self.remove_source_btn.set_tooltip_text(Some(tr("rules.domain.remove_source")));
        self.contents_group.set_title(tr("rules.domain.entries"));
        self.imported_summary_row.set_title(tr("rules.domain.view_entries"));
        self.imported_title_lbl.set_text(tr("rules.domain.imported_title"));
        self.imported_search.set_placeholder_text(Some(tr("rules.domain.imported_search")));
        self.imported_rules_group.set_title(tr("rules.domain.imported_group"));
        self.imported_load_more.set_label(tr("rules.domain.load_more"));

        self.custom_title_lbl.set_text(tr("rules.domain.custom_title"));
        self.custom_search.set_placeholder_text(Some(tr("rules.domain.custom_search")));
        self.custom_rules_group.set_title(tr("rules.domain.custom_title"));
        self.import_omega_btn.set_label(tr("rules.domain.import_omega"));
        self.clear_rules_btn.set_label(tr("rules.domain.clear_rules"));
        self.add_rule_btn.set_label(tr("rules.domain.quick_add"));
        self.custom_load_more.set_label(tr("rules.domain.load_more"));

        self.procs_group.set_title(tr("rules.blocked.proc_group"));
        self.new_proc_row.set_title(tr("rules.blocked.proc_add"));
        self.blocked_targets_group.set_title(tr("rules.blocked.target_group"));
        self.new_target_row.set_title(tr("rules.blocked.target_add"));
        self.blocked_apps_group.set_title(tr("rules.blocked.app_group"));
        self.app_search_row.set_title(tr("rules.blocked.app_search"));
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
            edit.set_tooltip_text(Some(tr("dialog.rule.title_edit")));
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
