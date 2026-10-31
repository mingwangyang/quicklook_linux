//! StatusNotifierItem tray icon. The resident broker owns it because it is the
//! only long-lived process; a tray icon that came and went with the preview
//! window would flash in the bar on every Space press.
use anyhow::Result;
use glib::variant::{ToVariant, Variant};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::atomic::{AtomicU32, Ordering},
};

const WATCHER_BUS: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";
const ITEM_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";
/// 托盘图标用自己的彩色图标：蓝底白放大镜。
///
/// 不用主题里的 `system-search` 之类名字，是因为颜色得自己说了算——图标带
/// 自己的底板，浅色和深色主题下都不用跟着面板前景色变，观感一致。主题图标
/// 则要么靠 symbolic 变体让宿主重新着色，要么依赖宿主加载对应的 -dark 主题，
/// 各家实现不一致。
///
/// 前提是图标真的能被查到：PNG/SVG 写进 XDG 图标目录后要重建
/// `icon-theme.cache`（见 hosts::apply_icons），否则宿主按旧缓存查不到这个名字，
/// 会退回自己的兜底图形——本机排查时就卡在这里。
const ICON_NAME: &str = "org.quicklook.Preview";
/// Menu ids. 0 is always the root node.
const ID_SETTINGS: i32 = 1;
const ID_RESTART: i32 = 2;
const ID_SEPARATOR: i32 = 3;
const ID_QUIT: i32 = 4;
const ID_ABOUT: i32 = 5;

const ITEM_XML: &str = r#"<node>
  <interface name="org.kde.StatusNotifierItem">
    <method name="ContextMenu"><arg name="x" type="i" direction="in"/><arg name="y" type="i" direction="in"/></method>
    <method name="Activate"><arg name="x" type="i" direction="in"/><arg name="y" type="i" direction="in"/></method>
    <method name="SecondaryActivate"><arg name="x" type="i" direction="in"/><arg name="y" type="i" direction="in"/></method>
    <method name="Scroll"><arg name="delta" type="i" direction="in"/><arg name="orientation" type="s" direction="in"/></method>
    <signal name="NewTitle"/>
    <signal name="NewIcon"/>
    <signal name="NewAttentionIcon"/>
    <signal name="NewOverlayIcon"/>
    <signal name="NewToolTip"/>
    <signal name="NewStatus"><arg name="status" type="s"/></signal>
    <property name="Category" type="s" access="read"/>
    <property name="Id" type="s" access="read"/>
    <property name="Title" type="s" access="read"/>
    <property name="Status" type="s" access="read"/>
    <property name="WindowId" type="i" access="read"/>
    <property name="IconName" type="s" access="read"/>
    <property name="IconPixmap" type="a(iiay)" access="read"/>
    <property name="OverlayIconName" type="s" access="read"/>
    <property name="OverlayIconPixmap" type="a(iiay)" access="read"/>
    <property name="AttentionIconName" type="s" access="read"/>
    <property name="AttentionIconPixmap" type="a(iiay)" access="read"/>
    <property name="AttentionMovieName" type="s" access="read"/>
    <property name="ToolTip" type="(sa(iiay)ss)" access="read"/>
    <property name="ItemIsMenu" type="b" access="read"/>
    <property name="Menu" type="o" access="read"/>
  </interface>
</node>"#;

