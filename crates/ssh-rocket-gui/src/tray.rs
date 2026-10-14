use gtk4::{gio, glib};
use glib::variant::{ObjectPath, ToVariant};
use ssh_rocket_core::RuleAction;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

const STATUS_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";
const SPEED_LABEL_GUIDE: &str = "1023K/s↑ 1023K/s↓";

const STATUS_XML: &str = r#"
<node>
  <interface name="org.kde.StatusNotifierItem">
    <property name="Category" type="s" access="read"/>
    <property name="Id" type="s" access="read"/>
    <property name="Title" type="s" access="read"/>
    <property name="Status" type="s" access="read"/>
    <property name="IconName" type="s" access="read"/>
    <property name="Menu" type="o" access="read"/>
    <property name="ItemIsMenu" type="b" access="read"/>
    <property name="XAyatanaLabel" type="s" access="read"/>
    <property name="XAyatanaLabelGuide" type="s" access="read"/>
    <method name="Activate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
    <method name="ContextMenu"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
    <method name="SecondaryActivate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
    <method name="Scroll"><arg type="i" direction="in"/><arg type="s" direction="in"/></method>
    <signal name="NewIcon"/>
    <signal name="NewStatus"><arg type="s"/></signal>
    <signal name="NewTitle"/>
    <signal name="XAyatanaNewLabel"><arg type="s"/><arg type="s"/></signal>
  </interface>
</node>
"#;

const MENU_XML: &str = r#"
<node>
  <interface name="com.canonical.dbusmenu">
    <property name="Version" type="u" access="read"/>
    <property name="TextDirection" type="s" access="read"/>
    <property name="Status" type="s" access="read"/>
    <property name="IconThemePath" type="as" access="read"/>
    <method name="GetLayout">
      <arg type="i" direction="in"/><arg type="i" direction="in"/><arg type="as" direction="in"/>
      <arg type="u" direction="out"/><arg type="(ia{sv}av)" direction="out"/>
    </method>
    <method name="GetGroupProperties">
      <arg type="ai" direction="in"/><arg type="as" direction="in"/><arg type="a(ia{sv})" direction="out"/>
    </method>
    <method name="AboutToShow"><arg type="i" direction="in"/><arg type="b" direction="out"/></method>
    <method name="Event">
      <arg type="i" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="in"/><arg type="u" direction="in"/>
    </method>
    <signal name="LayoutUpdated"><arg type="u"/><arg type="i"/></signal>
  </interface>
</node>
"#;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TrayConnectionState {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
}

struct TrayShared {
    state: Cell<TrayConnectionState>,
    speed_label: RefCell<String>,
    revision: Cell<u32>,
    profiles: Rc<dyn Fn() -> Vec<(String, String, bool)>>,
    select_profile: Rc<dyn Fn(usize)>,
    policy: Rc<dyn Fn() -> RuleAction>,
    select_policy: Rc<dyn Fn(RuleAction)>,
    toggle_connection: Rc<dyn Fn()>,
    show_window: Rc<dyn Fn()>,
    floating_visible: Rc<dyn Fn() -> bool>,
    set_floating_visible: Rc<dyn Fn(bool)>,
    speed_visible: Rc<dyn Fn() -> bool>,
    set_speed_visible: Rc<dyn Fn(bool)>,
    quit: Rc<dyn Fn()>,
}

pub struct TrayManager {
    connection: gio::DBusConnection,
    shared: Rc<TrayShared>,
    status_registration: Option<gio::RegistrationId>,
    menu_registration: Option<gio::RegistrationId>,
}

impl TrayManager {
    pub fn new(
        profiles: Rc<dyn Fn() -> Vec<(String, String, bool)>>,
        select_profile: Rc<dyn Fn(usize)>,
        policy: Rc<dyn Fn() -> RuleAction>,
        select_policy: Rc<dyn Fn(RuleAction)>,
        toggle_connection: Rc<dyn Fn()>,
        show_window: Rc<dyn Fn()>,
        floating_visible: Rc<dyn Fn() -> bool>,
        set_floating_visible: Rc<dyn Fn(bool)>,
        speed_visible: Rc<dyn Fn() -> bool>,
        set_speed_visible: Rc<dyn Fn(bool)>,
        quit: Rc<dyn Fn()>,
    ) -> Result<Self, glib::Error> {
        let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)?;
        let shared = Rc::new(TrayShared {
            state: Cell::new(TrayConnectionState::Disconnected),
            speed_label: RefCell::new(format_speed_label(0, 0)),
            revision: Cell::new(1),
            profiles,
            select_profile,
            policy,
            select_policy,
            toggle_connection,
            show_window,
            floating_visible,
            set_floating_visible,
            speed_visible,
            set_speed_visible,
            quit,
        });

