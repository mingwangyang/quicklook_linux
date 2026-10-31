//! 可配置的预览快捷键，并统一映射到各宿主自己的按键写法。
//!
//! 只接受不带 Shift 层的按键（空格、回车、F1–F12），这样同一个配置在
//! GTK、X11 keyval、Yazi 与 Nautilus 加速键里都是确定的一一对应关系；
//! 字母键会因宿主是否展开 Shift 层而出现歧义，因此不支持。
use std::fmt;

/// 设置界面可选的按键名，按显示顺序排列。
pub const KEY_NAMES: &[&str] = &[
    "Space", "Return", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
];
pub const MODIFIER_NAMES: &[&str] = &["Ctrl", "Shift", "Alt"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewKey {
    name: String,
    ctrl: bool,
    shift: bool,
    alt: bool,
}
impl Default for PreviewKey {
    fn default() -> Self {
        Self {
            name: "Space".into(),
            ctrl: false,
            shift: false,
            alt: false,
        }
    }
}
fn canonical(name: &str) -> Option<String> {
    let lower = name.trim().to_ascii_lowercase();
    match lower.as_str() {
        "space" | "空格" | " " => Some("Space".into()),
        "return" | "enter" | "回车" => Some("Return".into()),
        other => {
            let index = other.strip_prefix('f')?.parse::<u8>().ok()?;
            (1..=12).contains(&index).then(|| format!("F{index}"))
        }
    }
}
impl PreviewKey {
    /// 解析 `Ctrl+Shift+Space`、`F3`、`Alt+Return` 等写法。
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('+').map(str::trim).collect::<Vec<_>>();
        let name = canonical(parts.pop()?)?;
        let (mut ctrl, mut shift, mut alt) = (false, false, false);
        for part in parts {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "primary" => ctrl = true,
                "shift" => shift = true,
                "alt" | "mod1" => alt = true,
                _ => return None,
            }
        }
        Some(Self {
            name,
            ctrl,
            shift,
            alt,
        })
    }
    pub fn from_parts(name: &str, ctrl: bool, shift: bool, alt: bool) -> Option<Self> {
        Some(Self {
            name: canonical(name)?,
            ctrl,
            shift,
            alt,
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn ctrl(&self) -> bool {
        self.ctrl
    }
    pub fn shift(&self) -> bool {
        self.shift
    }
    pub fn alt(&self) -> bool {
        self.alt
    }
    pub fn modifier_count(&self) -> usize {
        usize::from(self.ctrl) + usize::from(self.shift) + usize::from(self.alt)
    }
    /// X11（GDK）keyval，供 Nemo 适配在宿主事件里比对。
    pub fn keyval(&self) -> u32 {
        match self.name.as_str() {
            "Space" => 0x20,
            "Return" => 0xff0d,
            other => {
                let index = other[1..].parse::<u32>().unwrap_or(1);
                0xffbe + index.saturating_sub(1)
            }
        }
    }
    /// X11 修饰位：ShiftMask=1、ControlMask=4、Mod1Mask=8。
    pub fn x11_modifiers(&self) -> u32 {
        u32::from(self.shift) | (u32::from(self.ctrl) << 2) | (u32::from(self.alt) << 3)
    }
    /// Yazi 的按键写法，例如 `<Space>`、`<C-Space>`、`<C-S-F3>`。
    pub fn yazi(&self) -> String {
        let mut text = String::from("<");
        if self.ctrl {
            text.push_str("C-");
        }
        if self.shift {
            text.push_str("S-");
        }
        if self.alt {
            text.push_str("A-");
        }
        text.push_str(&self.name);
        text.push('>');
        text
    }
    /// GTK 加速键写法，例如 `<Control>space`。
    pub fn accelerator(&self) -> String {
        let mut text = String::new();
        if self.ctrl {
            text.push_str("<Control>");
        }
        if self.shift {
            text.push_str("<Shift>");
        }
        if self.alt {
            text.push_str("<Alt>");
        }
        text.push_str(match self.name.as_str() {
            "Space" => "space",
            other => other,
        });
        text
    }
    /// Nautilus 脚本加速键只接受 Control 或 Alt；空格本身由宿主原生预览接管。
    pub fn nautilus_bindable(&self) -> bool {
        self.ctrl || self.alt
    }
    /// 中文说明，用于设置界面与文档。
    pub fn describe(&self) -> String {
        let mut text = String::new();
        if self.ctrl {
            text.push_str("Ctrl+");
        }
        if self.shift {
            text.push_str("Shift+");
        }
        if self.alt {
            text.push_str("Alt+");
        }
        text.push_str(match self.name.as_str() {
            "Space" => "空格",
            "Return" => "回车",
            other => other,
        });
        text
    }
}
impl fmt::Display for PreviewKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut text = String::new();
        if self.ctrl {
            text.push_str("Ctrl+");
        }
        if self.shift {
            text.push_str("Shift+");
        }
        if self.alt {
            text.push_str("Alt+");
        }
        text.push_str(&self.name);
        formatter.write_str(&text)
    }
}