const MENU_XML: &str = r#"<node>
  <interface name="com.canonical.dbusmenu">
    <method name="GetLayout">
      <arg name="parentId" type="i" direction="in"/>
      <arg name="recursionDepth" type="i" direction="in"/>
      <arg name="propertyNames" type="as" direction="in"/>
      <arg name="revision" type="u" direction="out"/>
      <arg name="layout" type="(ia{sv}av)" direction="out"/>
    </method>
    <method name="GetGroupProperties">
      <arg name="ids" type="ai" direction="in"/>
      <arg name="propertyNames" type="as" direction="in"/>
      <arg name="properties" type="a(ia{sv})" direction="out"/>
    </method>
    <method name="GetProperty">
      <arg name="id" type="i" direction="in"/>
      <arg name="name" type="s" direction="in"/>
      <arg name="value" type="v" direction="out"/>
    </method>
    <method name="Event">
      <arg name="id" type="i" direction="in"/>
      <arg name="eventId" type="s" direction="in"/>
      <arg name="data" type="v" direction="in"/>
      <arg name="timestamp" type="u" direction="in"/>
    </method>
    <method name="EventGroup">
      <arg name="events" type="a(isvu)" direction="in"/>
      <arg name="idErrors" type="ai" direction="out"/>
    </method>
    <method name="AboutToShow"><arg name="id" type="i" direction="in"/><arg name="needUpdate" type="b" direction="out"/></method>
    <method name="AboutToShowGroup">
      <arg name="ids" type="ai" direction="in"/>
      <arg name="updatesNeeded" type="ai" direction="out"/>
      <arg name="idErrors" type="ai" direction="out"/>
    </method>
    <signal name="ItemsPropertiesUpdated">
      <arg name="updatedProps" type="a(ia{sv})"/>
      <arg name="removedProps" type="a(ias)"/>
    </signal>
    <signal name="LayoutUpdated"><arg name="revision" type="u"/><arg name="parent" type="i"/></signal>
    <property name="Version" type="u" access="read"/>
    <property name="TextDirection" type="s" access="read"/>
    <property name="Status" type="s" access="read"/>
    <property name="IconThemePath" type="as" access="read"/>
  </interface>
</node>"#;

/// What the tray icon can do. The broker supplies these so the tray module does
/// not need to know how the service is wired.
pub struct Actions {
    pub activate: Rc<dyn Fn()>,
    pub settings: Rc<dyn Fn()>,
    pub restart: Rc<dyn Fn()>,
    pub about: Rc<dyn Fn()>,
    pub quit: Rc<dyn Fn()>,
}