        let status_info = gio::DBusNodeInfo::for_xml(STATUS_XML)?
            .lookup_interface("org.kde.StatusNotifierItem")
            .expect("StatusNotifierItem interface is declared");
        let status_shared = shared.clone();
        let status_registration = connection
            .register_object(STATUS_PATH, &status_info)
            .method_call(move |_, _, _, _, method, _, invocation| {
                match method {
                    "Activate" | "SecondaryActivate" => (status_shared.show_window)(),
                    "ContextMenu" | "Scroll" => {}
                    _ => {}
                }
                invocation.return_value(Some(&().to_variant()));
            })
            .property({
                let shared = shared.clone();
                move |_, _, _, _, property| status_property(&shared, property)
            })
            .build()?;

        let menu_info = gio::DBusNodeInfo::for_xml(MENU_XML)?
            .lookup_interface("com.canonical.dbusmenu")
            .expect("DBusMenu interface is declared");
        let menu_shared = shared.clone();
        let menu_connection = connection.clone();
        let menu_registration = connection
            .register_object(MENU_PATH, &menu_info)
            .method_call(move |_, _, _, _, method, parameters, invocation| {
                match method {
                    "GetLayout" => invocation.return_value(Some(&menu_layout(&menu_shared))),
                    "GetGroupProperties" => {
                        let empty: Vec<(i32, HashMap<String, glib::Variant>)> = Vec::new();
                        invocation.return_value(Some(&(empty,).to_variant()));
                    }
                    "AboutToShow" => invocation.return_value(Some(&(false,).to_variant())),
                    "Event" => {
                        let id = parameters.child_get::<i32>(0);
                        let event = parameters.child_get::<String>(1);
                        if event == "clicked" {
                            if id >= 1000 {
                                (menu_shared.select_profile)((id - 1000) as usize);
                            } else {
                                match id {
                                    201 => (menu_shared.toggle_connection)(),
                                    203 => (menu_shared.show_window)(),
                                    204 => {
                                        let visible = !(menu_shared.floating_visible)();
                                        (menu_shared.set_floating_visible)(visible);
                                    }
                                    205 => (menu_shared.quit)(),
                                    206 => {
                                        let visible = !(menu_shared.speed_visible)();
                                        (menu_shared.set_speed_visible)(visible);
                                        emit_speed_label_changed(&menu_connection, &menu_shared);
                                    }
                                    300 => (menu_shared.select_policy)(RuleAction::Proxy),
                                    301 => (menu_shared.select_policy)(RuleAction::Direct),
                                    302 => (menu_shared.select_policy)(RuleAction::Block),
                                    _ => {}
                                }
                            }
                            emit_menu_changed(&menu_connection, &menu_shared);
                        }
                        invocation.return_value(Some(&().to_variant()));
                    }
                    _ => invocation.return_value(Some(&().to_variant())),
                }
            })
            .property(|_, _, _, _, property| menu_property(property))
            .build()?;

        if let Err(error) = connection.call_sync(
            Some("org.kde.StatusNotifierWatcher"),
            "/StatusNotifierWatcher",
            "org.kde.StatusNotifierWatcher",
            "RegisterStatusNotifierItem",
            Some(&(STATUS_PATH,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            1500,
            gio::Cancellable::NONE,
        ) {
            let _ = connection.unregister_object(status_registration);
            let _ = connection.unregister_object(menu_registration);
            return Err(error);
        }

        Ok(Self {
            connection,
            shared,
            status_registration: Some(status_registration),
            menu_registration: Some(menu_registration),
        })
    }

    pub fn set_state(&self, state: TrayConnectionState) {
        if self.shared.state.replace(state) == state {
            return;
        }
        let _ = self.connection.emit_signal(
            None,
            STATUS_PATH,
            "org.kde.StatusNotifierItem",
            "NewIcon",
            Some(&().to_variant()),
        );
        emit_menu_changed(&self.connection, &self.shared);
    }

    /// 更新托盘中显示的系统总上传和下载速度。
    pub fn set_speed(&self, upload: u64, download: u64) {
        let label = format_speed_label(upload, download);
        let mut current = self.shared.speed_label.borrow_mut();
        if *current == label {
            return;
        }
        *current = label.clone();
        drop(current);

        if (self.shared.speed_visible)() {
            emit_speed_label_changed(&self.connection, &self.shared);
        }
    }

    pub fn refresh_menu(&self) {
        emit_menu_changed(&self.connection, &self.shared);
    }
}

impl Drop for TrayManager {
    fn drop(&mut self) {
        if let Some(registration) = self.status_registration.take() {
            let _ = self.connection.unregister_object(registration);
        }
        if let Some(registration) = self.menu_registration.take() {
            let _ = self.connection.unregister_object(registration);
        }
    }
}

fn status_property(shared: &TrayShared, property: &str) -> glib::Variant {
    match property {
        "Category" => "ApplicationStatus".to_variant(),
        "Id" => "io.github.idi0t.SshRocket".to_variant(),
        "Title" => "SSH Rocket".to_variant(),
        "Status" => "Active".to_variant(),
        "IconName" => match shared.state.get() {
            TrayConnectionState::Connected => "ssh-rocket-symbolic",
            TrayConnectionState::Connecting | TrayConnectionState::Disconnecting => "ssh-rocket-acquiring-symbolic",
            TrayConnectionState::Disconnected => "ssh-rocket-disconnected-symbolic",
        }.to_variant(),
        "Menu" => ObjectPath::try_from(MENU_PATH).expect("valid menu object path").to_variant(),
        "ItemIsMenu" => true.to_variant(),
        "XAyatanaLabel" => {
            if (shared.speed_visible)() {
                shared.speed_label.borrow().as_str().to_variant()
            } else {
                "".to_variant()
            }
        }
        "XAyatanaLabelGuide" => SPEED_LABEL_GUIDE.to_variant(),
        _ => ().to_variant(),
    }
}

fn format_speed_label(upload: u64, download: u64) -> String {
    format!(
        "{}↑ {}↓",
        format_tray_speed(upload),
        format_tray_speed(download)
    )
}

fn format_tray_speed(bytes_per_second: u64) -> String {
    const UNITS: [&str; 5] = ["B/s", "K/s", "M/s", "G/s", "T/s"];

    let mut value = bytes_per_second as f64;
    let mut unit_index = 0;
    while value >= 1024.0 && unit_index < UNITS.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{bytes_per_second}{}", UNITS[unit_index])
    } else if value >= 100.0 {
        format!("{value:.0}{}", UNITS[unit_index])
    } else {
        format!("{value:.1}{}", UNITS[unit_index])
    }
}

