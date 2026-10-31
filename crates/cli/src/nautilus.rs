//! Optional Nautilus protocol adapter; the preview core does not depend on it.
use anyhow::{Context, Result, bail, ensure};
use glib::variant::ToVariant;
use quicklook_core::{
    files::FileInfo,
    protocol::{BUS, PATH},
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    sync::mpsc,
    time::{Duration, Instant},
};

const NAME: &str = "org.gnome.NautilusPreviewer";
const OBJECT: &str = "/org/gnome/NautilusPreviewer";
const IFACE: &str = "org.gnome.NautilusPreviewer2";
const XML: &str = r#"<node><interface name="org.gnome.NautilusPreviewer2">
<method name="ShowFile"><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="b" direction="in"/></method>
<method name="Close"/>
<property name="Visible" type="b" access="read"/>
<property name="ParentHandle" type="s" access="read"/>
<signal name="SelectionEvent"><arg type="u"/></signal>
</interface></node>"#;

struct Adapter {
    bus: gio::DBusConnection,
    core: gio::DBusConnection,
    visible: Cell<bool>,
    opened: Cell<bool>,
    parent: RefCell<String>,
    activity: Cell<Instant>,
}
impl Adapter {
    fn call(
        &self,
        method: &str,
        parameters: Option<&glib::Variant>,
        passive: bool,
    ) -> Result<glib::Variant> {
        Ok(self.core.call_sync(
            Some(BUS),
            PATH,
            BUS,
            method,
            parameters,
            None,
            if passive {
                gio::DBusCallFlags::NO_AUTO_START
            } else {
                gio::DBusCallFlags::NONE
            },
            5000,
            None::<&gio::Cancellable>,
        )?)
    }
    fn status(&self) -> Option<serde_json::Value> {
        let value = self.call("Status", None, true).ok()?;
        let (json,) = value.get::<(String,)>()?;
        serde_json::from_str(&json).ok()
    }
    fn refresh(&self) {
        let visible = self
            .status()
            .and_then(|v| v["visible"].as_bool())
            .unwrap_or(false);
        if visible != self.visible.replace(visible) {
            self.changed();
        }
    }
    fn changed(&self) {
        let mut fields = BTreeMap::new();
        fields.insert("Visible".to_string(), self.visible.get().to_variant());
        fields.insert(
            "ParentHandle".to_string(),
            self.parent.borrow().to_variant(),
        );
        let _ = self.bus.emit_signal(
            None,
            OBJECT,
            "org.freedesktop.DBus.Properties",
            "PropertiesChanged",
            Some(&(IFACE, fields, Vec::<String>::new()).to_variant()),
        );
    }
    fn show(&self, parameters: &glib::Variant) -> Result<()> {
        // Older Nautilus uses three arguments; newer versions add an activation
        // token. The token is deliberately not used for a non-activating preview.
        let (uri, parent, toggle) =
            if let Some((uri, parent, toggle)) = parameters.get::<(String, String, bool)>() {
                (uri, parent, toggle)
            } else if let Some((uri, parent, toggle, _)) =
                parameters.get::<(String, String, bool, String)>()
            {
                (uri, parent, toggle)
            } else {
                bail!("不支持的 Nautilus ShowFile 参数")
            };
        ensure!(
            uri.len() <= 16 * 1024 && parent.len() <= 16 * 1024,
            "Nautilus 请求过长"
        );
        let info = FileInfo::from_arg(&uri)?;
        self.activity.set(Instant::now());
        self.parent.replace(parent);
        let visible = self
            .status()
            .and_then(|v| v["visible"].as_bool())
            .unwrap_or(false);
        let method = if toggle {
            "Toggle"
        } else if visible || self.opened.get() {
            "SetSelection"
        } else {
            "Show"
        };
        let result = self.call(
            method,
            Some(&(vec![info.uri],).to_variant()),
            method == "SetSelection",
        );
        if let Err(error) = result
            && !(method == "SetSelection" && self.status().is_none())
        {
            return Err(error);
        }
        self.refresh();
        if self.visible.get() {
            self.opened.set(true);
        }
        self.changed();
        Ok(())
    }
}
fn main() {
    if let Err(error) = run() {
        eprintln!("quicklook-nautilus-bridge: {error:#}");
        std::process::exit(1);
    }
}
fn run() -> Result<()> {
    let bus = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)?;
    let core = if let Ok(address) = std::env::var("QUICKLOOK_DBUS_ADDRESS") {
        gio::DBusConnection::for_address_sync(
            &address,
            gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
                | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
            None::<&gio::DBusAuthObserver>,
            None::<&gio::Cancellable>,
        )?
    } else {
        bus.clone()
    };
    let state = Rc::new(Adapter {
        bus: bus.clone(),
        core,
        visible: Cell::new(false),
        opened: Cell::new(false),
        parent: RefCell::new(String::new()),
        activity: Cell::new(Instant::now()),
    });
    // GDBus object registration accepts one signature per method. A message
    // filter handles both published ShowFile signatures before that validation.
    // Only serialized bytes cross the GDBus I/O thread boundary.
    let (sender, receiver) = mpsc::sync_channel(64);
    let _filter = bus.add_filter(move |connection, message, incoming| {
        if incoming
            && message.message_type() == gio::DBusMessageType::MethodCall
            && message.path().as_deref() == Some(OBJECT)
            && message.interface().as_deref() == Some(IFACE)
            && message.member().as_deref() == Some("ShowFile")
        {
            let queued = message
                .to_blob(gio::DBusCapabilityFlags::NONE)
                .ok()
                .map(|bytes| bytes.len() <= 64 * 1024 && sender.try_send(bytes).is_ok())
                .unwrap_or(false);
            if !queued {
                let reply = message.new_method_error_literal(
                    "org.quicklook.Error.InvalidRequest",
                    "请求过长或队列已满",
                );
                let _ = connection.send_message(&reply, gio::DBusSendMessageFlags::NONE);
            }
            None
        } else {
            Some(message.clone())
        }
    });
    let iface = gio::DBusNodeInfo::for_xml(XML)?
        .lookup_interface(IFACE)
        .context("无效 Nautilus 协议")?;
    let properties = state.clone();
    let methods = state.clone();
    let _object = bus
        .register_object(OBJECT, &iface)
        .property(move |_, _, _, _, property| match property {
            "Visible" => properties.visible.get().to_variant(),
            "ParentHandle" => properties.parent.borrow().to_variant(),
            _ => false.to_variant(),
        })
        .method_call(move |_, _, _, _, method, _, invocation| {
            if method == "Close" {
                methods.activity.set(Instant::now());
                let _ = methods.call("Close", None, true);
                methods.refresh();
                invocation.return_value(None);
            } else {
                invocation.return_dbus_error("org.quicklook.Error.InvalidRequest", "无效方法");
            }
        })
        .build()?;
    let name = bus.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "RequestName",
        Some(&(NAME, 4u32).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        3000,
        None::<&gio::Cancellable>,
    )?;
    let (code,) = name.get::<(u32,)>().context("无法申请 Nautilus 总线")?;
    ensure!(
        code == 1 || code == 4,
        "另一个预览服务正在使用 Nautilus 总线，未替换该服务"
    );
    let mainloop = glib::MainLoop::new(None, false);
    let adapter = state.clone();
    glib::timeout_add_local(Duration::from_millis(4), move || {
        for _ in 0..16 {
            let Ok(bytes) = receiver.try_recv() else {
                break;
            };
            if let Ok(message) = gio::DBusMessage::from_blob(&bytes, gio::DBusCapabilityFlags::NONE)
            {
                let result = message
                    .body()
                    .context("缺少参数")
                    .and_then(|p| adapter.show(&p));
                let reply = match result {
                    Ok(()) => message.new_method_reply(),
                    Err(e) => message.new_method_error_literal(
                        "org.quicklook.Error.InvalidRequest",
                        &format!("{e:#}"),
                    ),
                };
                let _ = adapter
                    .bus
                    .send_message(&reply, gio::DBusSendMessageFlags::NONE);
            }
        }
        glib::ControlFlow::Continue
    });
    let loop_ = mainloop.clone();
    glib::timeout_add_local(Duration::from_millis(100), move || {
        state.refresh();
        if !state.visible.get() && state.activity.get().elapsed() > Duration::from_secs(60) {
            loop_.quit();
            return glib::ControlFlow::Break;
        }
        glib::ControlFlow::Continue
    });
    mainloop.run();
    Ok(())
}
