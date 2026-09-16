use gtk4::{gio, glib};
use glib::variant::{ObjectPath, ToVariant};
use std::{cell::Cell, collections::HashMap, rc::Rc};

const STATUS_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";

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
    <method name="Activate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
    <method name="ContextMenu"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
    <method name="SecondaryActivate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
    <method name="Scroll"><arg type="i" direction="in"/><arg type="s" direction="in"/></method>
    <signal name="NewIcon"/>
    <signal name="NewStatus"><arg type="s"/></signal>
    <signal name="NewTitle"/>
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
    revision: Cell<u32>,
    profiles: Rc<dyn Fn() -> Vec<(String, bool)>>,
    select_profile: Rc<dyn Fn(usize)>,
    toggle_connection: Rc<dyn Fn()>,
    show_window: Rc<dyn Fn()>,
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
        profiles: Rc<dyn Fn() -> Vec<(String, bool)>>,
        select_profile: Rc<dyn Fn(usize)>,
        toggle_connection: Rc<dyn Fn()>,
        show_window: Rc<dyn Fn()>,
        quit: Rc<dyn Fn()>,
    ) -> Result<Self, glib::Error> {
        let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)?;
        let shared = Rc::new(TrayShared {
            state: Cell::new(TrayConnectionState::Disconnected),
            revision: Cell::new(1),
            profiles,
            select_profile,
            toggle_connection,
            show_window,
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
                                    204 => (menu_shared.quit)(),
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
        _ => ().to_variant(),
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
    for (index, (name, active)) in profiles.iter().enumerate() {
        let prefix = if *active { "● " } else { "○ " };
        children.push(menu_item(
            1000 + index as i32,
            &format!("{prefix}{name}"),
            shared.state.get() == TrayConnectionState::Disconnected,
        ));
    }
    if !profiles.is_empty() {
        children.push(menu_separator(200));
    }

    let (toggle_label, enabled) = match shared.state.get() {
        TrayConnectionState::Connected => ("Disconnect", true),
        TrayConnectionState::Connecting => ("Connecting…", false),
        TrayConnectionState::Disconnecting => ("Disconnecting…", false),
        TrayConnectionState::Disconnected => ("Connect", !profiles.is_empty()),
    };
    children.push(menu_item(201, toggle_label, enabled));
    children.push(menu_separator(202));
    children.push(menu_item(203, "Show SSH Rocket", true));
    children.push(menu_item(204, "Quit", true));

    let mut root_properties = HashMap::new();
    root_properties.insert("children-display".to_string(), "submenu".to_variant());
    let layout = (0i32, root_properties, children);
    (shared.revision.get(), layout).to_variant()
}

fn menu_item(id: i32, label: &str, enabled: bool) -> glib::Variant {
    let mut properties = HashMap::new();
    properties.insert("label".to_string(), label.to_variant());
    properties.insert("enabled".to_string(), enabled.to_variant());
    properties.insert("visible".to_string(), true.to_variant());
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
