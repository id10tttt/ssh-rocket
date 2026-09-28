use adw::prelude::*;
use gtk4::{self as gtk, glib};
use libadwaita as adw;
use std::{cell::RefCell, rc::Rc};

use crate::{
    i18n::tr,
    ui::widgets::{create_app_icon, format_bytes},
    AppTrafficStat,
};

pub struct AppsWidgets {
    pub scroller: gtk::ScrolledWindow,
    pub app_usage_group: adw::PreferencesGroup,
    pub app_traffic_search: gtk::SearchEntry,
    pub traffic_scope_filter: gtk::DropDown,
    pub app_traffic_sort: gtk::DropDown,
    pub app_traffic_list_box: gtk::ListBox,
}

pub fn build_apps_tab() -> AppsWidgets {
    let apps_box = gtk::Box::new(gtk::Orientation::Vertical, 16);
    apps_box.set_margin_start(18);
    apps_box.set_margin_end(18);
    apps_box.set_margin_top(12);
    apps_box.set_margin_bottom(18);

    let app_usage_group = adw::PreferencesGroup::builder()
        .title(tr("traffic.apps.title"))
        .build();

    let app_traffic_toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    app_traffic_toolbar.set_margin_bottom(8);

    let app_traffic_search = gtk::SearchEntry::builder()
        .placeholder_text(tr("traffic.apps.search"))
        .hexpand(true)
        .build();
    app_traffic_toolbar.append(&app_traffic_search);

    let traffic_scope_filter = gtk::DropDown::from_strings(&[
        tr("traffic.apps.filter.all"),
        tr("traffic.apps.filter.proxy"),
        tr("traffic.apps.filter.direct"),
    ]);
    app_traffic_toolbar.append(&traffic_scope_filter);

    let app_traffic_sort = gtk::DropDown::from_strings(&[
        tr("traffic.apps.sort.traffic"),
        tr("traffic.apps.sort.name"),
    ]);
    app_traffic_toolbar.append(&app_traffic_sort);

    app_usage_group.add(&app_traffic_toolbar);

    let app_traffic_list_box = gtk::ListBox::new();
    app_traffic_list_box.add_css_class("boxed-list");
    app_traffic_list_box.set_selection_mode(gtk::SelectionMode::None);
    app_usage_group.add(&app_traffic_list_box);
    apps_box.append(&app_usage_group);

    let apps_scroller = gtk::ScrolledWindow::builder()
        .child(&apps_box)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();

    AppsWidgets {
        scroller: apps_scroller,
        app_usage_group,
        app_traffic_search,
        traffic_scope_filter,
        app_traffic_sort,
        app_traffic_list_box,
    }
}

/// 刷新进程流量列表（支持全部 / 仅代理 / 本地与直连 范围过滤）
pub fn refresh_app_traffic_list(
    app_traffic_data: &Rc<RefCell<Vec<AppTrafficStat>>>,
    app_traffic_search: &gtk::SearchEntry,
    traffic_scope_filter: &gtk::DropDown,
    app_traffic_sort: &gtk::DropDown,
    app_traffic_list_box: &gtk::ListBox,
) {
    let query = app_traffic_search.text().trim().to_lowercase();
    let scope_mode = traffic_scope_filter.selected(); // 0: 全部, 1: 仅代理, 2: 本地与直连

    let mut items: Vec<AppTrafficStat> = app_traffic_data
        .borrow()
        .iter()
        .filter(|item| {
            let matches_query = query.is_empty()
                || item.name.to_lowercase().contains(&query)
                || item.id.to_lowercase().contains(&query);
            if !matches_query {
                return false;
            }

            match scope_mode {
                1 => item.proxy_upload + item.proxy_download > 0,
                2 => {
                    (item.direct_upload + item.direct_download)
                        + (item.local_upload + item.local_download)
                        > 0
                }
                _ => true,
            }
        })
        .cloned()
        .collect();

    if app_traffic_sort.selected() == 0 {
        items.sort_by(|a, b| {
            let val_a = match scope_mode {
                1 => a.proxy_upload + a.proxy_download,
                2 => (a.direct_upload + a.direct_download) + (a.local_upload + a.local_download),
                _ => a.upload + a.download,
            };
            let val_b = match scope_mode {
                1 => b.proxy_upload + b.proxy_download,
                2 => (b.direct_upload + b.direct_download) + (b.local_upload + b.local_download),
                _ => b.upload + b.download,
            };
            val_b.cmp(&val_a)
        });
    } else {
        items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    }
    items.truncate(80);

    while let Some(child) = app_traffic_list_box.first_child() {
        app_traffic_list_box.remove(&child);
    }

    if items.is_empty() {
        let is_en = crate::i18n::current_language() == ssh_rocket_core::Language::English;
        let empty_row = adw::ActionRow::builder()
            .title(if is_en { "No matching process traffic" } else { "暂无符合条件的进程流量记录" })
            .build();
        app_traffic_list_box.append(&empty_row);
        return;
    }

    for item in items {
        let (up_bytes, down_bytes, total_bytes) = match scope_mode {
            1 => (
                item.proxy_upload,
                item.proxy_download,
                item.proxy_upload + item.proxy_download,
            ),
            2 => {
                let u = item.direct_upload + item.local_upload;
                let d = item.direct_download + item.local_download;
                (u, d, u + d)
            }
            _ => (item.upload, item.download, item.upload + item.download),
        };

        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&item.name))
            .subtitle(&format!(
                "↑ {}   ↓ {}",
                format_bytes(up_bytes),
                format_bytes(down_bytes),
            ))
            .build();
        row.add_prefix(&create_app_icon(&item.icon));

        // 右侧高亮总流量（左列：固定宽度右对齐）
        let total_lbl = gtk::Label::builder()
            .label(format_bytes(total_bytes))
            .css_classes(["process-traffic-total", "numeric"])
            .valign(gtk::Align::Center)
            .halign(gtk::Align::End)
            .width_request(85)
            .build();
        row.add_suffix(&total_lbl);

        // 路由走向胶囊徽标（最右侧：固定宽度居中对齐）
        let badge = gtk::Label::builder()
            .label(item.primary_type.label())
            .css_classes([item.primary_type.badge_class()])
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Center)
            .width_request(58)
            .build();
        row.add_suffix(&badge);

        app_traffic_list_box.append(&row);
    }
}