fn menu_property(property: &str) -> glib::Variant {
    match property {
        "Version" => 3u32.to_variant(),
        "TextDirection" => "ltr".to_variant(),
        "Status" => "normal".to_variant(),
        "IconThemePath" => Vec::<String>::new().to_variant(),
        _ => ().to_variant(),
    }
}

fn menu_layout(shared: &TrayShared) -> glib::Variant {
    let profiles = (shared.profiles)();
    let mut children = Vec::<glib::Variant>::new();
    let active_profile = profiles.iter().find(|(_, _, active)| *active);
    let endpoint = active_profile.map(|(_, endpoint, _)| endpoint.as_str());
    let profile_name = active_profile
        .map(|(name, _, _)| name.as_str())
        .unwrap_or_else(|| crate::i18n::tr("tray.no_server"));

    children.push(menu_item(203, crate::i18n::tr("tray.open"), true));

    let (connection_label, connection_enabled) = match shared.state.get() {
        TrayConnectionState::Connected => (
            status_label(crate::i18n::tr("tray.connected"), endpoint),
            true,
        ),
        TrayConnectionState::Connecting => (
            status_label(crate::i18n::tr("tray.connecting"), endpoint),
            false,
        ),
        TrayConnectionState::Disconnecting => (
            status_label(crate::i18n::tr("tray.disconnecting"), endpoint),
            false,
        ),
        TrayConnectionState::Disconnected => (
            status_label(crate::i18n::tr("tray.disconnected"), endpoint),
            active_profile.is_some(),
        ),
    };
    children.push(menu_item(201, &connection_label, connection_enabled));
    children.push(menu_separator(202));

    let policy = (shared.policy)();
    let policy_label = format!(
        "{}：{}",
        crate::i18n::tr("tray.global_policy"),
        policy_name(policy)
    );
    children.push(submenu_item(
        210,
        &policy_label,
        vec![
            toggle_menu_item(
                300,
                crate::i18n::tr("action.proxy"),
                true,
                "radio",
                policy == RuleAction::Proxy,
            ),
            toggle_menu_item(
                301,
                crate::i18n::tr("action.direct"),
                true,
                "radio",
                policy == RuleAction::Direct,
            ),
            toggle_menu_item(
                302,
                crate::i18n::tr("action.block"),
                true,
                "radio",
                policy == RuleAction::Block,
            ),
        ],
    ));

    let profile_items = profiles
        .iter()
        .enumerate()
        .map(|(index, (name, _, active))| {
            toggle_menu_item(
                1000 + index as i32,
                name,
                shared.state.get() == TrayConnectionState::Disconnected,
                "radio",
                *active,
            )
        })
        .collect();
    children.push(submenu_item(
        211,
        &format!("{}：{profile_name}", crate::i18n::tr("tray.server")),
        profile_items,
    ));

    children.push(menu_separator(212));
    children.push(toggle_menu_item(
        204,
        crate::i18n::tr("tray.show_floating"),
        true,
        "checkmark",
        (shared.floating_visible)(),
    ));
    children.push(toggle_menu_item(
        206,
        crate::i18n::tr("tray.show_speed"),
        true,
        "checkmark",
        (shared.speed_visible)(),
    ));
    children.push(menu_item(205, crate::i18n::tr("tray.quit"), true));

    let mut root_properties = HashMap::new();
    root_properties.insert("children-display".to_string(), "submenu".to_variant());
    let layout = (0i32, root_properties, children);
    (shared.revision.get(), layout).to_variant()
}

