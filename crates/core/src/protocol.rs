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
