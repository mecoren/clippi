//! Add/edit sync backend dialog.
//!
//! 外壳与字段全部来自控件工厂（`dialog_panel` / `text_field` / `btn` /
//! `tool_icon_btn`），页面只负责排版与事件；颜色统一取 `Palette`。

use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement;

use crate::core::i18n_keys::I18nKey;
use crate::core::settings::{compose_webdav_url, BackendConfig};
use crate::services::backends::local_folder::detect_presets;
use crate::services::gpui_sync::test_webdav_connection;
use crate::ui::design::motion;
use crate::ui::design::palette::Palette;
use crate::ui::design::size::{IconSize, Radius, Space};
use crate::ui::design::typography::{self, type_role};
use crate::ui::transition::keyed_transition;
use crate::ui::widgets::{self, ButtonExt, Tone};
use crate::ui::window_manager::{WebDavBackendForm, WindowManager};

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditorStep {
    SelectType,
    LocalFolder,
    WebDav,
}

pub struct AddBackendPanel {
    visible: bool,
    animation_generation: u64,
    animation_started: Option<Instant>,
    edit_id: Option<String>,
    step: EditorStep,
    window_manager: Entity<WindowManager>,
    presets: Vec<(&'static str, String)>,
    name_input: Entity<InputState>,
    folder_input: Entity<InputState>,
    url_input: Entity<InputState>,
    webdav_path_input: Entity<InputState>,
    username_input: Entity<InputState>,
    password_input: Entity<InputState>,
    test_pending: bool,
    test_ok: bool,
    test_error: String,
    _test_task: Option<Task<()>>,
    _folder_dialog_task: Option<Task<()>>,
    last_lang_version: u64,
    _input_subscriptions: Vec<Subscription>,
}

impl AddBackendPanel {
    pub fn new(
        window_manager: Entity<WindowManager>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut panel = Self {
            visible: false,
            animation_generation: 0,
            animation_started: None,
            edit_id: None,
            step: EditorStep::SelectType,
            window_manager,
            presets: detect_presets(),
            name_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(I18nKey::BackendPhName.text())),
            folder_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(I18nKey::BackendPhFolder.text())),
            url_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(I18nKey::BackendPhUrl.text())),
            webdav_path_input: cx.new(|cx| {
                InputState::new(window, cx).placeholder(I18nKey::BackendPhWebdavPath.text())
            }),
            username_input: cx
                .new(|cx| InputState::new(window, cx).placeholder(I18nKey::BackendPhUser.text())),
            password_input: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(I18nKey::BackendPhPass.text())
                    .masked(true)
            }),
            test_pending: false,
            test_ok: false,
            test_error: String::new(),
            _test_task: None,
            _folder_dialog_task: None,
            last_lang_version: crate::core::i18n::lang_version(),
            _input_subscriptions: Vec::new(),
        };
        for input in [
            &panel.url_input,
            &panel.webdav_path_input,
            &panel.username_input,
            &panel.password_input,
        ] {
            panel._input_subscriptions.push(cx.subscribe(
                input,
                |panel, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        panel.reset_test();
                        cx.notify();
                    }
                },
            ));
        }
        panel
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn open_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.visible = true;
        self.animation_generation = self.animation_generation.wrapping_add(1);
        self.animation_started = Some(Instant::now());
        self.edit_id = None;
        self.step = EditorStep::SelectType;
        self.clear_inputs(window, cx);
        self.reset_test();
        cx.notify();
    }

    pub fn open_edit(
        &mut self,
        config: &BackendConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.visible = true;
        self.animation_generation = self.animation_generation.wrapping_add(1);
        self.animation_started = Some(Instant::now());
        self.edit_id = Some(config.id.clone());
        self.step = if config.backend_type == "webdav" {
            EditorStep::WebDav
        } else {
            EditorStep::LocalFolder
        };
        self.name_input
            .update(cx, |input, cx| input.set_value(&config.name, window, cx));
        self.folder_input.update(cx, |input, cx| {
            input.set_value(&config.folder_path, window, cx)
        });
        self.url_input.update(cx, |input, cx| {
            let root = if config.webdav_root_url.trim().is_empty() {
                config.webdav_url.clone()
            } else {
                config.webdav_root_url.clone()
            };
            input.set_value(root, window, cx)
        });
        self.webdav_path_input.update(cx, |input, cx| {
            input.set_value(config.webdav_path.clone(), window, cx)
        });
        self.username_input.update(cx, |input, cx| {
            input.set_value(&config.webdav_username, window, cx)
        });
        self.password_input.update(cx, |input, cx| {
            input.set_value(&config.webdav_password, window, cx)
        });
        self.reset_test();
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.visible = false;
        self.animation_started = None;
        self.test_pending = false;
        self._test_task = None;
        cx.notify();
    }

    fn clear_inputs(&self, window: &mut Window, cx: &mut Context<Self>) {
        for input in [
            &self.name_input,
            &self.folder_input,
            &self.url_input,
            &self.webdav_path_input,
            &self.username_input,
            &self.password_input,
        ] {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
    }

    fn reset_test(&mut self) {
        self._test_task = None;
        self.test_pending = false;
        self.test_ok = false;
        self.test_error.clear();
    }

    fn transition_f32(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        key: (&'static str, u64),
        initial: f32,
        target: f32,
    ) -> f32 {
        keyed_transition(
            window,
            cx,
            key,
            motion::fast(),
            initial,
            motion::ease_out,
            target,
        )
    }

    fn start_webdav_test(&mut self, cx: &mut Context<Self>) {
        let url = compose_webdav_url(
            self.url_input.read(cx).value(),
            self.webdav_path_input.read(cx).value(),
        );
        if url.trim().is_empty() || self.test_pending {
            return;
        }
        let username = self.username_input.read(cx).value().to_string();
        let password = self.password_input.read(cx).unmask_value().to_string();
        self.test_pending = true;
        self.test_ok = false;
        self.test_error.clear();

        let background =
            cx.background_spawn(async move { test_webdav_connection(&url, &username, &password) });
        self._test_task = Some(cx.spawn(async move |weak, cx| {
            let ok = background.await;
            if let Some(this) = weak.upgrade() {
                this.update(cx, |panel, cx| {
                    panel.test_pending = false;
                    panel.test_ok = ok;
                    panel.test_error = if ok {
                        String::new()
                    } else {
                        I18nKey::BackendTestFail.text().into()
                    };
                    cx.notify();
                });
            }
        }));
        cx.notify();
    }

    fn title(&self) -> &'static str {
        if self.edit_id.is_some() {
            I18nKey::BackendEditTitle.text()
        } else {
            match self.step {
                EditorStep::SelectType => I18nKey::BackendAddTitle.text(),
                EditorStep::LocalFolder => I18nKey::BackendLocalFolder.text(),
                EditorStep::WebDav => I18nKey::BackendWebdav.text(),
            }
        }
    }

    fn render_type_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity().clone();
        let p = Palette::current(cx);

        div()
            .flex()
            .flex_col()
            .gap(Space::Md.scaled())
            .child(field_label(&p, I18nKey::BackendSelectType.text()))
            .child(
                div()
                    .flex()
                    .gap(Space::Md.scaled())
                    .child(type_card(
                        &p,
                        I18nKey::BackendLocalFolder.text(),
                        I18nKey::BackendLocalDesc.text(),
                        {
                            let this = this.clone();
                            move |_window, cx| {
                                this.update(cx, |panel, cx| {
                                    panel.step = EditorStep::LocalFolder;
                                    cx.notify();
                                });
                            }
                        },
                    ))
                    .child(type_card(
                        &p,
                        I18nKey::BackendWebdav.text(),
                        I18nKey::BackendWebdavDesc.text(),
                        move |window, cx| {
                            this.update(cx, |panel, cx| {
                                panel.step = EditorStep::WebDav;
                                if panel.name_input.read(cx).value().is_empty() {
                                    panel.name_input.update(cx, |input, cx| {
                                        input.set_value(I18nKey::BackendWebdav.text(), window, cx)
                                    });
                                }
                                cx.notify();
                            });
                        },
                    )),
            )
            .into_any_element()
    }

    fn render_local_form(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let editing = self.edit_id.is_some();
        let p = Palette::current(cx);
        let this = cx.entity().clone();

        div()
            .flex()
            .flex_col()
            .gap(Space::Lg.scaled())
            .when(!editing && !self.presets.is_empty(), |form| {
                let presets: Vec<AnyElement> = self
                    .presets
                    .iter()
                    .map(|(name, path)| {
                        let input_name = (*name).to_string();
                        let path = path.clone();
                        let name_input = self.name_input.clone();
                        let folder_input = self.folder_input.clone();
                        let glyph = if matches!(*name, "OneDrive" | "iCloud") {
                            "CloudIcon"
                        } else {
                            "HelpIcon"
                        };
                        widgets::btn_small(
                            &p,
                            ElementId::Name(format!("backend-preset-{name}").into()),
                            *name,
                            Some(glyph),
                            Tone::Plain,
                        )
                        .on_click(move |_, window, cx| {
                            if name_input.read(cx).value().is_empty() {
                                name_input.update(cx, |input, cx| {
                                    input.set_value(&input_name, window, cx)
                                });
                            }
                            folder_input.update(cx, |input, cx| input.set_value(&path, window, cx));
                        })
                        .into_any_element()
                    })
                    .collect();
                form.child(field_label(&p, I18nKey::BackendQuickAdd.text()))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(Space::Sm.scaled())
                            .children(presets),
                    )
            })
            .when(!editing && !self.presets.is_empty(), |form| {
                form.child(div().h(px(1.)).bg(p.stroke))
            })
            .child(field_label(&p, I18nKey::BackendFolder.text()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(Space::Sm.scaled())
                    .child(div().flex_1().min_w(px(0.)).child(input_box(
                        &p,
                        &self.folder_input,
                        window,
                        cx,
                    )))
                    .child(
                        widgets::btn_small(
                            &p,
                            "backend-browse",
                            I18nKey::BackendBrowse.text(),
                            None,
                            Tone::Plain,
                        )
                        .on_click({
                            let folder_input = self.folder_input.clone();
                            let this = this.clone();
                            move |_window, window, cx| {
                                let input = folder_input.clone();
                                let window_handle = window.window_handle();
                                let dialog_guard = this
                                    .read(cx)
                                    .window_manager
                                    .read(cx)
                                    .suspend_auto_hide_for_dialog();
                                let dialog = rfd::AsyncFileDialog::new().pick_folder();
                                let task = cx.spawn(async move |cx| {
                                    let selected = dialog.await;
                                    drop(dialog_guard);
                                    if let Some(path) = selected {
                                        let path = path.path().to_string_lossy().to_string();
                                        let _ = cx.update_window(window_handle, |_, window, cx| {
                                            input.update(cx, |input, cx| {
                                                input.set_value(&path, window, cx);
                                            });
                                        });
                                    }
                                });
                                this.update(cx, |panel, _cx| {
                                    panel._folder_dialog_task = Some(task);
                                });
                            }
                        }),
                    ),
            )
            .child(div().h(px(1.)).bg(p.stroke))
            .child(primary_button(
                &p,
                "backend-save-local",
                if editing {
                    I18nKey::BackendSave.text()
                } else {
                    I18nKey::BackendAddTitle.text()
                },
                {
                    let name_input = self.name_input.clone();
                    let folder_input = self.folder_input.clone();
                    let edit_id = self.edit_id.clone();
                    let wm = self.window_manager.clone();
                    let this = this.clone();
                    move |window, cx| {
                        let name = name_input.read(cx).value().to_string();
                        if name.trim().is_empty() {
                            name_input.update(cx, |input, cx| {
                                input.focus_handle(cx).focus(window, cx);
                            });
                            wm.update(cx, |wm, cx| {
                                wm.show_warning_toast(I18nKey::BackendNameRequired.text(), cx);
                            });
                            return;
                        }
                        let folder = folder_input.read(cx).value().to_string();
                        if folder.trim().is_empty() {
                            return;
                        }
                        wm.update(cx, |wm, cx| {
                            if let Some(id) = edit_id.as_deref() {
                                wm.edit_backend(id, name, folder, cx);
                            } else {
                                wm.add_local_folder_backend(name, folder, cx);
                            }
                        });
                        this.update(cx, |panel, cx| panel.close(cx));
                    }
                },
            ))
            .into_any_element()
    }

    fn render_webdav_form(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let editing = self.edit_id.is_some();
        let p = Palette::current(cx);
        let this = cx.entity().clone();

        let test_color = if self.test_pending {
            p.ink
        } else if self.test_ok {
            p.ok
        } else if !self.test_error.is_empty() {
            p.danger
        } else {
            p.ink
        };
        let test_label = if self.test_pending {
            I18nKey::BackendTesting.text()
        } else if self.test_ok {
            I18nKey::BackendTestSuccess.text()
        } else if !self.test_error.is_empty() {
            I18nKey::BackendTestFailed.text()
        } else {
            I18nKey::BackendTest.text()
        };

        div()
            .flex()
            .flex_col()
            .gap(Space::Lg.scaled())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(Space::Xs.scaled())
                    .child(field_label(&p, I18nKey::BackendServerUrl.text()))
                    .child(input_box(&p, &self.url_input, window, cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(Space::Xs.scaled())
                    .child(field_label(&p, I18nKey::BackendWebdavPath.text()))
                    .child(input_box(&p, &self.webdav_path_input, window, cx)),
            )
            .child(
                div()
                    .flex()
                    .gap(Space::Md.scaled())
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(Space::Xs.scaled())
                            .child(field_label(&p, I18nKey::BackendUsername.text()))
                            .child(input_box(&p, &self.username_input, window, cx)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(Space::Xs.scaled())
                            .child(field_label(&p, I18nKey::BackendPassword.text()))
                            .child(input_box(&p, &self.password_input, window, cx)),
                    ),
            )
            .child(div().h(px(1.)).bg(p.stroke))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(Space::Sm.scaled())
                    .child(div().flex_1().min_w(px(0.)).child(primary_button(
                        &p,
                        "backend-save-webdav",
                        if editing {
                            I18nKey::BackendSave.text()
                        } else {
                            I18nKey::BackendAddTitle.text()
                        },
                        {
                            let this = this.clone();
                            let name_input = self.name_input.clone();
                            let url_input = self.url_input.clone();
                            let path_input = self.webdav_path_input.clone();
                            let username_input = self.username_input.clone();
                            let password_input = self.password_input.clone();
                            let edit_id = self.edit_id.clone();
                            let wm = self.window_manager.clone();
                            move |window, cx| {
                                let name = name_input.read(cx).value().to_string();
                                if name.trim().is_empty() {
                                    name_input.update(cx, |input, cx| {
                                        input.focus_handle(cx).focus(window, cx);
                                    });
                                    wm.update(cx, |wm, cx| {
                                        wm.show_warning_toast(
                                            I18nKey::BackendNameRequired.text(),
                                            cx,
                                        );
                                    });
                                    return;
                                }
                                let root_url = url_input.read(cx).value().to_string();
                                if root_url.trim().is_empty() {
                                    url_input.update(cx, |input, cx| {
                                        input.focus_handle(cx).focus(window, cx);
                                    });
                                    wm.update(cx, |wm, cx| {
                                        wm.show_warning_toast(
                                            I18nKey::BackendUrlRequired.text(),
                                            cx,
                                        );
                                    });
                                    return;
                                }
                                let path = path_input.read(cx).value().to_string();
                                let username = username_input.read(cx).value().to_string();
                                let password = password_input.read(cx).unmask_value().to_string();
                                wm.update(cx, |wm, cx| {
                                    let form = WebDavBackendForm {
                                        name,
                                        root_url,
                                        path,
                                        username,
                                        password,
                                    };
                                    if let Some(id) = edit_id.as_deref() {
                                        wm.edit_webdav_backend(id, form, cx);
                                    } else {
                                        wm.add_webdav_backend(form, cx);
                                    }
                                });
                                this.update(cx, |panel, cx| panel.close(cx));
                            }
                        },
                    )))
                    .child(
                        div().flex_1().min_w(px(0.)).child(
                            widgets::btn(&p, "backend-test", test_label, None, Tone::Plain)
                                .w_full()
                                .justify_center()
                                .text_color(test_color)
                                .inert(self.test_pending)
                                .on_click({
                                    let this = this.clone();
                                    move |_, _window, cx| {
                                        this.update(cx, |panel, cx| panel.start_webdav_test(cx));
                                    }
                                }),
                        ),
                    ),
            )
            .into_any_element()
    }
}