pub struct Tray {
    connection: gio::DBusConnection,
    bus: String,
    actions: Actions,
    icon_theme_paths: Vec<String>,
    revision: AtomicU32,
    registrations: RefCell<Vec<gio::RegistrationId>>,
    /// 托盘宿主是否真的收下了这个条目。只要收下，broker 就不能按空闲时间退出：
    /// 它是唯一持有托盘图标的进程，退出等于把托盘（和退出菜单）一起带走。
    registered: std::cell::Cell<bool>,
}
impl Tray {
    pub fn start(connection: &gio::DBusConnection, actions: Actions) -> Result<Rc<Self>> {
        let bus = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
        if !own_name(connection, &bus)? {
            anyhow::bail!("托盘总线名已被占用：{bus}");
        }
        let icon_theme_paths = [quicklook_core::config::xdg_data_home().join("icons/hicolor")]
            .into_iter()
            .filter(|path| path.is_dir())
            .map(|path| path.display().to_string())
            .collect();
        let tray = Rc::new(Self {
            connection: connection.clone(),
            bus,
            actions,
            icon_theme_paths,
            revision: AtomicU32::new(1),
            registrations: RefCell::new(Vec::new()),
            registered: std::cell::Cell::new(false),
        });
        tray.export()?;
        tray.watch()?;
        tray.register_with_watcher();
        Ok(tray)
    }
    pub fn is_registered(&self) -> bool {
        self.registered.get()
    }
    /// Objects live as long as the broker, so the registrations are only kept
    /// for the final unexport.
    fn export(self: &Rc<Self>) -> Result<()> {
        let node = gio::DBusNodeInfo::for_xml(ITEM_XML)?;
        let interface = node
            .lookup_interface("org.kde.StatusNotifierItem")
            .ok_or_else(|| anyhow::anyhow!("托盘接口缺失"))?;
        let calls = self.clone();
        let properties = self.clone();
        let item = self
            .connection
            .register_object(ITEM_PATH, &interface)
            .method_call(move |_, _, _, _, method, _, invocation| {
                calls.invoke(method, invocation);
            })
            .property(move |_, _, _, _, property| properties.property(property))
            .build()?;
        let node = gio::DBusNodeInfo::for_xml(MENU_XML)?;
        let interface = node
            .lookup_interface("com.canonical.dbusmenu")
            .ok_or_else(|| anyhow::anyhow!("菜单接口缺失"))?;
        let calls = self.clone();
        let properties = self.clone();
        let menu = self
            .connection
            .register_object(MENU_PATH, &interface)
            .method_call(move |_, _, _, _, method, parameters, invocation| {
                calls.menu_call(method, &parameters, invocation);
            })
            .property(move |_, _, _, _, property| properties.menu_property(property))
            .build()?;
        self.registrations.borrow_mut().extend([item, menu]);
        Ok(())
    }
    fn invoke(&self, method: &str, invocation: gio::DBusMethodInvocation) {
        match method {
            // A left click has no selection to preview, so it asks the picker.
            "Activate" => (self.actions.activate)(),
            "SecondaryActivate" => (self.actions.settings)(),
            "ContextMenu" | "Scroll" => {}
            _ => {
                invocation.return_dbus_error(
                    "org.quicklook.Error.Unknown",
                    &format!("未知托盘方法 {method}"),
                );
                return;
            }
        }
        invocation.return_value(None);
    }
    fn property(&self, property: &str) -> Variant {
        let pixmaps: Vec<(i32, i32, Vec<u8>)> = Vec::new();
        match property {
            "Category" => "ApplicationStatus".to_variant(),
            "Id" => "quicklook".to_variant(),
            "Title" => "Quick Look".to_variant(),
            "Status" => "Active".to_variant(),
            "WindowId" => 0i32.to_variant(),
            "IconName" => ICON_NAME.to_variant(),
            // 只有 IconPixmap 有内容；覆盖层与提醒图标不单独区分。
            "IconPixmap" | "OverlayIconPixmap" | "AttentionIconPixmap" => pixmaps.to_variant(),
            "OverlayIconName" | "AttentionIconName" | "AttentionMovieName" => "".to_variant(),
            "ToolTip" => (ICON_NAME, pixmaps, "Quick Look", "空格预览文件").to_variant(),
            "ItemIsMenu" => false.to_variant(),
            // GVariant 的文本格式只把裸字面量当字符串，类型为 o 的值必须写成 @o '…'。
            "Menu" => Variant::parse(None, &format!("@o '{MENU_PATH}'"))
                .unwrap_or_else(|_| "".to_variant()),
            _ => "".to_variant(),
        }
    }
    fn menu_property(&self, property: &str) -> Variant {
        match property {
            "Version" => 3u32.to_variant(),
            "TextDirection" => "ltr".to_variant(),
            "Status" => "normal".to_variant(),
            "IconThemePath" => self.icon_theme_paths.to_variant(),
            _ => "".to_variant(),
        }
    }
    fn menu_call(&self, method: &str, parameters: &Variant, invocation: gio::DBusMethodInvocation) {
        match method {
            "GetLayout" => {
                let revision = self.revision.load(Ordering::Relaxed);
                let reply = Variant::tuple_from_iter([revision.to_variant(), Self::layout()]);
                invocation.return_value(Some(&reply));
            }
            "GetGroupProperties" => {
                let ids = parameters
                    .get::<(Vec<i32>, Vec<String>)>()
                    .map(|(ids, _)| ids)
                    .unwrap_or_default();
                let rows: Vec<(i32, std::collections::HashMap<String, Variant>)> = ids
                    .into_iter()
                    .map(|id| (id, self.entry_properties(id)))
                    .collect();
                invocation.return_value(Some(&(rows,).to_variant()));
            }
            "GetProperty" => {
                let value = parameters
                    .get::<(i32, String)>()
                    .and_then(|(id, name)| self.entry_property(id, &name))
                    .unwrap_or_else(|| Variant::from(""));
                invocation.return_value(Some(&(value,).to_variant()));
            }
            "Event" => {
                if let Some((id, event, _, _)) = parameters.get::<(i32, String, Variant, u32)>() {
                    self.menu_event(id, &event);
                }
                invocation.return_value(None);
            }
            "EventGroup" => {
                if let Some((events,)) = parameters.get::<(Vec<(i32, String, Variant, u32)>,)>() {
                    for (id, event, _, _) in events {
                        self.menu_event(id, &event);
                    }
                }
                let errors: Vec<i32> = Vec::new();
                invocation.return_value(Some(&(errors,).to_variant()));
            }
            "AboutToShow" => invocation.return_value(Some(&(false,).to_variant())),
            "AboutToShowGroup" => {
                let empty: Vec<i32> = Vec::new();
                invocation.return_value(Some(&(empty.clone(), empty).to_variant()));
            }
            _ => invocation.return_dbus_error(
                "org.quicklook.Error.Unknown",
                &format!("未知菜单方法 {method}"),
            ),
        }
    }
    fn menu_event(&self, id: i32, event: &str) {
        if event != "clicked" {
            return;
        }
        match id {
            ID_SETTINGS => (self.actions.settings)(),
            ID_RESTART => (self.actions.restart)(),
            ID_ABOUT => (self.actions.about)(),
            ID_QUIT => (self.actions.quit)(),
            _ => {}
        }
    }
    /// `(ia{sv}av)`: id, properties, children.
    fn layout() -> Variant {
        // 每个节点必须是三段：少了子节点数组这一段，gdbus 打印出来「看着正常」，
        // 但宿主按签名解析时整份布局会被判为无效，右键菜单就出不来。
        //
        // 而且必须用 tuple_from_iter 直接拼节点：`Variant` 的 ToVariant 会再包
        // 一层 `v`，整个元组就成了 `(ia{sv}v)`。introspection 仍写着 `(ia{sv}av)`、
        // gdbus 也照常打印，但 sdbus-c++ 这类宿主解码子节点时签名对不上，异常被
        // 静默吞掉，最终只显示一个空菜单（noctalia 实测如此）。
        let children = [
            Self::node(
                ID_SETTINGS,
                &[
                    ("label", "设置…".to_variant()),
                    ("type", "standard".to_variant()),
                ],
            ),
            Self::node(
                ID_ABOUT,
                &[
                    ("label", "关于 Quick Look".to_variant()),
                    ("type", "standard".to_variant()),
                ],
            ),
            Self::node(ID_SEPARATOR, &[("type", "separator".to_variant())]),
            Self::node(
                ID_RESTART,
                &[
                    ("label", "重启".to_variant()),
                    ("type", "standard".to_variant()),
                ],
            ),
            Self::node(
                ID_QUIT,
                &[
                    ("label", "退出".to_variant()),
                    ("type", "standard".to_variant()),
                ],
            ),
        ];
        Variant::tuple_from_iter([
            0i32.to_variant(),
            Self::properties(&[("children-display", "submenu".to_variant())]).to_variant(),
            // av 的元素类型是 v，每个子节点都真的包一层 variant。
            Variant::array_from_iter_with_type(
                glib::VariantTy::VARIANT,
                children.iter().map(Variant::from_variant),
            ),
        ])
    }