fn status_label(status: &str, endpoint: Option<&str>) -> String {
    endpoint.map_or_else(|| status.to_string(), |value| format!("{status}：{value}"))
}

fn policy_name(policy: RuleAction) -> &'static str {
    match policy {
        RuleAction::Proxy => crate::i18n::tr("action.proxy"),
        RuleAction::Direct => crate::i18n::tr("action.direct"),
        RuleAction::Block => crate::i18n::tr("action.block"),
    }
}

fn menu_item(id: i32, label: &str, enabled: bool) -> glib::Variant {
    let mut properties = HashMap::new();
    properties.insert("label".to_string(), label.to_variant());
    properties.insert("enabled".to_string(), enabled.to_variant());
    properties.insert("visible".to_string(), true.to_variant());
    (id, properties, Vec::<glib::Variant>::new()).to_variant()
}

fn submenu_item(id: i32, label: &str, children: Vec<glib::Variant>) -> glib::Variant {
    let mut properties = HashMap::new();
    properties.insert("label".to_string(), label.to_variant());
    properties.insert("enabled".to_string(), (!children.is_empty()).to_variant());
    properties.insert("visible".to_string(), true.to_variant());
    properties.insert("children-display".to_string(), "submenu".to_variant());
    (id, properties, children).to_variant()
}

fn toggle_menu_item(
    id: i32,
    label: &str,
    enabled: bool,
    toggle_type: &str,
    active: bool,
) -> glib::Variant {
    let mut properties = HashMap::new();
    properties.insert("label".to_string(), label.to_variant());
    properties.insert("enabled".to_string(), enabled.to_variant());
    properties.insert("visible".to_string(), true.to_variant());
    properties.insert("toggle-type".to_string(), toggle_type.to_variant());
    properties.insert("toggle-state".to_string(), i32::from(active).to_variant());
    (id, properties, Vec::<glib::Variant>::new()).to_variant()
}

fn menu_separator(id: i32) -> glib::Variant {
    let mut properties = HashMap::new();
    properties.insert("type".to_string(), "separator".to_variant());
    properties.insert("visible".to_string(), true.to_variant());
    (id, properties, Vec::<glib::Variant>::new()).to_variant()
}

fn emit_menu_changed(connection: &gio::DBusConnection, shared: &TrayShared) {
    let revision = shared.revision.get().wrapping_add(1).max(1);
    shared.revision.set(revision);
    let _ = connection.emit_signal(
        None,
        MENU_PATH,
        "com.canonical.dbusmenu",
        "LayoutUpdated",
        Some(&(revision, 0i32).to_variant()),
    );
}

fn emit_speed_label_changed(connection: &gio::DBusConnection, shared: &TrayShared) {
    let label = if (shared.speed_visible)() {
        shared.speed_label.borrow().clone()
    } else {
        String::new()
    };
    let _ = connection.emit_signal(
        None,
        STATUS_PATH,
        "org.kde.StatusNotifierItem",
        "XAyatanaNewLabel",
        Some(&(label.as_str(), SPEED_LABEL_GUIDE).to_variant()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_compact_tray_speed() {
        assert_eq!(format_tray_speed(0), "0B/s");
        assert_eq!(format_tray_speed(1024), "1.0K/s");
        assert_eq!(format_tray_speed(14 * 1024), "14.0K/s");
        assert_eq!(format_tray_speed(120 * 1024), "120K/s");
        assert_eq!(format_tray_speed(3 * 1024 * 1024), "3.0M/s");
    }

    #[test]
    fn formats_single_line_speed_label() {
        assert_eq!(
            format_speed_label(14 * 1024, 2 * 1024),
            "14.0K/s↑ 2.0K/s↓"
        );
    }

    #[test]
    fn declares_ayatana_speed_label_properties() {
        let node = gio::DBusNodeInfo::for_xml(STATUS_XML).expect("valid StatusNotifierItem XML");
        let interface = node
            .lookup_interface("org.kde.StatusNotifierItem")
            .expect("StatusNotifierItem interface");

        assert!(interface.lookup_property("XAyatanaLabel").is_some());
        assert!(interface.lookup_property("XAyatanaLabelGuide").is_some());
        assert!(interface.lookup_signal("XAyatanaNewLabel").is_some());
    }
}