impl Render for AddBackendPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 语言切换时刷新 InputState placeholder
        let current = crate::core::i18n::lang_version();
        if self.last_lang_version != current {
            self.last_lang_version = current;
            self.name_input.update(cx, |state, cx| {
                state.set_placeholder(I18nKey::BackendPhName.text(), window, cx);
            });
            self.folder_input.update(cx, |state, cx| {
                state.set_placeholder(I18nKey::BackendPhFolder.text(), window, cx);
            });
            self.url_input.update(cx, |state, cx| {
                state.set_placeholder(I18nKey::BackendPhUrl.text(), window, cx);
            });
            self.webdav_path_input.update(cx, |state, cx| {
                state.set_placeholder(I18nKey::BackendPhWebdavPath.text(), window, cx);
            });
            self.username_input.update(cx, |state, cx| {
                state.set_placeholder(I18nKey::BackendPhUser.text(), window, cx);
            });
            self.password_input.update(cx, |state, cx| {
                state.set_placeholder(I18nKey::BackendPhPass.text(), window, cx);
            });
        }

        if !self.visible {
            return div().into_any_element();
        }

        let this = cx.entity().clone();
        let p = Palette::current(cx);
        let show_back = self.edit_id.is_none() && self.step != EditorStep::SelectType;
        let content = match self.step {
            EditorStep::SelectType => self.render_type_picker(cx),
            EditorStep::LocalFolder => self.render_local_form(window, cx),
            EditorStep::WebDav => self.render_webdav_form(window, cx),
        };
        let scale = if self
            .animation_started
            .is_some_and(|started| started.elapsed() <= motion::fast() + Duration::from_millis(24))
        {
            self.transition_f32(
                window,
                cx,
                ("backend-panel-scale", self.animation_generation),
                0.96,
                1.0,
            )
        } else {
            1.0
        };
        let header_icon =
            |glyph: &'static str| widgets::icon(glyph, IconSize::Md.scaled(), p.ink_2);

        crate::ui::components::modal_overlay::modal_overlay()
            .rounded(Radius::Lg.scaled())
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, {
                let this = this.clone();
                move |_ev, _window, cx| {
                    cx.stop_propagation();
                    this.update(cx, |panel, cx| panel.close(cx));
                }
            })
            .child(
                widgets::dialog_panel(&p)
                    .w(px(320. * scale))
                    .max_h(px(460. * scale))
                    .p(Space::Lg.scaled())
                    .gap(Space::Md.scaled())
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_ev, _window, cx| {
                        cx.stop_propagation();
                    })
                    .child(
                        div()
                            .h(px(28.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .items_center()
                                    .gap(Space::Sm.scaled())
                                    .when(show_back, |header| {
                                        header.child(
                                            div()
                                                .id("backend-back")
                                                .role(Role::Button)
                                                .tab_stop(true)
                                                .cursor(CursorStyle::PointingHand)
                                                .rounded(Radius::Md.scaled())
                                                .hover(move |style| style.bg(p.hover))
                                                .on_mouse_down(MouseButton::Left, {
                                                    let this = this.clone();
                                                    move |_ev, _window, cx| {
                                                        cx.stop_propagation();
                                                        this.update(cx, |panel, cx| {
                                                            panel.step = EditorStep::SelectType;
                                                            panel.reset_test();
                                                            cx.notify();
                                                        });
                                                    }
                                                })
                                                .child(header_icon("BackIcon")),
                                        )
                                    })
                                    .child(if self.step == EditorStep::SelectType {
                                        div()
                                            .text_size(typography::font(type_role::BASE))
                                            .font_weight(typography::LABEL_WEIGHT)
                                            .text_color(p.ink)
                                            .child(self.title())
                                            .into_any_element()
                                    } else {
                                        header_name_input(&self.name_input, &p)
                                    }),
                            )
                            .child(
                                div()
                                    .id("backend-close")
                                    .role(Role::Button)
                                    .tab_stop(true)
                                    .cursor(CursorStyle::PointingHand)
                                    .rounded(Radius::Md.scaled())
                                    .hover(move |style| style.bg(p.hover))
                                    .on_mouse_down(MouseButton::Left, {
                                        let this = this.clone();
                                        move |_ev, _window, cx| {
                                            cx.stop_propagation();
                                            this.update(cx, |panel, cx| panel.close(cx));
                                        }
                                    })
                                    .child(header_icon("CloseIcon")),
                            ),
                    )
                    .child(div().h(px(1.)).bg(p.stroke))
                    .child(div().max_h(px(380.)).overflow_y_scrollbar().child(content)),
            )
            .into_any_element()
    }
}