    fn properties(entries: &[(&str, Variant)]) -> std::collections::HashMap<String, Variant> {
        let mut properties = std::collections::HashMap::new();
        properties.insert("enabled".to_string(), true.to_variant());
        properties.insert("visible".to_string(), true.to_variant());
        for (name, value) in entries {
            properties.insert((*name).to_string(), value.clone());
        }
        properties
    }

    /// 一个 dbusmenu 节点：`(i32, a{sv}, av)`，第三个字段是自己的子节点（这里都为空）。
    fn node(id: i32, entries: &[(&str, Variant)]) -> Variant {
        Variant::tuple_from_iter([
            id.to_variant(),
            Self::properties(entries).to_variant(),
            Variant::array_from_iter_with_type(
                glib::VariantTy::VARIANT,
                std::iter::empty::<Variant>(),
            ),
        ])
    }

    fn entry_properties(&self, id: i32) -> std::collections::HashMap<String, Variant> {
        let mut properties = std::collections::HashMap::new();
        for name in ["label", "type", "enabled", "visible"] {
            if let Some(value) = self.entry_property(id, name) {
                properties.insert(name.to_string(), value);
            }
        }
        properties
    }
    fn entry_property(&self, id: i32, name: &str) -> Option<Variant> {
        let (label, kind) = match id {
            ID_SETTINGS => ("设置…", "standard"),
            ID_RESTART => ("重启", "standard"),
            ID_ABOUT => ("关于 Quick Look", "standard"),
            ID_SEPARATOR => ("", "separator"),
            ID_QUIT => ("退出", "standard"),
            _ => return None,
        };
        Some(match name {
            "label" => label.to_variant(),
            "type" => kind.to_variant(),
            "enabled" | "visible" => true.to_variant(),
            _ => return None,
        })
    }
    /// The item is useless without a host to display it, so re-register when the
    /// watcher appears (session restart, shell reload) instead of only at start.
    fn watch(self: &Rc<Self>) -> Result<()> {
        let tray = self.clone();
        #[allow(deprecated)]
        self.connection.signal_subscribe(
            Some("org.freedesktop.DBus"),
            Some("org.freedesktop.DBus"),
            Some("NameOwnerChanged"),
            Some("/org/freedesktop/DBus"),
            Some(WATCHER_BUS),
            gio::DBusSignalFlags::NONE,
            move |_: &gio::DBusConnection,
                  _: &str,
                  _: &str,
                  _: &str,
                  _: &str,
                  parameters: &Variant| {
                let new_owner = parameters
                    .get::<(String, String, String)>()
                    .map(|(_, new, _)| new)
                    .unwrap_or_default();
                if new_owner.is_empty() {
                    tray.registered.set(false);
                } else {
                    tray.register_with_watcher();
                }
            },
        );
        Ok(())
    }
    fn register_with_watcher(self: &Rc<Self>) {
        let bus = self.bus.clone();
        let tray = self.clone();
        self.connection.call(
            Some(WATCHER_BUS),
            WATCHER_PATH,
            WATCHER_BUS,
            "RegisterStatusNotifierItem",
            Some(&(bus.clone(),).to_variant()),
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            2000,
            None::<&gio::Cancellable>,
            move |result| {
                match result {
                    Ok(_) => tray.registered.set(true),
                    Err(error) => {
                        tray.registered.set(false);
                        // No tray host is a normal desktop, not a failure to report.
                        if !error.matches(gio::IOErrorEnum::DbusError) {
                            eprintln!("quicklookd: 托盘注册失败：{error}");
                        }
                    }
                }
            },
        );
    }
}
fn own_name(connection: &gio::DBusConnection, name: &str) -> Result<bool> {
    let result = connection.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "RequestName",
        Some(&(name, 4u32).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        3000,
        None::<&gio::Cancellable>,
    )?;
    Ok(result
        .get::<(u32,)>()
        .map(|(code,)| code == 1 || code == 4)
        .unwrap_or(false))
}
impl Drop for Tray {
    fn drop(&mut self) {
        for registration in self.registrations.borrow_mut().drain(..) {
            let _ = self.connection.unregister_object(registration);
        }
    }
}
/// Where the icon files are expected to live, for installers and diagnostics.
pub fn icon_directory() -> std::path::PathBuf {
    quicklook_core::config::xdg_data_home().join("icons/hicolor")
}

