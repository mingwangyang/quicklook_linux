use super::*;
use crate::backend::{ImageData, PdfData};
use sourceview5::prelude::*;
use webkit6::prelude::*;

impl Controller {
    pub(super) fn render(self: &Rc<Self>, prepared: Arc<Prepared>) {
        clear(&self.content);
        clear(&self.format_bar);
        {
            let mut state = self.state.borrow_mut();
            state.active = Active::default();
            state.prepared = Some(prepared.clone());
        }
        match &prepared.body {
            Body::Image(image) => self.image_view(image),
            Body::Tile { .. } => {}
            Body::Pdf(pdf) => {
                self.pdf_view(&prepared.info, pdf);
                // PDF 默认用连续滚动；用户可在格式栏切回单页。
                if self.config.pdf_continuous && !self.state.borrow().pdf_continuous {
                    self.toggle_pdf_continuous();
                }
            }
            Body::Text(text) => {
                let view = self.text_view(&prepared.info, text);
                self.content.append(&scroll(&view));
                self.text_controls(&prepared.info, text, &view);
            }
            Body::Markdown {
                source,
                html,
                diagrams,
            } => {
                let source_view = self.text_view(&prepared.info, source);
                let rendered = self.web_view_with(html, &prepared.info, *diagrams);
                let stack = gtk4::Stack::new();
                stack.set_vexpand(true);
                stack.add_titled(&rendered, Some("rendered"), "预览");
                stack.add_titled(&scroll(&source_view), Some("source"), "源码");
                let switcher = gtk4::StackSwitcher::new();
                switcher.set_stack(Some(&stack));
                self.format_bar.append(&switcher);
                self.content.append(&stack);
                self.text_controls(&prepared.info, source, &source_view);
            }
            Body::Rtf { source, html } => {
                let source_view = self.text_view(&prepared.info, source);
                let rendered = self.web_view(html, &prepared.info);
                let stack = gtk4::Stack::new();
                stack.set_vexpand(true);
                stack.add_titled(&rendered, Some("rendered"), "预览");
                stack.add_titled(&scroll(&source_view), Some("source"), "源码");
                let switcher = gtk4::StackSwitcher::new();
                switcher.set_stack(Some(&stack));
                self.format_bar.append(&switcher);
                self.content.append(&stack);
                self.text_controls(&prepared.info, source, &source_view);
            }
            Body::Html(html) => self.content.append(&self.web_view(html, &prepared.info)),
            Body::Csv(doc) => {
                // 表格和源码两种模式：表格看数据，源码看原始分隔文本（对齐原始字节）。
                let source_view = self.text_view(&prepared.info, &doc.source);
                let table = self.web_view(&doc.html, &prepared.info);
                let stack = gtk4::Stack::new();
                stack.set_vexpand(true);
                stack.add_titled(&table, Some("table"), "表格");
                stack.add_titled(&scroll(&source_view), Some("source"), "源码");
                let switcher = gtk4::StackSwitcher::new();
                switcher.set_stack(Some(&stack));
                self.format_bar.append(&switcher);
                self.content.append(&stack);
                self.csv_controls(doc);
            }
            Body::Office(doc) => {
                let html = doc.to_html();
                self.content.append(&self.web_view(&html, &prepared.info));
            }
            Body::Structured { source, value } => {
                let root = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
                root.set_margin_start(18);
                root.set_margin_end(18);
                root.set_margin_top(12);
                append_json(&root, "根节点", value.clone(), 0);
                let text = self.text_view(&prepared.info, source);
                let stack = gtk4::Stack::new();
                stack.set_vexpand(true);
                stack.add_titled(&scroll(&root), Some("tree"), "结构");
                stack.add_titled(&scroll(&text), Some("source"), "源码");
                // JSON 通常是想直接读原文；默认落在源码页，需要时再切结构树。
                if json_like(&prepared.info.name) {
                    stack.set_visible_child_name("source");
                }
                let switcher = gtk4::StackSwitcher::new();
                switcher.set_stack(Some(&stack));
                self.format_bar.append(&switcher);
                self.content.append(&stack);
            }
            Body::Directory(directory) => {
                let heading = gtk4::Label::new(Some(&format!(
                    "{} 项 · 本层文件 {}{}",
                    directory.entries.len(),
                    format_size(directory.total_bytes),
                    if directory.truncated {
                        " · 统计未完成"
                    } else {
                        ""
                    }
                )));
                heading.set_margin_top(12);
                heading.set_margin_bottom(12);
                self.content.append(&heading);
                let generation = self.state.borrow().generation;
                self.submit(
                    Task::DirectoryStats(prepared.info.clone()),
                    Target::DirectoryStats {
                        generation,
                        label: heading.downgrade(),
                    },
                    true,
                );
                self.file_list(directory.entries.clone());
            }
            Body::Archive { entries, truncated } => {
                let total = entries
                    .iter()
                    .fold(0u64, |sum, e| sum.saturating_add(e.size));
                let heading = gtk4::Label::new(Some(&format!(
                    "{} 个条目 · 解压大小 {}{} · 双击预览，压缩包可继续展开",
                    entries.len(),
                    format_size(total),
                    if *truncated {
                        " · 列表已截断"
                    } else {
                        ""
                    }
                )));
                heading.set_margin_top(12);
                heading.set_margin_bottom(12);
                self.content.append(&heading);
                let model = gtk4::StringList::new(
                    &entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
                );
                let factory = gtk4::SignalListItemFactory::new();
                factory.connect_setup(|_, item| {
                    let item = item.downcast_ref::<gtk4::ListItem>().unwrap();
                    let label = gtk4::Label::new(None);
                    label.set_xalign(0.);
                    label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
                    item.set_child(Some(&label));
                });
                let entries = Rc::new(entries.clone());
                let entries2 = entries.clone();
                factory.connect_bind(move |_, item| {
                    let item = item.downcast_ref::<gtk4::ListItem>().unwrap();
                    let label = item.child().and_downcast::<gtk4::Label>().unwrap();
                    if let Some(entry) = entries2.get(item.position() as usize) {
                        label.set_text(&format!(
                            "{} {}   ·   {} · {}",
                            if entry.directory {
                                "▸"
                            } else if entry.safe {
                                "·"
                            } else {
                                "⚠"
                            },
                            entry.name,
                            format_size(entry.size),
                            glib::DateTime::from_unix_local(entry.modified)
                                .ok()
                                .and_then(|t| t.format("%Y-%m-%d").ok())
                                .map(|s| s.to_string())
                                .unwrap_or_default()
                        ));
                    }
                });
                let list = gtk4::ListView::new(
                    Some(gtk4::SingleSelection::new(Some(model))),
                    Some(factory),
                );
                let weak = Rc::downgrade(self);
                let source = prepared.info.clone();
                list.connect_activate(move |_, position| {
                    if let (Some(c), Some(entry)) = (weak.upgrade(), entries.get(position as usize))
                    {
                        if !entry.safe {
                            c.status
                                .set_text("该条目为目录、链接或含不安全路径，暂不展开");
                            return;
                        }
                        let previous = c.state.borrow().prepared.clone();
                        c.begin();
                        let generation = c.state.borrow().generation;
                        c.submit(
                            Task::Extract(source.clone(), entry.index),
                            Target::Extract {
                                generation,
                                previous,
                            },
                            false,
                        );
                        c.status.set_text("正在按需解压文件…");
                    }
                });
                self.content.append(&scroll(&list));
            }
            Body::Binary(text) => {
                let buffer = gtk4::TextBuffer::new(None::<&gtk4::TextTagTable>);
                buffer.set_text(text);
                let view = gtk4::TextView::with_buffer(&buffer);
                view.set_monospace(true);
                view.set_editable(false);
                view.set_left_margin(20);
                view.set_top_margin(16);
                self.content.append(&scroll(&view));
                self.status.set_text("二进制文件 · 前 2 KiB 十六进制摘要");
            }
            Body::Summary(text) => self.content.append(&gtk4::Label::new(Some(text))),
            Body::Media => {
                match MediaView::new(&prepared.info.uri, prepared.info.mime.starts_with("audio/")) {
                    Ok(media) => {
                        self.content.append(&media.root);
                        self.state.borrow_mut().active.media = Some(media);
                    }
                    Err(error) => self.error(&error.to_string()),
                }
            }
            Body::Font(font) => self.font_view(&prepared.info, font),
        }
        if self.inspector.reveals_child() {
            self.update_properties();
        }
        let depth = {
            let mut state = self.state.borrow_mut();
            state.ready = !matches!(
                &prepared.body,
                Body::Markdown { .. }
                    | Body::Rtf { .. }
                    | Body::Html(_)
                    | Body::Office(_)
                    | Body::Csv(_)
            );
            state.last_elapsed_ms = state.request_start.elapsed().as_secs_f64() * 1000.;
            state.nested.len()
        };
        self.refresh_titles();
        let mut hint = prepared.note.clone();
        if depth > 0 {
            if !hint.is_empty() {
                hint.push_str(" · ");
            }
            hint.push_str("Backspace / Esc 返回上一层");
        }
        if hint.is_empty() && !matches!(&prepared.body, Body::Binary(_)) {
            hint.push_str("Space 关闭 · ↑↓ 文件 · Enter 打开 · I 属性");
        }
        self.status.set_text(&hint);
        self.format_bar
            .set_visible(self.format_bar.first_child().is_some());
        self.window.queue_resize();
    }
    fn image_view(self: &Rc<Self>, image: &ImageData) {
        let canvas = Canvas::new();
        canvas.set_pixels(&image.frames[0].0);
        if let Some(size) = image.source_size {
            canvas.set_source_size(size);
            let file = self.displayed().expect("包内或本地文件");
            let generation = self.state.borrow().generation;
            let weak = canvas.downgrade();
            let controller = Rc::downgrade(self);
            let mut previous = None;
            let mut submitted = None;
            glib::timeout_add_local(Duration::from_millis(80), move || {
                let (Some(canvas), Some(c)) = (weak.upgrade(), controller.upgrade()) else {
                    return glib::ControlFlow::Break;
                };
                if c.state.borrow().generation != generation {
                    return glib::ControlFlow::Break;
                }
                let request = canvas.detail_request();
                if let Some(rect) = request {
                    if previous == request
                        && submitted != request
                        && !c
                            .state
                            .borrow()
                            .pending
                            .values()
                            .any(|t| matches!(t, Target::Tile { .. }))
                    {
                        canvas.mark_detail_requested(rect);
                        c.submit(
                            Task::Tile(file.clone(), rect),
                            Target::Tile {
                                generation,
                                canvas: canvas.downgrade(),
                            },
                            false,
                        );
                        submitted = request;
                    }
                } else {
                    submitted = None;
                }
                previous = request;
                glib::ControlFlow::Continue
            });
        }
        self.size_window(Some(
            image.frames[0].0.width as f64 / image.frames[0].0.height as f64,
        ));
        self.content.append(&canvas);
        self.state.borrow_mut().active.canvas = Some(canvas.clone());
        let label = gtk4::Label::new(Some(&image.details));
        label.set_hexpand(true);
        label.set_xalign(0.);
        self.format_bar.append(&label);
        self.canvas_controls(&canvas);
        if image.streaming {
            let file = self.displayed().expect("包内或本地文件");
            let cancel = self.state.borrow().cancel.clone();
            let receiver = crate::backend::stream_webp(
                file.path,
                self.config.image_max_edge.min(1600),
                cancel,
            );
            let pause = gtk4::ToggleButton::with_label("暂停动画（K）");
            let paused = self.state.borrow().active.paused.clone();
            let paused2 = paused.clone();
            pause.connect_toggled(move |b| paused2.set(b.is_active()));
            self.state.borrow_mut().active.animation_pause = Some(pause.clone());
            self.format_bar.append(&pause);
            let weak = canvas.downgrade();
            let label = label.downgrade();
            let mut deadline = Instant::now();
            let displayed = self.state.borrow().active.animation_frames.clone();
            glib::timeout_add_local(Duration::from_millis(16), move || {
                let Some(canvas) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                if !paused.get() && Instant::now() >= deadline {
                    match receiver.try_recv() {
                        Ok(Ok((pixels, delay))) => {
                            displayed.set(displayed.get() + 1);
                            canvas.set_pixels(&pixels);
                            deadline = Instant::now() + Duration::from_millis(delay as u64);
                        }
                        Ok(Err(error)) => {
                            if let Some(label) = label.upgrade() {
                                label.set_text(&format!("动画解码失败：{error}"));
                            }
                            return glib::ControlFlow::Break;
                        }
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            return glib::ControlFlow::Break;
                        }
                        _ => {}
                    }
                }
                glib::ControlFlow::Continue
            });
        } else if image.frames.len() > 1 {
            let pause = gtk4::ToggleButton::with_label("暂停动画（K）");
            let paused = self.state.borrow().active.paused.clone();
            let paused2 = paused.clone();
            pause.connect_toggled(move |b| paused2.set(b.is_active()));
            self.state.borrow_mut().active.animation_pause = Some(pause.clone());
            self.format_bar.append(&pause);
            let frames = image.frames.clone();
            let weak = canvas.downgrade();
            let mut index = 0usize;
            let mut deadline = Instant::now() + Duration::from_millis(frames[0].1 as u64);
            let displayed = self.state.borrow().active.animation_frames.clone();
            glib::timeout_add_local(Duration::from_millis(16), move || {
                let Some(canvas) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                if !paused.get() && Instant::now() >= deadline {
                    index = (index + 1) % frames.len();
                    displayed.set(displayed.get() + 1);
                    canvas.set_pixels(&frames[index].0);
                    deadline = Instant::now() + Duration::from_millis(frames[index].1 as u64);
                }
                glib::ControlFlow::Continue
            });
        }
    }
    fn canvas_controls(self: &Rc<Self>, _canvas: &Canvas) {
        for (icon, tip, action) in [
            ("zoom-fit-best-symbolic", "适应窗口（0）", 0),
            ("zoom-original-symbolic", "原始像素（1）", 1),
            ("object-rotate-right-symbolic", "顺时针旋转（R）", 2),
        ] {
            let button = icon_button(icon, tip);
            let controller = Rc::downgrade(self);
            button.connect_clicked(move |_| {
                if let Some(controller) = controller.upgrade() {
                    let Some(canvas) = controller.state.borrow().active.canvas.clone() else {
                        return;
                    };
                    match action {
                        0 => canvas.fit(),
                        1 => canvas.actual_size(),
                        _ => canvas.rotate(true),
                    }
                }
            });
            self.format_bar.append(&button);
        }
    }
    fn pdf_view(self: &Rc<Self>, file: &FileInfo, pdf: &PdfData) {
        let canvas = Canvas::new();
        canvas.set_pdf(pdf);
        let paned = gtk4::Paned::new(gtk4::Orientation::Horizontal);
        paned.set_hexpand(true);
        paned.set_vexpand(true);
        paned.set_position(180);
        paned.set_resize_start_child(false);
        let thumbnails = self.pdf_thumbnails(file, pdf.total);
        thumbnails.set_visible(false);
        thumbnails.set_size_request(175, -1);
        paned.set_start_child(Some(&thumbnails));
        paned.set_end_child(Some(&canvas));
        self.content.append(&paned);
        let toggle = gtk4::ToggleButton::builder()
            .icon_name("view-list-symbolic")
            .tooltip_text("页缩略图（T）")
            .build();
        let weak = Rc::downgrade(self);
        toggle.connect_toggled(move |b| {
            thumbnails.set_visible(b.is_active());
            if let Some(c) = weak.upgrade() {
                c.state.borrow_mut().pdf_thumbnails = b.is_active();
            }
        });
        let visible = self.state.borrow().pdf_thumbnails;
        toggle.set_active(visible);
        self.state.borrow_mut().active.pdf_sidebar = Some(toggle.clone());
        self.format_bar.append(&toggle);
        {
            let mut state = self.state.borrow_mut();
            state.active.canvas = Some(canvas.clone());
            state.page = pdf.page;
            state.pages = pdf.total;
        }
        let previous = icon_button("go-up-symbolic", "上一页（Page Up）");
        let weak = Rc::downgrade(self);
        previous.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                c.change_page(-1);
            }
        });
        self.format_bar.append(&previous);
        let input = gtk4::SpinButton::with_range(1., pdf.total as f64, 1.);
        input.set_value((pdf.page + 1) as f64);
        input.set_width_chars(4);
        let total = gtk4::Label::new(Some(&format!("/ {} 页", pdf.total)));
        let weak = Rc::downgrade(self);
        input.connect_value_changed(move |input| {
            if let Some(c) = weak.upgrade() {
                let page = input.value_as_int() - 1;
                if let Some(file) = c.displayed() {
                    c.load(
                        file,
                        Spec {
                            page,
                            ..Default::default()
                        },
                    );
                }
            }
        });
        self.format_bar.append(&input);
        self.format_bar.append(&total);
        let next = icon_button("go-down-symbolic", "下一页（Page Down）");
        let weak = Rc::downgrade(self);
        next.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                c.change_page(1);
            }
        });
        self.format_bar.append(&next);
        let continuous = gtk4::Button::with_label("连续滚动");
        continuous.set_widget_name("pdf-continuous-toggle");
        let weak = Rc::downgrade(self);
        continuous.connect_clicked(move |button| {
            if let Some(c) = weak.upgrade() {
                c.toggle_pdf_continuous();
                button.set_label(if c.state.borrow().pdf_continuous {
                    "单页模式"
                } else {
                    "连续滚动"
                });
            }
        });
        self.format_bar.append(&continuous);
        let hint = gtk4::Label::new(Some(if pdf.text.is_empty() {
            "扫描页面 · 无文本层"
        } else {
            "拖动选字 · Shift+拖动平移"
        }));
        hint.set_widget_name("pdf-hint");
        hint.set_hexpand(true);
        hint.set_xalign(1.);
        self.format_bar.append(&hint);
        self.canvas_controls(&canvas);
        // 让键盘一开始就落到画布上：窗口首个可聚焦控件是页码 SpinButton，
        // 焦点停在那里时所有快捷键都会被当成「正在输入」而忽略（F / +/- 全失灵）。
        canvas.grab_focus();
    }
    fn pdf_continuous(self: &Rc<Self>, file: &FileInfo, total: i32, aspect: f64) {
        clear(&self.content);
        {
            let mut s = self.state.borrow_mut();
            s.pdf_continuous = true;
            s.active.canvas = None;
            s.pdf_aspect = aspect;
            s.pdf_zoom = 1.0;
            s.pdf_pages.clear();
            s.pdf_scroll = None;
            if let Some(button) = &s.active.pdf_sidebar {
                button.set_visible(false);
            }
        }
        let mut control = self.format_bar.first_child();
        while let Some(widget) = control {
            if widget.widget_name() == "pdf-continuous-toggle"
                && let Some(button) = widget.downcast_ref::<gtk4::Button>()
            {
                button.set_label("单页模式");
            }
            control = widget.next_sibling();
        }
        let strings: Vec<_> = (0..total.min(100_000)).map(|i| i.to_string()).collect();
        let model = gtk4::StringList::new(&strings.iter().map(String::as_str).collect::<Vec<_>>());
        let factory = gtk4::SignalListItemFactory::new();
        let weak = Rc::downgrade(self);
        let file = file.clone();
        let generation = self.state.borrow().generation;
        factory.connect_bind(move |_, item| {
            let Some(c) = weak.upgrade() else {
                return;
            };
            let item = item.downcast_ref::<gtk4::ListItem>().unwrap();
            let canvas = Canvas::new();
            let weak = Rc::downgrade(&c);
            let page = item.position() as i32;
            canvas.connect_has_focus_notify(move |canvas| {
                if canvas.has_focus()
                    && let Some(c) = weak.upgrade()
                {
                    let mut s = c.state.borrow_mut();
                    s.active.canvas = Some(canvas.clone());
                    s.page = page;
                }
            });
            canvas.set_vexpand(false);
            canvas.set_scroll_zoom(false);
            let zoom = {
                let mut s = c.state.borrow_mut();
                s.pdf_pages.push(canvas.downgrade());
                s.pdf_zoom
            };
            canvas.set_zoom(zoom);
            canvas.set_size_request(-1, (640. * aspect * zoom).clamp(120., 6000.) as i32);
            item.set_child(Some(&canvas));
            c.submit(
                Task::Prepare(
                    file.clone(),
                    Spec {
                        page: item.position() as i32,
                        edge: Some(1600),
                        ..Default::default()
                    },
                ),
                Target::PdfPage {
                    generation,
                    canvas: canvas.downgrade(),
                },
                false,
            );
        });
        let list = gtk4::ListView::new(Some(gtk4::NoSelection::new(Some(model))), Some(factory));
        let scrolled = scroll(&list);
        {
            let mut s = self.state.borrow_mut();
            s.pdf_scroll = Some(scrolled.downgrade());
        }
        // Ctrl+滚轮：一次缩放所有页，而不是只缩放鼠标下那一页。
        let wheel = gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::VERTICAL);
        let weak = Rc::downgrade(self);
        wheel.connect_scroll(move |controller, _dx, dy| {
            let Some(c) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let ctrl = controller
                .current_event()
                .map(|event| {
                    event
                        .modifier_state()
                        .contains(gdk::ModifierType::CONTROL_MASK)
                })
                .unwrap_or(false);
            if !ctrl {
                return glib::Propagation::Proceed;
            }
            c.zoom_all_pages(if dy < 0. { 1.2 } else { 1. / 1.2 });
            glib::Propagation::Stop
        });
        scrolled.add_controller(wheel);
        self.content.append(&scrolled);
        list.grab_focus();
        let start = self.state.borrow().page.max(0) as u32;
        list.scroll_to(start, gtk4::ListScrollFlags::NONE, None);
        // 提示同时写进格式栏，避免底部状态行被别的提示覆盖后用户找不到缩放说明。
        let mut control = self.format_bar.first_child();
        while let Some(widget) = control {
            if widget.widget_name() == "pdf-hint"
                && let Some(label) = widget.downcast_ref::<gtk4::Label>()
            {
                label.set_text("连续滚动 · Ctrl+滚轮或 +/- 缩放全部页面 · Page Up/Down 翻页");
            }
            control = widget.next_sibling();
        }
        self.status
            .set_text("连续滚动 · Ctrl+滚轮或 +/- 缩放全部页面 · Page Up/Down 翻页");
    }
    pub(super) fn toggle_pdf_continuous(self: &Rc<Self>) {
        let (prepared, continuous, page) = {
            let s = self.state.borrow();
            (s.prepared.clone(), s.pdf_continuous, s.page)
        };
        let Some(prepared) = prepared else {
            return;
        };
        let Body::Pdf(pdf) = &prepared.body else {
            return;
        };
        if continuous {
            {
                let mut s = self.state.borrow_mut();
                s.pdf_pages.clear();
                s.pdf_scroll = None;
                s.pdf_zoom = 1.0;
            }
            self.load(
                prepared.info.clone(),
                Spec {
                    page,
                    ..Default::default()
                },
            );
        } else {
            self.pdf_continuous(
                &prepared.info,
                pdf.total,
                pdf.pixels.height as f64 / pdf.pixels.width as f64,
            );
        }
    }
    fn pdf_thumbnails(self: &Rc<Self>, file: &FileInfo, total: i32) -> gtk4::ScrolledWindow {
        let strings: Vec<_> = (0..total.min(100_000)).map(|i| i.to_string()).collect();
        let model = gtk4::StringList::new(&strings.iter().map(String::as_str).collect::<Vec<_>>());
        let factory = gtk4::SignalListItemFactory::new();
        let weak = Rc::downgrade(self);
        let file_for_bind = file.clone();
        let generation = self.state.borrow().generation;
        factory.connect_bind(move |_, item| {
            let Some(c) = weak.upgrade() else {
                return;
            };
            let item = item.downcast_ref::<gtk4::ListItem>().unwrap();
            let column = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
            let picture = gtk4::Picture::new();
            picture.set_size_request(145, 190);
            column.append(&picture);
            column.append(&gtk4::Label::new(Some(&(item.position() + 1).to_string())));
            item.set_child(Some(&column));
            c.submit(
                Task::Prepare(
                    file_for_bind.clone(),
                    Spec {
                        page: item.position() as i32,
                        edge: Some(240),
                        ..Default::default()
                    },
                ),
                Target::Thumbnail {
                    generation,
                    picture: picture.downgrade(),
                },
                true,
            );
        });
        let selection = gtk4::SingleSelection::new(Some(model));
        // 侧栏每次换页都会重建：不把选中项对准当前页，第 1 页会一直显示高亮，
        // 翻到后面的页时侧栏也不会跟着滚。
        let current = self.state.borrow().page.clamp(0, (total - 1).max(0)) as u32;
        selection.set_selected(current);
        let list = gtk4::ListView::new(Some(selection), Some(factory));
        list.set_single_click_activate(true);
        // 构造时列表还没 map/分配，scroll_to 是空操作；等显示出来再滚到当前页。
        let list_for_scroll = list.clone();
        list.connect_map(move |_| {
            let list = list_for_scroll.clone();
            glib::timeout_add_local_once(Duration::from_millis(60), move || {
                if list.is_mapped() {
                    list.scroll_to(current, gtk4::ListScrollFlags::NONE, None);
                }
            });
        });
        let weak = Rc::downgrade(self);
        let file = file.clone();
        list.connect_activate(move |_, page| {
            if let Some(c) = weak.upgrade() {
                c.load(
                    file.clone(),
                    Spec {
                        page: page as i32,
                        ..Default::default()
                    },
                );
            }
        });
        scroll(&list)
    }
    fn text_view(
        &self,
        file: &FileInfo,
        text: &quicklook_core::text::TextChunk,
    ) -> sourceview5::View {
        let buffer = sourceview5::Buffer::new(None::<&gtk4::TextTagTable>);
        let manager = sourceview5::LanguageManager::default();
        if let Some(language) = manager.guess_language(Some(&file.path), Some(&file.mime)) {
            buffer.set_language(Some(&language));
        }
        let schemes = sourceview5::StyleSchemeManager::default();
        let scheme = if self.config.uses_dark_theme() {
            schemes
                .scheme("Adwaita-dark")
                .or_else(|| schemes.scheme("oblivion"))
        } else {
            schemes
                .scheme("Adwaita")
                .or_else(|| schemes.scheme("classic"))
        };
        if let Some(scheme) = scheme {
            buffer.set_style_scheme(Some(&scheme));
        }
        buffer.set_text(&text.text);
        let view = sourceview5::View::with_buffer(&buffer);
        view.set_editable(false);
        view.set_monospace(true);
        view.set_show_line_numbers(true);
        view.set_highlight_current_line(false);
        view.set_left_margin(14);
        view.set_right_margin(14);
        view.set_top_margin(12);
        view.set_bottom_margin(12);
        view.set_wrap_mode(gtk4::WrapMode::None);
        self.state.borrow_mut().active.text = Some(view.clone());
        self.state.borrow_mut().active.text_bytes = text.text.len();
        view
    }
    /// CSV 的格式栏：行/列摘要 + 编码切换（中文 CSV 常见 GBK）。
    fn csv_controls(self: &Rc<Self>, doc: &crate::csv::CsvDoc) {
        let mut text = doc.summary();
        text.push_str(" · ");
        text.push_str(&doc.source.encoding);
        if doc.source.had_errors {
            text.push_str("（含替换字符）");
        }
        let label = gtk4::Label::new(Some(&text));
        label.set_hexpand(true);
        label.set_xalign(0.);
        self.format_bar.append(&label);
        let encoding = gtk4::Button::with_label("切换编码（E）");
        let weak = Rc::downgrade(self);
        encoding.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                c.cycle_encoding();
            }
        });
        self.format_bar.append(&encoding);
    }

    fn text_controls(
        self: &Rc<Self>,
        file: &FileInfo,
        text: &quicklook_core::text::TextChunk,
        view: &sourceview5::View,
    ) {
        let label = gtk4::Label::new(Some(&format!(
            "{}{} · 已读 {} / {}",
            text.encoding,
            if text.had_errors {
                "（含替换字符）"
            } else {
                ""
            },
            format_size(text.next_offset),
            format_size(text.total)
        )));
        label.set_hexpand(true);
        label.set_xalign(0.);
        self.format_bar.append(&label);
        let encoding = gtk4::Button::with_label("切换编码（E）");
        let weak = Rc::downgrade(self);
        encoding.connect_clicked(move |_| {
            if let Some(c) = weak.upgrade() {
                c.cycle_encoding();
            }
        });
        self.format_bar.append(&encoding);
        let wrap = gtk4::ToggleButton::with_label("自动换行（W）");
        let weak = view.downgrade();
        wrap.connect_toggled(move |b| {
            if let Some(v) = weak.upgrade() {
                v.set_wrap_mode(if b.is_active() {
                    gtk4::WrapMode::WordChar
                } else {
                    gtk4::WrapMode::None
                });
            }
        });
        self.format_bar.append(&wrap);
        if text.next_offset < text.total {
            let more = gtk4::Button::with_label("继续加载");
            let weak = Rc::downgrade(self);
            let view = view.downgrade();
            let file = file.clone();
            more.connect_clicked(move |button| {
                if let Some(c) = weak.upgrade() {
                    let state = c.state.borrow();
                    let chunk = state.prepared.as_ref().and_then(|p| match &p.body {
                        Body::Text(t) | Body::Markdown { source: t, .. } => Some(t.clone()),
                        _ => None,
                    });
                    let generation = state.generation;
                    drop(state);
                    if let Some(chunk) = chunk {
                        button.set_sensitive(false);
                        let mut info = file.clone();
                        info.kind = Kind::Text;
                        c.submit(
                            Task::Prepare(
                                info,
                                Spec {
                                    offset: chunk.next_offset,
                                    encoding: Some(chunk.encoding),
                                    ..Default::default()
                                },
                            ),
                            Target::Append {
                                generation,
                                view: view.clone(),
                                button: button.downgrade(),
                            },
                            false,
                        );
                    }
                }
            });
            self.format_bar.append(&more);
        }
    }
    fn web_view(self: &Rc<Self>, html: &str, file: &FileInfo) -> webkit6::WebView {
        self.web_view_with(html, file, false)
    }
    /// `diagrams` 为真时给这个 WebView 打开 JavaScript，并注入内置 Mermaid 渲染流程图。
    ///
    /// 只有 Markdown 里确实存在 ```mermaid 代码块才会走到这里；HTML/Office 等其它视图
    /// 仍然完全关闭 JavaScript。
    fn web_view_with(
        self: &Rc<Self>,
        html: &str,
        file: &FileInfo,
        diagrams: bool,
    ) -> webkit6::WebView {
        let diagrams = diagrams && self.config.mermaid && !crate::mermaid::bundle().is_empty();
        let generation = self.state.borrow().generation;
        let (view, manager) = if diagrams {
            let manager = webkit6::UserContentManager::new();
            manager.add_script(&webkit6::UserScript::new(
                crate::mermaid::bundle(),
                webkit6::UserContentInjectedFrames::TopFrame,
                webkit6::UserScriptInjectionTime::Start,
                &[],
                &[],
            ));
            manager.add_script(&webkit6::UserScript::new(
                &crate::mermaid::bootstrap(self.config.uses_dark_theme()),
                webkit6::UserContentInjectedFrames::TopFrame,
                webkit6::UserScriptInjectionTime::End,
                &[],
                &[],
            ));
            let weak = Rc::downgrade(self);
            manager.connect_script_message_received(None, move |_, value| {
                let report = value.to_str().to_string();
                if let Some(c) = weak.upgrade() {
                    let mut s = c.state.borrow_mut();
                    if s.generation == generation {
                        if let Some(error) = report.strip_prefix("error:") {
                            eprintln!("quicklook: Mermaid 渲染失败：{error}");
                        }
                        s.mermaid = report;
                    }
                }
            });
            manager.register_script_message_handler(crate::mermaid::HANDLER, None);
            let view = webkit6::WebView::builder()
                .network_session(&webkit6::NetworkSession::new_ephemeral())
                .user_content_manager(&manager)
                .hexpand(true)
                .vexpand(true)
                .build();
            (view, Some(manager))
        } else {
            (
                webkit6::WebView::builder()
                    .network_session(&webkit6::NetworkSession::new_ephemeral())
                    .hexpand(true)
                    .vexpand(true)
                    .build(),
                None,
            )
        };
        let _ = manager;
        if let Some(settings) = webkit6::prelude::WebViewExt::settings(&view) {
            settings.set_enable_javascript(diagrams);
            settings.set_enable_html5_database(false);
            settings.set_enable_html5_local_storage(false);
            settings.set_allow_file_access_from_file_urls(false);
            settings.set_allow_universal_access_from_file_urls(false);
        }
        let html = format!(
            "<!doctype html><html{}><head><meta charset='utf-8'><meta http-equiv='Content-Security-Policy' content=\"default-src 'none'; img-src data: file:; style-src 'unsafe-inline'; font-src data:; base-uri 'none'; form-action 'none'\"><style>{}</style></head><body>{html}</body></html>",
            if self.config.uses_dark_theme() {
                " class='dark'"
            } else {
                ""
            },
            include_str!("web.css")
        );
        let base =
            gio::File::for_path(file.path.parent().unwrap_or(std::path::Path::new("/"))).uri();
        let weak = Rc::downgrade(self);
        view.connect_load_changed(move |_, event| {
            if event == webkit6::LoadEvent::Finished
                && let Some(c) = weak.upgrade()
            {
                let mut s = c.state.borrow_mut();
                if s.generation == generation {
                    s.ready = true;
                    s.last_elapsed_ms = s.request_start.elapsed().as_secs_f64() * 1000.;
                }
            }
        });
        let weak = Rc::downgrade(self);
        view.connect_load_failed(move |_, _, _, error| {
            let weak = weak.clone();
            let message = format!("文档渲染失败：{error}");
            glib::idle_add_local_once(move || {
                if let Some(c) = weak.upgrade()
                    && c.state.borrow().generation == generation
                {
                    c.error(&message);
                }
            });
            true
        });
        let weak = Rc::downgrade(self);
        view.connect_web_process_terminated(move |_, reason| {
            let weak = weak.clone();
            let message = format!("文档渲染进程退出：{reason:?}");
            glib::idle_add_local_once(move || {
                if let Some(c) = weak.upgrade()
                    && c.state.borrow().generation == generation
                {
                    c.error(&message);
                }
            });
        });
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_secs(15), move || {
            if let Some(c) = weak.upgrade() {
                let timed_out = {
                    let s = c.state.borrow();
                    s.generation == generation && !s.ready
                };
                if timed_out {
                    c.error("文档渲染超时，可切换默认应用打开");
                }
            }
        });
        view.load_html(&html, Some(&base));
        view.connect_decide_policy(|_, decision, kind| {
            if kind == webkit6::PolicyDecisionType::NavigationAction
                && let Some(nav) = decision.downcast_ref::<webkit6::NavigationPolicyDecision>()
                && let Some(action) = nav.navigation_action()
                && let Some(uri) = action.request().and_then(|r| r.uri())
                && (uri.starts_with("http:")
                    || uri.starts_with("https:")
                    || uri.starts_with("mailto:"))
            {
                if action.is_user_gesture() {
                    let _ =
                        gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>);
                }
                decision.ignore();
                return true;
            }
            false
        });
        view
    }
    fn file_list(self: &Rc<Self>, files: Vec<FileInfo>) {
        let files = Rc::new(files);
        let model =
            gtk4::StringList::new(&files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>());
        let factory = gtk4::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
            let icon = gtk4::Image::new();
            icon.set_pixel_size(24);
            row.append(&icon);
            let label = gtk4::Label::new(None);
            label.set_hexpand(true);
            label.set_xalign(0.);
            label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
            row.append(&label);
            let size = gtk4::Label::new(None);
            size.add_css_class("dim-label");
            row.append(&size);
            item.downcast_ref::<gtk4::ListItem>()
                .unwrap()
                .set_child(Some(&row));
        });
        let files2 = files.clone();
        factory.connect_bind(move |_, item| {
            let item = item.downcast_ref::<gtk4::ListItem>().unwrap();
            let row = item.child().and_downcast::<gtk4::Box>().unwrap();
            if let Some(file) = files2.get(item.position() as usize) {
                row.first_child()
                    .and_downcast::<gtk4::Image>()
                    .unwrap()
                    .set_icon_name(Some(kind_icon(file.kind)));
                row.first_child()
                    .unwrap()
                    .next_sibling()
                    .and_downcast::<gtk4::Label>()
                    .unwrap()
                    .set_text(&file.name);
                row.last_child()
                    .and_downcast::<gtk4::Label>()
                    .unwrap()
                    .set_text(&format_size(file.size));
            }
        });
        let list =
            gtk4::ListView::new(Some(gtk4::SingleSelection::new(Some(model))), Some(factory));
        let weak = Rc::downgrade(self);
        list.connect_activate(move |_, position| {
            if let (Some(c), Some(file)) = (weak.upgrade(), files.get(position as usize)) {
                let _ = c.show(vec![file.uri.clone()], ShowMode::Show);
            }
        });
        self.content.append(&scroll(&list));
    }
    pub(super) fn show_grid(self: &Rc<Self>, files: Vec<FileInfo>) {
        self.begin();
        self.state.borrow_mut().nested.clear();
        {
            let mut s = self.state.borrow_mut();
            s.grid = true;
            s.current = files.first().cloned();
            s.ready = true;
        }
        self.title.set_text(&format!("{} 个文件", files.len()));
        self.subtitle.set_text("双击缩略图打开单文件预览");
        let generation = self.state.borrow().generation;
        let files = Rc::new(files);
        let model =
            gtk4::StringList::new(&files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>());
        let factory = gtk4::SignalListItemFactory::new();
        let weak = Rc::downgrade(self);
        let files2 = files.clone();
        factory.connect_bind(move |_, item| {
            let Some(c) = weak.upgrade() else {
                return;
            };
            let item = item.downcast_ref::<gtk4::ListItem>().unwrap();
            let Some(file) = files2.get(item.position() as usize) else {
                return;
            };
            let tile = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
            tile.add_css_class("file-tile");
            let picture = gtk4::Picture::new();
            picture.set_size_request(150, 130);
            picture.set_can_shrink(true);
            tile.append(&picture);
            let label = gtk4::Label::new(Some(&file.name));
            label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
            label.set_max_width_chars(19);
            tile.append(&label);
            item.set_child(Some(&tile));
            if matches!(file.kind, Kind::Image | Kind::Psd | Kind::Raw | Kind::Pdf) {
                c.submit(
                    Task::Prepare(
                        file.clone(),
                        Spec {
                            edge: Some(256),
                            ..Default::default()
                        },
                    ),
                    Target::Thumbnail {
                        generation,
                        picture: picture.downgrade(),
                    },
                    true,
                );
            } else {
                let icon = gtk4::Image::from_icon_name(kind_icon(file.kind));
                icon.set_pixel_size(64);
                tile.prepend(&icon);
                picture.set_visible(false);
            }
        });
        let grid =
            gtk4::GridView::new(Some(gtk4::SingleSelection::new(Some(model))), Some(factory));
        grid.set_min_columns(1);
        grid.set_max_columns(6);
        let weak = Rc::downgrade(self);
        grid.connect_activate(move |_, position| {
            if let (Some(c), Some(file)) = (weak.upgrade(), files.get(position as usize)) {
                c.state.borrow_mut().index = position as usize;
                c.load(file.clone(), Spec::default());
            }
        });
        self.content.append(&scroll(&grid));
        self.status
            .set_text("多选预览 · 双击放大 · ↑↓ 在选中文件中导航");
        self.format_bar.set_visible(false);
    }
    pub(super) fn properties(&self) {
        if self.inspector.reveals_child() {
            self.inspector.set_reveal_child(false);
            return;
        }
        self.update_properties();
        self.inspector.set_reveal_child(true);
    }
    pub(super) fn update_properties(&self) {
        if let Some(file) = self.displayed().as_ref() {
            let body = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
            body.set_size_request(270, -1);
            body.set_margin_start(20);
            body.set_margin_end(20);
            body.set_margin_top(20);
            body.set_margin_bottom(20);
            let mut details = format!(
                "{}\n\n类型：{}\n大小：{}（{} 字节）\n位置：{}\nURI：{}",
                file.name,
                file.mime,
                format_size(file.size),
                file.size,
                file.path.display(),
                file.uri
            );
            if let Some(prepared) = self.state.borrow().prepared.as_ref()
                && let Body::Image(image) = &prepared.body
            {
                details.push_str(&format!("\n{}", image.details));
                for (key, value) in &image.exif {
                    details.push_str(&format!("\n{key}：{value}"));
                }
            }
            let label = gtk4::Label::new(Some(&details));
            label.set_max_width_chars(32);
            label.set_selectable(true);
            label.set_wrap(true);
            label.set_xalign(0.);
            body.append(&label);
            self.inspector.set_child(Some(&scroll(&body)));
        }
    }
    fn font_view(&self, file: &FileInfo, font: &crate::backend::FontData) {
        unsafe extern "C" {
            fn FcConfigGetCurrent() -> *mut std::ffi::c_void;
            fn FcConfigAppFontAddFile(config: *mut std::ffi::c_void, file: *const u8) -> i32;
        }
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(file.path.as_os_str().as_bytes());
        if let Ok(name) = name {
            unsafe {
                FcConfigAppFontAddFile(FcConfigGetCurrent(), name.as_ptr().cast());
            }
        }
        let body = gtk4::Box::new(gtk4::Orientation::Vertical, 24);
        {
            let (side, top) = (24, 20);
            body.set_margin_start(side);
            body.set_margin_end(side);
            body.set_margin_top(top);
            body.set_margin_bottom(top);
        }
        let heading = gtk4::Label::new(Some(&format!("{} · {} 个字形", font.family, font.glyphs)));
        heading.set_xalign(0.);
        heading.add_css_class("section-title");
        body.append(&heading);
        for (size, text) in [
            (40, "The quick brown fox jumps over the lazy dog"),
            (28, "中文字体预览 · 你好，世界！ 0123456789"),
            (20, font.samples.as_str()),
        ] {
            let label = gtk4::Label::new(Some(text));
            label.set_xalign(0.);
            label.set_wrap(true);
            label.set_selectable(true);
            let attributes = gtk4::pango::AttrList::new();
            attributes.insert(gtk4::pango::AttrString::new_family(&font.family));
            attributes.insert(gtk4::pango::AttrSize::new(size * gtk4::pango::SCALE));
            label.set_attributes(Some(&attributes));
            body.append(&label);
        }
        self.content.append(&scroll(&body));
    }
}
fn scroll(widget: &impl IsA<gtk4::Widget>) -> gtk4::ScrolledWindow {
    gtk4::ScrolledWindow::builder()
        .child(widget)
        .hexpand(true)
        .vexpand(true)
        .build()
}
fn kind_icon(kind: Kind) -> &'static str {
    match kind {
        Kind::Directory => "folder-symbolic",
        Kind::Image | Kind::Raw | Kind::Psd => "image-x-generic-symbolic",
        Kind::Media => "video-x-generic-symbolic",
        Kind::Pdf | Kind::Eps => "x-office-document-symbolic",
        Kind::Archive => "package-x-generic-symbolic",
        Kind::Font => "font-x-generic-symbolic",
        _ => "text-x-generic-symbolic",
    }
}
fn append_json(parent: &gtk4::Box, key: &str, value: serde_json::Value, depth: usize) {
    if depth > 64 {
        parent.append(&gtk4::Label::new(Some("层级较深，请切换源码查看")));
        return;
    }
    match value {
        serde_json::Value::Object(map) => {
            let expander = gtk4::Expander::new(Some(&format!("{key}  {{ {} }}", map.len())));
            let children = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
            children.set_margin_start(18);
            expander.set_child(Some(&children));
            let loaded = Cell::new(false);
            expander.connect_expanded_notify(move |e| {
                if e.is_expanded() && !loaded.replace(true) {
                    for (key, value) in map.iter().take(500) {
                        append_json(&children, key, value.clone(), depth + 1);
                    }
                    if map.len() > 500 {
                        children.append(&gtk4::Label::new(Some("仅展示前 500 项，完整内容见源码")));
                    }
                }
            });
            parent.append(&expander);
            if depth == 0 {
                expander.set_expanded(true);
            }
        }
        serde_json::Value::Array(values) => {
            let expander = gtk4::Expander::new(Some(&format!("{key}  [ {} ]", values.len())));
            let children = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
            children.set_margin_start(18);
            expander.set_child(Some(&children));
            let loaded = Cell::new(false);
            expander.connect_expanded_notify(move |e| {
                if e.is_expanded() && !loaded.replace(true) {
                    for (index, value) in values.iter().take(500).enumerate() {
                        append_json(&children, &format!("[{index}]"), value.clone(), depth + 1);
                    }
                    if values.len() > 500 {
                        children.append(&gtk4::Label::new(Some("仅展示前 500 项，完整内容见源码")));
                    }
                }
            });
            parent.append(&expander);
        }
        value => {
            let text = format!("{key}: {value}");
            let text = if text.chars().count() > 8192 {
                format!(
                    "{}…（完整内容见源码）",
                    text.chars().take(8192).collect::<String>()
                )
            } else {
                text
            };
            let label = gtk4::Label::new(Some(&text));
            label.set_xalign(0.);
            label.set_selectable(true);
            label.set_wrap(true);
            parent.append(&label);
        }
    }
}

/// JSON / JSONC 默认用源码视图；XML、YAML 仍然先看结构树。
fn json_like(name: &str) -> bool {
    let extension = std::path::Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    matches!(extension.as_str(), "json" | "jsonc")
}
