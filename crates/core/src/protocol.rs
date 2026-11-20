use glib::variant::ToVariant;
pub const BUS: &str = "org.quicklook.Preview";
pub const PATH: &str = "/org/quicklook/Preview";
pub const APP_ID: &str = "org.quicklook.Preview";
/// 分离窗口的 app-id：独立进程，不匹配常驻窗口那条浮动/不抢焦点的 niri 规则。
pub const DETACHED_APP_ID: &str = "org.quicklook.Preview.Detached";
pub const NEMO_BUS: &str = "org.nemo.Preview";
pub const NEMO_PATH: &str = "/org/nemo/Preview";
pub const VIEWER_BUS: &str = "org.quicklook.Viewer";
pub const VIEWER_PATH: &str = "/org/quicklook/Viewer";
pub const XML: &str = r#"<node><interface name="org.quicklook.Preview">
<method name="Preview"><arg name="uris" type="as" direction="in"/></method>
<method name="Show"><arg name="uris" type="as" direction="in"/></method>
<method name="Toggle"><arg name="uris" type="as" direction="in"/></method>
<method name="SetSelection"><arg name="uris" type="as" direction="in"/></method>
<method name="Pick"/>
<method name="ReloadConfig"><arg name="nemo_enabled" type="b" direction="in"/></method>
<method name="ApplyConfig"/>
<method name="Next"/><method name="Prev"/><method name="Close"/>
<method name="Back"/>
<method name="Pin"><arg name="pinned" type="b" direction="in"/></method>
<method name="Status"><arg name="json" type="s" direction="out"/></method>
<method name="Screenshot"><arg name="path" type="s" direction="in"/></method>
<method name="Page"><arg name="page" type="i" direction="in"/></method>
<method name="Extract"><arg name="index" type="u" direction="in"/></method>
<method name="Action"><arg name="action" type="s" direction="in"/></method>
<method name="Quit"/>
</interface></node>"#;
pub const NEMO_XML: &str = r#"<node><interface name="org.nemo.Preview">
<method name="ShowFile"><arg type="s" direction="in"/><arg type="i" direction="in"/><arg type="b" direction="in"/></method>
<method name="Close"/>
</interface></node>"#;

/// 读取常驻 broker 里的预览可见状态。宿主适配器在按键旁路器里用它判断
/// 「这次空格是打开还是关闭」——焦点不在文件视图时也能可靠关闭。
pub fn preview_visible() -> bool {
    let Ok(connection) = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>) else {
        return false;
    };
    let Ok(reply) = connection.call_sync(
        Some(BUS),
        PATH,
        BUS,
        "Status",
        None,
        None,
        gio::DBusCallFlags::NONE,
        500,
        None::<&gio::Cancellable>,
    ) else {
        return false;
    };
    reply
        .get::<(String,)>()
        .and_then(|(json,)| serde_json::from_str::<serde_json::Value>(&json).ok())
        .and_then(|value| value["visible"].as_bool())
        .unwrap_or(false)
}

/// 请求 broker 关闭当前预览。用于宿主已经确认预览可见、但焦点不在文件视图
/// （例如停在路径栏/搜索框）时的空格关闭。
pub fn close_preview() -> bool {
    let Ok(connection) = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>) else {
        return false;
    };
    connection
        .call_sync(
            Some(BUS),
            PATH,
            BUS,
            "Close",
            None,
            None,
            gio::DBusCallFlags::NONE,
            500,
            None::<&gio::Cancellable>,
        )
        .is_ok()
}

/// 让正在运行的预览窗口重新读取配置并应用主题/背景色。
/// 设置窗口修改主题或点「应用」后调用；没有运行中的 viewer 时静默失败，
/// 下一次打开预览窗口会直接读取新配置。
pub fn reload_broker_config(nemo_enabled: bool) -> bool {
    let Ok(connection) = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>) else {
        return false;
    };
    connection
        .call_sync(
            Some(BUS),
            PATH,
            BUS,
            "ReloadConfig",
            Some(&(nemo_enabled,).to_variant()),
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            1000,
            None::<&gio::Cancellable>,
        )
        .is_ok()
}

pub fn apply_viewer_config() -> bool {
    let Ok(connection) = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>) else {
        return false;
    };
    connection
        .call_sync(
            Some(VIEWER_BUS),
            VIEWER_PATH,
            VIEWER_BUS,
            "ApplyConfig",
            None,
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            1000,
            None::<&gio::Cancellable>,
        )
        .is_ok()
}