/// GDK/X11 里会参与预览键判定的修饰位：ShiftMask=1、LockMask=2、ControlMask=4、
/// Mod1Mask=8、Mod4Mask=64、SuperMask=0x40000000。
/// 忽略 CapsLock/NumLock；Ctrl/Shift/Alt/Super 必须精确匹配。
pub const MODIFIER_MASK: u32 = 1 | 4 | 8 | 64 | 0x4000_0000;

/// `GdkEventType` 里与按键相关的两个值。
pub const KEY_PRESS: i32 = 8;
pub const KEY_RELEASE: i32 = 9;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAction {
    /// 不是预览键，或抬起时本来就没按下：交回宿主。
    Ignore,
    /// 预览键的抬起事件或自动重复：吞掉，避免宿主再处理。
    Consume,
    /// 一次新的按下：唤出/关闭预览。
    Activate,
}

/// GTK 的 key snooper 对按下和抬起都会调用（见 GTK3 `gtk_main_do_event`），
/// 所以必须按事件类型区分，否则一次空格会 Toggle 两次：窗口刚出现就被关掉。
///
/// `pressed` records key down/up state. No timing window suppresses a second tap.
/// Hosts reset it when their window regains focus, since key-up can go to the viewer.
///
/// `state` 是事件的原始修饰位。
pub fn classify(
    kind: i32,
    keyval: u32,
    state: u32,
    wanted: &PreviewKey,
    pressed: bool,
) -> KeyAction {
    if keyval != wanted.keyval() {
        return KeyAction::Ignore;
    }
    match kind {
        KEY_RELEASE => {
            if pressed {
                KeyAction::Consume
            } else {
                KeyAction::Ignore
            }
        }
        KEY_PRESS => {
            if pressed {
                KeyAction::Consume
            } else if state & MODIFIER_MASK != wanted.x11_modifiers() {
                KeyAction::Ignore
            } else {
                KeyAction::Activate
            }
        }
        _ => KeyAction::Ignore,
    }
}

/// 自动重复判定窗口。
///
/// 正常路径下抬起事件或宿主窗口的焦点变化会立刻清掉锁存，这里只是兜底：
/// 预览窗口抢走焦点后，空格抬起事件会打到预览窗口，宿主再也收不到。没有兜底
/// 时锁存会永远停在「按下」，下一次空格被误当成自动重复吞掉——用户看到的就是
/// 「空格有时候要按两下才关」。
///
/// 400ms 远大于 X/Wayland 的自动重复间隔（约 30ms），又短到丢失抬起的副作用
/// 只影响紧接着的那一下。
pub const STUCK_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(400);

/// 自动重复与「新的一次按下」的判定间隔。
///
/// 宿主在预览窗口抢走焦点之前还会短暂收到两次按键：第一次按下把预览叫出来，紧接着
/// 用户如果很快再按一下（想关掉），这次按下会落在同一个宿主上。此时抬起事件已经
/// 打到预览窗口，宿主手里的锁存还停在「按下」，于是这一次新按键会被误判成自动重复
/// 吞掉——用户看到的就是「空格有时候关不掉，得再按一次」。
///
/// X/Wayland 的自动重复间隔默认约 33ms（即使把 `xset r rate` 调慢也很少超过 100ms），
/// 而人手两次有意的按键之间一般在 150ms 以上，所以用 150ms 区分：
/// 更短视为自动重复（吞掉），更长视为新的一次按下（照常触发 Toggle）。
pub const REPEAT_GAP: std::time::Duration = std::time::Duration::from_millis(150);

