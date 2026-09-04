from __future__ import annotations

from typing import Callable, List, Optional

import gi
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gtk

from ..models import Profile


class ProfileEditorWindow(Adw.PreferencesWindow):
    """Profile 配置编辑与新建窗口"""

    METHODS = ["auto", "nat", "tproxy", "nft"]
    VERBOSITIES = ["normal", "verbose", "very_verbose"]
    VERBOSITY_LABELS = ["Normal", "Verbose", "Very Verbose"]

    def __init__(
        self,
        profile: Optional[Profile] = None,
        parent: Optional[Gtk.Window] = None,
        on_save: Optional[Callable[[Profile], None]] = None,
        on_delete: Optional[Callable[[Profile], None]] = None,
    ) -> None:
        super().__init__()
        self.set_transient_for(parent)
        self.set_modal(True)
        self.set_default_size(460, 620)

        self.original_profile = profile
        self.is_new = profile is None
        self.on_save = on_save
        self.on_delete = on_delete

        self.excludes: List[str] = list(profile.exclude) if profile else []

        page_title = "New Profile" if self.is_new else "Edit Profile"
        self.set_title(page_title)

        # 构建主配置页
        page = Adw.PreferencesPage()
        self.add(page)

        # SSH 分组
        ssh_group = Adw.PreferencesGroup(title="SSH")
        page.add(ssh_group)

        self.name_row = Adw.EntryRow(title="Name")
        self.name_row.set_text(profile.name if profile else "New Server")
        ssh_group.add(self.name_row)

        self.host_row = Adw.EntryRow(title="Host")
        self.host_row.set_text(profile.host if profile else "")
        ssh_group.add(self.host_row)

        self.port_row = Adw.SpinRow.new_with_range(1, 65535, 1)
        self.port_row.set_title("Port")
        self.port_row.set_value(profile.port if profile else 22)
        ssh_group.add(self.port_row)

        self.user_row = Adw.EntryRow(title="Username")
        self.user_row.set_text(profile.username if profile else "")
        ssh_group.add(self.user_row)

        # 路由与网络分组
        routing_group = Adw.PreferencesGroup(title="Routing")
        page.add(routing_group)

        self.routes_row = Adw.EntryRow(title="Remote Routes")
        routes_val = ", ".join(profile.routes) if (profile and profile.routes) else "0.0.0.0/0"
        self.routes_row.set_text(routes_val)
        routing_group.add(self.routes_row)

        self.dns_row = Adw.SwitchRow(title="DNS Forwarding")
        self.dns_row.set_active(profile.dns if profile else True)
        routing_group.add(self.dns_row)

        self.ipv6_row = Adw.SwitchRow(title="IPv6")
        self.ipv6_row.set_active(profile.ipv6 if profile else False)
        routing_group.add(self.ipv6_row)

        # 排除网络分组
        self.exclude_group = Adw.PreferencesGroup(title="Exclude Networks")
        page.add(self.exclude_group)

        self.exclude_rows_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        self.exclude_group.add(self.exclude_rows_box)

        # 添加排除网段输入行
        self.new_exclude_entry = Adw.EntryRow(title="Add Network")
        add_btn = Gtk.Button.new_from_icon_name("list-add-symbolic")
        add_btn.add_css_class("flat")
        add_btn.set_valign(Gtk.Align.CENTER)
        add_btn.connect("clicked", self._on_add_exclude_clicked)
        self.new_exclude_entry.add_suffix(add_btn)
        self.new_exclude_entry.connect("entry-activated", lambda *_: self._on_add_exclude_clicked(add_btn))
        self.exclude_group.add(self.new_exclude_entry)

        self._refresh_excludes_list()

        # 高级选项分组
        adv_group = Adw.PreferencesGroup(title="Advanced")
        page.add(adv_group)

        self.method_row = Adw.ComboRow(title="Method")
        method_model = Gtk.StringList.new(self.METHODS)
        self.method_row.set_model(method_model)
        selected_method = profile.method if profile else "auto"
        if selected_method in self.METHODS:
            self.method_row.set_selected(self.METHODS.index(selected_method))
        adv_group.add(self.method_row)

        self.verbosity_row = Adw.ComboRow(title="Verbosity")
        verb_model = Gtk.StringList.new(self.VERBOSITY_LABELS)
        self.verbosity_row.set_model(verb_model)
        selected_verb = profile.verbosity if profile else "normal"
        if selected_verb in self.VERBOSITIES:
            self.verbosity_row.set_selected(self.VERBOSITIES.index(selected_verb))
        adv_group.add(self.verbosity_row)

        self.auto_connect_row = Adw.SwitchRow(title="Auto Connect")
        self.auto_connect_row.set_active(profile.auto_connect if profile else False)
        adv_group.add(self.auto_connect_row)

        # 底部操作分组：保存与删除
        actions_group = Adw.PreferencesGroup()
        page.add(actions_group)

        save_btn = Gtk.Button(label="Save")
        save_btn.add_css_class("suggested-action")
        save_btn.add_css_class("pill")
        save_btn.set_margin_top(8)
        save_btn.set_margin_bottom(8)
        save_btn.connect("clicked", self._on_save_clicked)
        actions_group.add(save_btn)

        if not self.is_new:
            del_btn = Gtk.Button(label="Delete Profile")
            del_btn.add_css_class("destructive-action")
            del_btn.add_css_class("pill")
            del_btn.set_margin_bottom(12)
            del_btn.connect("clicked", self._on_delete_clicked)
            actions_group.add(del_btn)

    def _refresh_excludes_list(self) -> None:
        """刷新排除网络列表中的项目"""
        child = self.exclude_rows_box.get_first_child()
        while child:
            next_child = child.get_next_sibling()
            self.exclude_rows_box.remove(child)
            child = next_child

        for item in self.excludes:
            row = Adw.ActionRow(title=item)
            del_icon_btn = Gtk.Button.new_from_icon_name("user-trash-symbolic")
            del_icon_btn.add_css_class("flat")
            del_icon_btn.set_valign(Gtk.Align.CENTER)
            del_icon_btn.connect("clicked", self._make_remove_exclude_handler(item))
            row.add_suffix(del_icon_btn)
            self.exclude_rows_box.append(row)

    def _make_remove_exclude_handler(self, item: str) -> Callable[[Gtk.Button], None]:
        """构建排除项删除回调"""
        def handler(button: Gtk.Button) -> None:
            if item in self.excludes:
                self.excludes.remove(item)
                self._refresh_excludes_list()
        return handler

    def _on_add_exclude_clicked(self, button: Gtk.Button) -> None:
        """添加排除网络"""
        text = self.new_exclude_entry.get_text().strip()
        if text and text not in self.excludes:
            self.excludes.append(text)
            self.new_exclude_entry.set_text("")
            self._refresh_excludes_list()

    def _on_save_clicked(self, button: Gtk.Button) -> None:
        """保存 Profile 配置"""
        name = self.name_row.get_text().strip() or "Unnamed"
        host = self.host_row.get_text().strip()
        port = int(self.port_row.get_value())
        username = self.user_row.get_text().strip()

        routes_raw = self.routes_row.get_text().strip()
        routes = [r.strip() for r in routes_raw.split(",") if r.strip()]
        if not routes:
            routes = ["0.0.0.0/0"]

        dns = self.dns_row.get_active()
        ipv6 = self.ipv6_row.get_active()

        method_idx = self.method_row.get_selected()
        method = self.METHODS[method_idx] if 0 <= method_idx < len(self.METHODS) else "auto"

        verb_idx = self.verbosity_row.get_selected()
        verbosity = self.VERBOSITIES[verb_idx] if 0 <= verb_idx < len(self.VERBOSITIES) else "normal"

        auto_connect = self.auto_connect_row.get_active()

        profile_id = self.original_profile.id if self.original_profile else None
        saved_profile = Profile(
            name=name,
            host=host,
            port=port,
            username=username,
            routes=routes,
            exclude=list(self.excludes),
            dns=dns,
            ipv6=ipv6,
            method=method,
            verbosity=verbosity,
            auto_connect=auto_connect,
        )
        if profile_id:
            saved_profile.id = profile_id

        if self.on_save:
            self.on_save(saved_profile)

        self.close()

    def _on_delete_clicked(self, button: Gtk.Button) -> None:
        """删除当前 Profile"""
        if self.original_profile and self.on_delete:
            self.on_delete(self.original_profile)
        self.close()