fn header_name_input(input: &Entity<InputState>, p: &Palette) -> AnyElement {
    div()
        .flex_1()
        .min_w(px(0.))
        .h(px(26.))
        .flex()
        .items_center()
        .child(
            Input::new(input)
                .appearance(false)
                .bordered(false)
                .focus_bordered(false)
                .w_full()
                .h(px(20.))
                .text_size(typography::font(type_role::BASE))
                .font_weight(typography::LABEL_WEIGHT)
                .text_color(p.ink),
        )
        .into_any_element()
}

fn field_label(p: &Palette, label: &'static str) -> impl IntoElement {
    let p = *p;
    div()
        .text_size(typography::font(type_role::CAPTION))
        .font_weight(typography::LABEL_WEIGHT)
        .text_color(p.ink_2)
        .child(label)
}

fn input_box(p: &Palette, input: &Entity<InputState>, window: &Window, cx: &App) -> AnyElement {
    widgets::text_field(p, input, "", true, window, cx).into_any_element()
}

fn type_card(
    p: &Palette,
    title: &'static str,
    description: &'static str,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let p = *p;
    div()
        .flex_1()
        .min_w(px(0.))
        .h(px(84.))
        .rounded(Radius::Lg.scaled())
        .bg(p.layer_strong)
        .border_1()
        .border_color(p.stroke)
        .p(Space::Lg.scaled())
        .flex()
        .flex_col()
        .justify_center()
        .gap(Space::Xs.scaled())
        .cursor(CursorStyle::PointingHand)
        .hover(move |style| style.border_color(p.accent))
        .on_mouse_down(MouseButton::Left, move |_ev, window, cx| {
            on_click(window, cx);
        })
        .child(
            div()
                .text_size(typography::font(type_role::BASE))
                .font_weight(typography::LABEL_WEIGHT)
                .text_color(p.ink)
                .child(title),
        )
        .child(
            div()
                .text_size(typography::font(type_role::CAPTION))
                .text_color(p.ink_3)
                .child(description),
        )
}

fn primary_button(
    p: &Palette,
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    widgets::btn(p, id, label, None, Tone::Accent)
        .w_full()
        .justify_center()
        .on_click(move |_, window, cx| on_click(window, cx))
}