/// Per-host pressed state; releases and focus changes reset it immediately.
/// The timestamp only heals a latch whose key-up was never delivered.
#[derive(Default)]
pub struct HoldLatch(std::cell::Cell<Option<std::time::Instant>>);
impl HoldLatch {
    pub const fn new() -> Self {
        Self(std::cell::Cell::new(None))
    }
    pub fn pressed(&self) -> bool {
        self.0.get().is_some_and(|at| at.elapsed() < STUCK_TIMEOUT)
    }
    /// 距上一次按下不足 [`REPEAT_GAP`]：几乎可以肯定是自动重复，而不是新的按键。
    pub fn rapid(&self) -> bool {
        self.0.get().is_some_and(|at| at.elapsed() < REPEAT_GAP)
    }
    pub fn press(&self) {
        self.0.set(Some(std::time::Instant::now()));
    }
    pub fn release(&self) {
        self.0.set(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_plain_space() {
        let key = PreviewKey::default();
        assert_eq!(key.to_string(), "Space");
        assert_eq!(key.yazi(), "<Space>");
        assert_eq!(key.accelerator(), "space");
        assert_eq!(key.keyval(), 0x20);
        assert_eq!(key.x11_modifiers(), 0);
    }
    #[test]
    fn parses_modifiers_in_any_order() {
        let key = PreviewKey::parse("shift+ctrl+F3").unwrap();
        assert_eq!(key.to_string(), "Ctrl+Shift+F3");
        assert_eq!(key.yazi(), "<C-S-F3>");
        assert_eq!(key.accelerator(), "<Control><Shift>F3");
        assert_eq!(key.keyval(), 0xffc0);
        assert_eq!(key.x11_modifiers(), 1 | 4);
    }
    #[test]
    fn rejects_ambiguous_and_unknown_keys() {
        assert!(PreviewKey::parse("k").is_none());
        assert!(PreviewKey::parse("F13").is_none());
        assert!(PreviewKey::parse("Space+").is_none());
        assert!(PreviewKey::parse("Hyper+Space").is_none());
        assert!(PreviewKey::parse("").is_none());
    }
    #[test]
    fn normalizes_aliases() {
        assert_eq!(
            PreviewKey::parse("primary+enter").unwrap().to_string(),
            "Ctrl+Return"
        );
        assert_eq!(PreviewKey::parse(" 空格 ").unwrap().to_string(), "Space");
        assert_eq!(
            PreviewKey::parse("alt+空格").unwrap().to_string(),
            "Alt+Space"
        );
    }
    #[test]
    fn nautilus_requires_control_or_alt() {
        assert!(!PreviewKey::default().nautilus_bindable());
        assert!(
            !PreviewKey::parse("Shift+Space")
                .unwrap()
                .nautilus_bindable()
        );
        assert!(!PreviewKey::parse("F3").unwrap().nautilus_bindable());
        assert!(PreviewKey::parse("Ctrl+Space").unwrap().nautilus_bindable());
        assert!(PreviewKey::parse("Alt+F3").unwrap().nautilus_bindable());
    }
    #[test]
    fn round_trips_through_text() {
        for name in KEY_NAMES {
            let key = PreviewKey::from_parts(name, true, true, false).unwrap();
            assert_eq!(PreviewKey::parse(&key.to_string()).as_ref(), Some(&key));
        }
    }

    /// 回归：GTK 对按下和抬起都会调用 snooper，一次空格只能触发一次预览。
    #[test]
    fn one_space_tap_activates_once() {
        let key = PreviewKey::default();
        assert_eq!(
            classify(KEY_PRESS, 0x20, 0, &key, false),
            KeyAction::Activate
        );
        assert_eq!(
            classify(KEY_RELEASE, 0x20, 0, &key, true),
            KeyAction::Consume
        );
        assert_eq!(
            classify(KEY_RELEASE, 0x20, 0, &key, false),
            KeyAction::Ignore
        );
    }

    #[test]
    fn held_space_auto_repeat_is_consumed() {
        let key = PreviewKey::default();
        assert_eq!(classify(KEY_PRESS, 0x20, 0, &key, true), KeyAction::Consume);
    }

    #[test]
    fn consecutive_taps_have_no_minimum_interval() {
        let latch = HoldLatch::new();
        let key = PreviewKey::default();
        for _ in 0..20 {
            assert_eq!(
                classify(KEY_PRESS, 0x20, 0, &key, latch.pressed()),
                KeyAction::Activate
            );
            latch.press();
            assert_eq!(
                classify(KEY_PRESS, 0x20, 0, &key, latch.pressed()),
                KeyAction::Consume
            );
            latch.release();
        }
    }

    /// 抬起事件丢失时锁存必须自己松开，否则下一次空格会被当成自动重复吞掉。
    #[test]
    fn rapid_distinguishes_auto_repeat_from_a_new_tap() {
        let latch = HoldLatch::new();
        assert!(!latch.pressed());
        assert!(!latch.rapid(), "没有按下过时不该是重复");
        latch.press();
        // 刚按下：既算「按下」也算「疑似重复」。
        assert!(latch.pressed());
        assert!(latch.rapid());
        std::thread::sleep(REPEAT_GAP + std::time::Duration::from_millis(20));
        // 超过重复间隔后：仍算按下（抬起事件可能晚到），但不再算重复。
        assert!(latch.pressed());
        assert!(!latch.rapid(), "间隔超过 REPEAT_GAP 应视为新的一次按下");
        latch.release();
        assert!(!latch.pressed());
        assert!(!latch.rapid());
    }

    #[test]
    fn latch_self_heals_when_the_key_up_never_arrives() {
        let latch = HoldLatch::new();
        let key = PreviewKey::default();
        latch.press();
        assert_eq!(
            classify(KEY_PRESS, 0x20, 0, &key, latch.pressed()),
            KeyAction::Consume
        );
        std::thread::sleep(STUCK_TIMEOUT + std::time::Duration::from_millis(60));
        assert!(!latch.pressed(), "锁存应在超时后自动松开");
        assert_eq!(
            classify(KEY_PRESS, 0x20, 0, &key, latch.pressed()),
            KeyAction::Activate
        );
    }

    #[test]
    fn press_with_extra_modifier_is_ignored() {
        let key = PreviewKey::default();
        assert_eq!(classify(KEY_PRESS, 0x20, 4, &key, false), KeyAction::Ignore);
    }

    #[test]
    fn modified_preview_key_requires_its_modifier() {
        let key = PreviewKey::parse("Ctrl+Space").expect("ctrl space");
        assert_eq!(
            classify(KEY_PRESS, 0x20, 4, &key, false),
            KeyAction::Activate
        );
        assert_eq!(classify(KEY_PRESS, 0x20, 0, &key, false), KeyAction::Ignore);
    }

    #[test]
    fn other_keys_are_ignored_on_press_and_release() {
        let key = PreviewKey::default();
        assert_eq!(classify(KEY_PRESS, 0x41, 0, &key, false), KeyAction::Ignore);
        assert_eq!(
            classify(KEY_RELEASE, 0x41, 0, &key, false),
            KeyAction::Ignore
        );
    }

    #[test]
    fn plain_space_needs_no_modifier() {
        let key = PreviewKey::parse("Space").expect("space");
        assert_eq!(key.keyval(), 0x20);
        assert_eq!(key.x11_modifiers() & MODIFIER_MASK, 0);
    }

    #[test]
    fn control_space_requires_control() {
        let key = PreviewKey::parse("Ctrl+Space").expect("ctrl space");
        assert_eq!(key.x11_modifiers() & MODIFIER_MASK, 4);
    }

    #[test]
    fn shift_and_alt_land_on_gdk_bits() {
        let key = PreviewKey::parse("Ctrl+Shift+Alt+Space").expect("all");
        assert_eq!(key.x11_modifiers() & MODIFIER_MASK, 1 | 4 | 8);
    }

    #[test]
    fn modifier_mask_ignores_lock_bits() {
        // CapsLock(2) 与 NumLock(128) 不该影响判定。
        assert_eq!(2 & MODIFIER_MASK, 0);
        assert_eq!(128 & MODIFIER_MASK, 0);
    }
}
