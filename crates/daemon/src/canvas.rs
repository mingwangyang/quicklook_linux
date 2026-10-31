use crate::backend::{PdfData, Pixels};
use gtk4::{gdk, glib, graphene, prelude::*, subclass::prelude::*};
use std::cell::{Cell, RefCell};

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct Canvas {
        pub texture: RefCell<Option<gdk::Texture>>,
        pub zoom: Cell<f64>,
        pub actual: Cell<bool>,
        pub pan: Cell<(f64, f64)>,
        pub rotation: Cell<i32>,
        pub text: RefCell<String>,
        pub boxes: RefCell<Vec<[f64; 4]>>,
        pub selection: Cell<Option<[f64; 4]>>,
        pub scroll_zoom: Cell<bool>,
        pub source_size: Cell<Option<[u32; 2]>>,
        pub detail: RefCell<Option<(gdk::Texture, [u32; 4])>>,
        pub requested: Cell<Option<[u32; 4]>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Canvas {
        const NAME: &'static str = "QuicklookCanvas";
        type Type = super::Canvas;
        type ParentType = gtk4::Widget;
    }
    impl ObjectImpl for Canvas {}
    impl WidgetImpl for Canvas {
        fn measure(&self, orientation: gtk4::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            (
                120,
                if orientation == gtk4::Orientation::Horizontal {
                    720
                } else {
                    480
                },
                -1,
                -1,
            )
        }
        fn snapshot(&self, snapshot: &gtk4::Snapshot) {
            let obj = self.obj();
            let width = obj.width() as f32;
            let height = obj.height() as f32;
            snapshot.push_clip(&graphene::Rect::new(0., 0., width, height));
            snapshot.append_color(
                &gdk::RGBA::new(0.055, 0.063, 0.078, 1.),
                &graphene::Rect::new(0., 0., width, height),
            );
            if let Some(texture) = self.texture.borrow().as_ref() {
                let (w, h) = obj.image_size();
                let (w, h) = (w as f32, h as f32);
                let scale = obj.scale();
                let (px, py) = self.pan.get();
                snapshot.save();
                snapshot.translate(&graphene::Point::new(
                    width / 2. + px as f32,
                    height / 2. + py as f32,
                ));
                snapshot.rotate((self.rotation.get() * 90) as f32);
                snapshot.scale(scale as f32, scale as f32);
                snapshot.translate(&graphene::Point::new(-w / 2., -h / 2.));
                snapshot.append_texture(texture, &graphene::Rect::new(0., 0., w, h));
                if let Some((detail, r)) = self.detail.borrow().as_ref() {
                    snapshot.append_texture(
                        detail,
                        &graphene::Rect::new(r[0] as f32, r[1] as f32, r[2] as f32, r[3] as f32),
                    );
                }
                if let Some(rect) = self.selection.get() {
                    for b in self.boxes.borrow().iter() {
                        if intersects(*b, rect) {
                            snapshot.append_color(
                                &gdk::RGBA::new(0.2, 0.5, 1., 0.35),
                                &graphene::Rect::new(
                                    b[0] as f32,
                                    b[1] as f32,
                                    (b[2] - b[0]).max(1.) as f32,
                                    (b[3] - b[1]).max(1.) as f32,
                                ),
                            );
                        }
                    }
                }
                snapshot.restore();
            }
            snapshot.pop();
        }
    }
}
glib::wrapper! {pub struct Canvas(ObjectSubclass<imp::Canvas>) @extends gtk4::Widget,@implements gtk4::Accessible,gtk4::Buildable,gtk4::ConstraintTarget;}
impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}
impl Canvas {
    pub fn new() -> Self {
        let obj: Self = glib::Object::new();
        obj.set_hexpand(true);
        obj.set_vexpand(true);
        obj.set_focusable(true);
        obj.imp().zoom.set(1.);
        obj.imp().scroll_zoom.set(true);
        let scroll = gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::VERTICAL);
        let weak = obj.downgrade();
        scroll.connect_scroll(move |_event, _, dy| {
            if let Some(obj) = weak.upgrade() {
                // 连续滚动模式下画布完全让路：交给外层的「缩放全部页面」控制器，
                // 否则指针落在某一页上时 Ctrl+滚轮只会缩放这一页。
                if !obj.imp().scroll_zoom.get() {
                    return glib::Propagation::Proceed;
                }
                obj.zoom_by((-dy * 0.16).exp());
            }
            glib::Propagation::Stop
        });
        obj.add_controller(scroll);
        let click = gtk4::GestureClick::new();
        let weak = obj.downgrade();
        click.connect_pressed(move |_, n, _, _| {
            if let Some(obj) = weak.upgrade() {
                obj.grab_focus();
                if n == 2 {
                    obj.fit();
                }
            }
        });
        obj.add_controller(click);
        let drag = gtk4::GestureDrag::new();
        let origin = std::rc::Rc::new(Cell::new((0., 0.)));
        let start = std::rc::Rc::new(Cell::new((0., 0.)));
        let selecting = std::rc::Rc::new(Cell::new(false));
        let weak = obj.downgrade();
        let origin2 = origin.clone();
        let start2 = start.clone();
        let selecting2 = selecting.clone();
        drag.connect_drag_begin(move |gesture, x, y| {
            if let Some(obj) = weak.upgrade() {
                obj.grab_focus();
                origin2.set(obj.imp().pan.get());
                start2.set((x, y));
                let select = !obj.imp().text.borrow().is_empty()
                    && !gesture
                        .current_event_state()
                        .contains(gdk::ModifierType::SHIFT_MASK);
                selecting2.set(select);
                if select {
                    let (x, y) = obj.to_image(x, y);
                    obj.imp().selection.set(Some([x, y, x, y]));
                }
            }
        });
        let weak = obj.downgrade();
        drag.connect_drag_update(move |_, dx, dy| {
            if let Some(obj) = weak.upgrade() {
                if selecting.get() {
                    let (x, y) = start.get();
                    let (a, b) = obj.to_image(x, y);
                    let (c, d) = obj.to_image(x + dx, y + dy);
                    obj.imp()
                        .selection
                        .set(Some([a.min(c), b.min(d), a.max(c), b.max(d)]));
                } else {
                    let (x, y) = origin.get();
                    obj.imp().pan.set((x + dx, y + dy));
                }
                obj.queue_draw();
            }
        });
        obj.add_controller(drag);
        // 右键按住拖动直接平移画面（不需要按 Shift），符合「抓手」直觉。
        let pan_drag = gtk4::GestureDrag::new();
        pan_drag.set_button(gdk::BUTTON_SECONDARY);
        let pan_origin = std::rc::Rc::new(Cell::new((0., 0.)));
        let weak = obj.downgrade();
        let pan_begin = pan_origin.clone();
        pan_drag.connect_drag_begin(move |_, _, _| {
            if let Some(obj) = weak.upgrade() {
                obj.grab_focus();
                pan_begin.set(obj.imp().pan.get());
            }
        });
        let weak = obj.downgrade();
        let pan_start = pan_origin.clone();
        pan_drag.connect_drag_update(move |_, dx, dy| {
            if let Some(obj) = weak.upgrade() {
                let (x, y) = pan_start.get();
                obj.imp().pan.set((x + dx, y + dy));
                obj.queue_draw();
            }
        });
        obj.add_controller(pan_drag);
        let pinch = gtk4::GestureZoom::new();
        let base = std::rc::Rc::new(Cell::new(1.));
        let weak = obj.downgrade();
        let base2 = base.clone();
        pinch.connect_begin(move |_, _| {
            if let Some(obj) = weak.upgrade() {
                base2.set(obj.imp().zoom.get());
            }
        });
        let weak = obj.downgrade();
        pinch.connect_scale_changed(move |_, scale| {
            if let Some(obj) = weak.upgrade() {
                obj.imp().zoom.set((base.get() * scale).clamp(0.05, 64.));
                obj.queue_draw();
            }
        });
        obj.add_controller(pinch);
        let key = gtk4::EventControllerKey::new();
        let weak = obj.downgrade();
        key.connect_key_pressed(move |_, key, _, state| {
            if key == gdk::Key::c
                && state.contains(gdk::ModifierType::CONTROL_MASK)
                && let Some(obj) = weak.upgrade()
            {
                let text = obj.selected_text();
                if !text.is_empty() {
                    obj.clipboard().set_text(&text);
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        });
        obj.add_controller(key);
        obj
    }
    pub fn set_pixels(&self, pixels: &Pixels) {
        let bytes = glib::Bytes::from_owned(pixels.bytes.clone());
        let texture = gdk::MemoryTexture::new(
            pixels.width as i32,
            pixels.height as i32,
            if pixels.bgra {
                gdk::MemoryFormat::B8g8r8a8Premultiplied
            } else {
                gdk::MemoryFormat::R8g8b8a8
            },
            &bytes,
            pixels.width as usize * 4,
        );
        self.imp().texture.replace(Some(texture.upcast()));
        self.queue_draw();
    }
    pub fn set_pdf(&self, pdf: &PdfData) {
        self.set_pixels(&pdf.pixels);
        self.imp().text.replace(pdf.text.clone());
        self.imp().boxes.replace(pdf.boxes.clone());
        self.imp().selection.set(None);
    }
    pub fn fit(&self) {
        self.imp().actual.set(false);
        self.imp().detail.replace(None);
        self.imp().requested.set(None);
        self.imp().zoom.set(1.);
        self.imp().pan.set((0., 0.));
        self.queue_draw();
    }
    pub fn set_scroll_zoom(&self, enabled: bool) {
        self.imp().scroll_zoom.set(enabled);
    }
    pub fn actual_size(&self) {
        self.imp().actual.set(true);
        self.imp().zoom.set(1.);
        self.imp().pan.set((0., 0.));
        self.queue_draw();
    }
    pub fn rotate(&self, clockwise: bool) {
        self.imp()
            .rotation
            .set((self.imp().rotation.get() + if clockwise { 1 } else { 3 }) % 4);
        self.fit();
    }
    /// 直接设定缩放倍数（供 PDF 连续模式让所有页共享同一缩放）。
    pub fn set_zoom(&self, zoom: f64) {
        self.imp().zoom.set(zoom.clamp(0.05, 64.));
        self.queue_draw();
    }
    /// 当前缩放倍数。
    pub fn zoom(&self) -> f64 {
        self.imp().zoom.get()
    }
    pub fn zoom_by(&self, factor: f64) {
        self.imp()
            .zoom
            .set((self.imp().zoom.get() * factor).clamp(0.05, 64.));
        self.queue_draw();
    }
    fn fit_scale(&self) -> f64 {
        self.imp()
            .texture
            .borrow()
            .as_ref()
            .map(|t| {
                let _ = t;
                let (mut w, mut h) = self.image_size();
                if self.imp().rotation.get() % 2 != 0 {
                    std::mem::swap(&mut w, &mut h);
                }
                ((self.width() as f64 - 32.).max(1.) / w)
                    .min((self.height() as f64 - 32.).max(1.) / h)
                    .min(1.)
            })
            .unwrap_or(1.)
    }
    fn scale(&self) -> f64 {
        (if self.imp().actual.get() {
            1.
        } else {
            self.fit_scale()
        }) * self.imp().zoom.get()
    }
    fn to_image(&self, x: f64, y: f64) -> (f64, f64) {
        let (px, py) = self.imp().pan.get();
        let x = (x - self.width() as f64 / 2. - px) / self.scale();
        let y = (y - self.height() as f64 / 2. - py) / self.scale();
        let (x, y) = match self.imp().rotation.get() % 4 {
            1 => (y, -x),
            2 => (-x, -y),
            3 => (-y, x),
            _ => (x, y),
        };
        self.imp()
            .texture
            .borrow()
            .as_ref()
            .map(|_| {
                let (w, h) = self.image_size();
                (x + w / 2., y + h / 2.)
            })
            .unwrap_or((x, y))
    }
    fn image_size(&self) -> (f64, f64) {
        if let Some([w, h]) = self.imp().source_size.get() {
            (w as f64, h as f64)
        } else {
            self.imp()
                .texture
                .borrow()
                .as_ref()
                .map(|t| (t.width() as f64, t.height() as f64))
                .unwrap_or((1., 1.))
        }
    }
    pub fn set_source_size(&self, size: [u32; 2]) {
        self.imp().source_size.set(Some(size));
        self.queue_draw();
    }
    pub fn detail_status(&self) -> serde_json::Value {
        serde_json::json!({"source_size": self.imp().source_size.get(), "viewport": [self.width(), self.height()], "detail": self.imp().detail.borrow().as_ref().map(|(t,r)| serde_json::json!({"rect":r,"pixels":[t.width(),t.height()]}))})
    }
    pub fn detail_request(&self) -> Option<[u32; 4]> {
        let [w, h] = self.imp().source_size.get()?;
        let preview_width = self.imp().texture.borrow().as_ref()?.width() as f64;
        if self.width() <= 0 || self.height() <= 0 || self.scale() <= preview_width / w as f64 * 1.1
        {
            return None;
        }
        let corners = [
            (0., 0.),
            (self.width() as f64, 0.),
            (0., self.height() as f64),
            (self.width() as f64, self.height() as f64),
        ]
        .map(|(x, y)| self.to_image(x, y));
        let x = corners
            .iter()
            .map(|p| p.0)
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(0.)
            .min(w as f64) as u32;
        let y = corners
            .iter()
            .map(|p| p.1)
            .fold(f64::INFINITY, f64::min)
            .floor()
            .max(0.)
            .min(h as f64) as u32;
        let right = corners
            .iter()
            .map(|p| p.0)
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .max(0.)
            .min(w as f64) as u32;
        let bottom = corners
            .iter()
            .map(|p| p.1)
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .max(0.)
            .min(h as f64) as u32;
        (right > x && bottom > y).then_some([
            x,
            y,
            right.saturating_sub(x),
            bottom.saturating_sub(y),
        ])
    }
    pub fn mark_detail_requested(&self, rect: [u32; 4]) {
        self.imp().requested.set(Some(rect));
    }
    pub fn set_detail(&self, pixels: &Pixels, rect: [u32; 4]) {
        if self.imp().requested.get() != Some(rect) {
            return;
        }
        let bytes = glib::Bytes::from_owned(pixels.bytes.clone());
        let texture = gdk::MemoryTexture::new(
            pixels.width as i32,
            pixels.height as i32,
            gdk::MemoryFormat::R8g8b8a8,
            &bytes,
            pixels.width as usize * 4,
        );
        self.imp().detail.replace(Some((texture.upcast(), rect)));
        self.queue_draw();
    }
    pub fn selected_text(&self) -> String {
        let Some(selection) = self.imp().selection.get() else {
            return String::new();
        };
        let boxes = self.imp().boxes.borrow();
        let text = self.imp().text.borrow();
        text.chars()
            .zip(boxes.iter())
            .filter_map(|(c, b)| intersects(*b, selection).then_some(c))
            .collect()
    }
}
fn intersects(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}