#[cfg(test)]
mod tests {

    #[test]
    fn object_path_parses() {
        let value = glib::Variant::parse(None, "@o '/MenuBar'");
        assert!(value.is_ok(), "{:?}", value.err());
        let value = value.unwrap();
        assert_eq!(value.type_().as_str(), "o");
        assert_eq!(value.get::<String>().as_deref(), Some("/MenuBar"));
    }

    #[test]
    fn layout_children_are_dbusmenu_nodes() {
        let layout = super::Tray::layout();
        assert_eq!(layout.type_().as_str(), "(ia{sv}av)");
        let children = layout.child_value(2);
        assert_eq!(children.type_().as_str(), "av");
        assert_eq!(children.n_children(), 5);
        let mut labels = Vec::new();
        for index in 0..children.n_children() {
            let node = children
                .child_value(index)
                .as_variant()
                .expect("子节点必须包在 variant 里");
            // 曾经用 ToVariant 拼元组，第三个字段被多包一层，这里会变成 (ia{sv}v)。
            assert_eq!(node.type_().as_str(), "(ia{sv}av)");
            let properties = node
                .child_value(1)
                .get::<std::collections::HashMap<String, glib::Variant>>()
                .expect("节点属性必须是 a{sv}");
            if let Some(label) = properties.get("label").and_then(|v| v.get::<String>()) {
                labels.push(label);
            }
        }
        assert_eq!(labels, ["设置…", "关于 Quick Look", "重启", "退出"]);
    }
}
