//! --- Sync settings tab. ---
//!
//! 页面层只取语义色（`Palette`）与控件工厂：分组卡、行、开关、下拉、
//! 状态胶囊、分段控件都不在这里拼样式（对齐 Argus `pages/settings.rs`）。

use std::time::Duration;

use gpui::prelude::*;
use gpui::*;
use gpui_kit::component::scroll::ScrollableElement;

use crate::core::i18n_keys::I18nKey;
use crate::services::gpui_sync::format_last_sync;
use crate::state::sync::BackendStatus;
use crate::ui::components::wait_switch::WaitToggleSwitch;
use crate::ui::components::wait_tabs::WaitTabs;
use crate::ui::design::motion;
use crate::ui::design::palette::Palette;
use crate::ui::design::size::{Radius, Space};
use crate::ui::design::typography::{self, type_role};
use crate::ui::transition::keyed_transition;
use crate::ui::widgets::{self, ButtonExt, PillTone};

use super::{BackendCollapseState, SettingsPanel};

/// 后端卡片的高度动画时长：沿用动效令牌（250 ms），不再自定 300。
fn collapse_duration() -> Duration {
    motion::slow()
}

impl SettingsPanel {
    pub fn render_sync_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = self.palette;
        let sync = self.state.read(cx).sync.clone();
        let wm = self.window_manager.clone();
        let backend_panel = self.backend_panel();
        let transfer_enabled = self.state.read(cx).settings.transfer_station_enabled;
        let transfer_retention = self.state.read(cx).settings.transfer_retention_days;
        let enabled_backend_count = sync
            .backends
            .iter()
            .filter(|backend| backend.config.enabled)
            .count();

        let backend_cards: Vec<AnyElement> = sync
            .backends
            .iter()
            .map(|backend| {
                self.render_backend_card(backend, window, cx)
                    .into_any_element()
            })
            .collect();

        let mut container = div()
            .flex()
            .flex_col()
            .gap(Space::Sm.scaled())
            .pt(Space::Md.scaled());

        // --- 同步总开关 ---
        let mut master_rows = vec![toggle_row(
            &p,
            "sync-auto-enabled",
            I18nKey::SyncTabTitle.text(),
            I18nKey::DescSyncAuto.text(),
            sync.auto_enabled,
            {
                let wm = wm.clone();
                move |_window, cx| {
                    wm.update(cx, |wm, cx| wm.toggle_sync_auto_enabled(cx));
                }
            },
        )];
        if sync.auto_enabled {
            master_rows.push(toggle_row(
                &p,
                "sync-favorites-only",
                I18nKey::SyncFavoritesOnly.text(),
                I18nKey::DescSyncFavorites.text(),
                sync.favorites_only,
                {
                    let wm = wm.clone();
                    move |_window, cx| {
                        wm.update(cx, |wm, cx| wm.toggle_sync_favorites_only(cx));
                    }
                },
            ));
        }
        container =
            container.child(self.settings_group(I18nKey::GroupSyncMaster.text(), master_rows));

        // --- 同步内容（仅在同步开启时出现） ---
        if sync.auto_enabled {
            let mut option_rows = vec![toggle_row(
                &p,
                "sync-include-images",
                I18nKey::SyncIncludeImages.text(),
                I18nKey::DescSyncIncludeImages.text(),
                sync.include_images,
                {
                    let wm = wm.clone();
                    move |_window, cx| {
                        wm.update(cx, |wm, cx| wm.toggle_sync_include_images(cx));
                    }
                },
            )];
            if sync.include_images {
                option_rows.push(toggle_row(
                    &p,
                    "sync-compress-images",
                    I18nKey::SyncCompressImages.text(),
                    I18nKey::DescSyncCompressImages.text(),
                    sync.compress_images,
                    {
                        let wm = wm.clone();
                        move |_window, cx| {
                            wm.update(cx, |wm, cx| wm.toggle_sync_compress_images(cx));
                        }
                    },
                ));
            }
            container =
                container.child(self.settings_group(I18nKey::GroupSyncOptions.text(), option_rows));
        }

        // --- 文件中转站 ---
        let mut transfer_rows = vec![toggle_row(
            &p,
            "transfer-station-enabled",
            I18nKey::TransferStation.text(),
            if enabled_backend_count == 0 {
                I18nKey::TransferNoBackend.text()
            } else {
                I18nKey::DescTransferStation.text()
            },
            transfer_enabled,
            {
                let wm = wm.clone();
                move |_window, cx| {
                    wm.update(cx, |wm, cx| wm.toggle_transfer_station(cx));
                }
            },
        )];
        if transfer_enabled {
            transfer_rows.push(self.render_retention_row(transfer_retention, window, cx));
        }
        container =
            container.child(self.settings_group(I18nKey::GroupTransfer.text(), transfer_rows));

        // --- 配置同步 ---
        container = container.child(self.render_config_sync_group(window, cx));

        // --- 同步后端 ---
        let mut backend_rows: Vec<AnyElement> = vec![div()
            .px(Space::Sm.scaled())
            .py(Space::Sm.scaled())
            .child(
                widgets::btn(
                    &p,
                    "sync-add-backend",
                    I18nKey::SyncAddBackend.text(),
                    Some("PlusIcon"),
                    widgets::Tone::Plain,
                )
                .w_full()
                .justify_center()
                .inert_dimmed(!sync.auto_enabled)
                .on_click(move |_, window, cx| {
                    backend_panel.update(cx, |panel, cx| panel.open_add(window, cx));
                }),
            )
            .into_any_element()];
        if !backend_cards.is_empty() {
            backend_rows.push(
                div()
                    .max_h(px(320.))
                    .overflow_y_scrollbar()
                    .px(Space::Sm.scaled())
                    .pb(Space::Sm.scaled())
                    .flex()
                    .flex_col()
                    .children(backend_cards)
                    .into_any_element(),
            );
        }
        container =
            container.child(self.settings_group(I18nKey::GroupBackends.text(), backend_rows));

        container
    }

    /// 中转站的自动过期：0/1/3/7/30 天，分段控件（滑块 250 ms 滑移）。
    fn render_retention_row(
        &self,
        retention: u32,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let wm = self.window_manager.clone();
        let days = [0u32, 1, 3, 7, 30];
        let labels: Vec<SharedString> = days
            .iter()
            .map(|days| SharedString::from(retention_label(*days)))
            .collect();
        let selected = days.iter().position(|days| *days == retention).unwrap_or(0);
        let tabs = WaitTabs::new("transfer-retention", labels)
            .selected(selected)
            .on_select(move |index, _window, cx| {
                let days = days[index.min(days.len() - 1)];
                wm.update(cx, |wm, cx| wm.set_transfer_retention_days(days, cx));
            });
        self.row_shell()
            .child(self.row_text(
                I18nKey::TransferRetention.text(),
                I18nKey::DescTransferRetention.text(),
            ))
            .child(div().w(px(220.)).flex_shrink_0().child(tabs))
            .into_any_element()
    }

    /// 配置同步：标题行 + 「后端下拉 + 上传 + 应用」一行。
    fn render_config_sync_group(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let wm = self.window_manager.clone();
        let this = cx.entity().clone();
        let sync = self.state.read(cx).sync.clone();
        let config_backends: Vec<BackendStatus> = sync
            .backends
            .iter()
            .filter(|backend| {
                backend.config.backend_type == "local_folder"
                    || backend.config.backend_type == "webdav"
            })
            .cloned()
            .collect();

        if self.config_sync_backend_id.is_none() {
            self.config_sync_backend_id = config_backends.first().map(|b| b.config.id.clone());
        }
        // 每帧复核：选中的后端被删掉时回落到第一个，按钮不会操作陈旧 ID。
        let selected_still_exists = self
            .config_sync_backend_id
            .as_ref()
            .is_some_and(|id| config_backends.iter().any(|b| &b.config.id == id));
        if !selected_still_exists {
            self.config_sync_backend_id = config_backends.first().map(|b| b.config.id.clone());
        }

        let selected_id = self.config_sync_backend_id.clone();
        let no_backends = config_backends.is_empty();
        let busy = wm.read(cx).is_config_sync_busy();
        let buttons_disabled = no_backends || busy;
        let names: Vec<String> = config_backends
            .iter()
            .map(|backend| backend.config.name.clone())
            .collect();
        let selected_index = config_backends
            .iter()
            .position(|backend| Some(&backend.config.id) == selected_id.as_ref());
        let selected_name = selected_index
            .and_then(|index| names.get(index))
            .cloned()
            .unwrap_or_default();
        let open = self.config_sync_menu_open && names.len() > 1;

        // 没有可用后端时，把提示语当成唯一（禁用）选项：按钮里只有一段文字，
        // 会走和正常选项一样的省略号裁切，不再在按钮末尾另挂一个节点把宽度
        // 撑爆（旧写法会与 chevron 叠在一起）。
        let (select_options, select_selected) = if no_backends {
            (
                vec![I18nKey::ConfigSyncNoBackend.text().to_string()],
                Some(0),
            )
        } else {
            (names.clone(), selected_index)
        };

        let select = widgets::select(
            &p,
            "config-sync-backend",
            I18nKey::ConfigSyncTargetBackend.text(),
            &select_options,
            select_selected,
            open,
            no_backends,
            {
                let this = this.clone();
                move |_window, cx| {
                    this.update(cx, |panel, cx| {
                        panel.config_sync_menu_open = !panel.config_sync_menu_open;
                        cx.notify();
                    });
                }
            },
        );

        let mut select_area = div()
            .relative()
            .w_full()
            // 占满一行：内部内容列已 `min_w(0)` + 省略号，窄窗口下按容器裁切。
            .child(select.w_full().min_w(px(0.)));
        if open {
            select_area = select_area.child(
                deferred(div().absolute().top(px(38.)).left_0().w_full().child(
                    widgets::select_list(&p, "config-sync-backend-list", &names, selected_index, {
                        let this = this.clone();
                        move |index, _window, cx| {
                            let id = config_backends
                                .get(index)
                                .map(|backend| backend.config.id.clone());
                            this.update(cx, |panel, cx| {
                                panel.config_sync_backend_id = id;
                                panel.config_sync_menu_open = false;
                                cx.notify();
                            });
                        }
                    }),
                ))
                .with_priority(1),
            );
        }

        let upload = {
            let this = this.clone();
            let backend_name = selected_name.clone();
            widgets::btn(
                &p,
                "config-sync-upload",
                I18nKey::ConfigSyncUpload.text(),
                Some("UploadIcon"),
                widgets::Tone::Accent,
            )
            .flex_1()
            .justify_center()
            .inert_dimmed(buttons_disabled)
            .on_click(move |_, _window, cx| {
                if buttons_disabled {
                    return;
                }
                this.update(cx, |panel, cx| {
                    panel.config_sync_upload_confirm = Some(backend_name.clone());
                    panel.config_sync_upload_confirm_gen =
                        panel.config_sync_upload_confirm_gen.wrapping_add(1);
                    cx.notify();
                });
            })
        };

        let apply = {
            let this = this.clone();
            let wm = wm.clone();
            widgets::btn(
                &p,
                "config-sync-download",
                I18nKey::ConfigSyncApply.text(),
                Some("DownloadIcon"),
                widgets::Tone::Plain,
            )
            .flex_1()
            .justify_center()
            .inert_dimmed(buttons_disabled)
            .on_click(move |_, _window, cx| {
                if buttons_disabled {
                    return;
                }
                let Some(id) = selected_id.as_ref() else {
                    return;
                };
                let backends = this.read(cx).state.read(cx).settings.sync_backends.clone();
                if let Some(config) = backends.iter().find(|backend| &backend.id == id) {
                    if let Some(backend) =
                        crate::services::backends::create_config_snapshot_backend(config)
                    {
                        wm.update(cx, |wm, _cx| wm.start_config_download(backend));
                    }
                }
                this.update(cx, |_panel, cx| cx.notify());
            })
        };

        let card = div()
            .rounded(Radius::Card.scaled())
            .bg(p.layer)
            .border_1()
            .border_color(p.stroke)
            .flex()
            .flex_col()
            .child(self.row_header(
                I18nKey::ConfigSyncTitle.text(),
                I18nKey::ConfigSyncDesc.text(),
            ))
            .child(div().h(px(1.)).bg(p.stroke))
            .child(
                // 两行：后端下拉独占一行，上传/应用平分第二行。三个控件挤一行
                // 时按钮文字会被卡片边界裁掉（下拉的最小宽度 + 两个按钮的最小
                // 内容宽度之和超过可用宽度）。
                div()
                    .px(Space::Lg.scaled())
                    .py(Space::Md.scaled())
                    .flex()
                    .flex_col()
                    .gap(Space::Sm.scaled())
                    .child(select_area)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(Space::Sm.scaled())
                            .child(upload)
                            .child(apply),
                    ),
            );

        self.settings_group(
            I18nKey::GroupConfigSync.text(),
            vec![card.into_any_element()],
        )
        .into_any_element()
    }

    fn render_backend_card(
        &mut self,
        backend: &BackendStatus,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = self.palette;
        let id = backend.config.id.clone();
        let enabled = backend.config.enabled;
        let (transfer_station_enabled, transfer_selected) = {
            let settings = &self.state.read(cx).settings;
            let selected = settings
                .sync_backends
                .iter()
                .find(|config| {
                    config.enabled
                        && !settings.transfer_backend_id.is_empty()
                        && config.id == settings.transfer_backend_id
                })
                .or_else(|| settings.sync_backends.iter().find(|config| config.enabled))
                .is_some_and(|config| config.id == id);
            (settings.transfer_station_enabled, selected)
        };
        let (previous, generation, changed) = match self.backend_collapse_states.get_mut(&id) {
            Some(state) => {
                let previous = state.enabled;
                let changed = previous != enabled;
                if changed {
                    state.enabled = enabled;
                    state.generation = state.generation.wrapping_add(1);
                }
                (previous, state.generation, changed)
            }
            None => {
                self.backend_collapse_states.insert(
                    id.clone(),
                    BackendCollapseState {
                        enabled,
                        generation: 0,
                    },
                );
                (enabled, 0, false)
            }
        };
        let key = hash_key(&id).wrapping_add(generation << 32);
        let height = transition_f32(
            window,
            cx,
            ("sync-backend-height", key),
            if changed {
                card_height(previous)
            } else {
                card_height(enabled)
            },
            card_height(enabled),
        );
        let footer_opacity = transition_f32(
            window,
            cx,
            ("sync-backend-footer-opacity", key),
            if changed {
                bool_f32(previous)
            } else {
                bool_f32(enabled)
            },
            bool_f32(enabled),
        );
        let content_opacity = transition_f32(
            window,
            cx,
            ("sync-backend-content-opacity", key),
            if changed {
                main_opacity(previous)
            } else {
                main_opacity(enabled)
            },
            main_opacity(enabled),
        );

        let auto_enabled = self.state.read(cx).sync.auto_enabled;
        let wm = self.window_manager.clone();
        let backend_panel = self.backend_panel();
        let (tone, status_text) = if backend.syncing {
            (PillTone::Busy, I18nKey::SyncSyncing.text())
        } else {
            match backend.status.as_str() {
                "online" => (PillTone::Ok, I18nKey::SyncStatusOnline.text()),
                "error" => (PillTone::Warn, I18nKey::SyncStatusError.text()),
                _ => (PillTone::Idle, I18nKey::SyncStatusOffline.text()),
            }
        };
        let interval = backend.effective_sync_interval_secs;
        let stats = format!(
            "{} · {} {} · {} {}",
            format_last_sync(&backend.config.last_sync_at),
            backend.config.last_item_count,
            I18nKey::SyncStatsItems.text(),
            backend.config.last_tag_count,
            I18nKey::SyncStatsTags.text()
        );
        let this = cx.entity().clone();

        let mut transfer_button = widgets::tool_icon_btn(
            &p,
            ("sync-transfer", key),
            "CloudIcon",
            if transfer_selected { p.accent } else { p.ink_3 },
            if transfer_selected {
                I18nKey::BackendTooltipTransferActive.text()
            } else {
                I18nKey::BackendTooltipSetTransfer.text()
            },
            !(auto_enabled && enabled && transfer_station_enabled),
            {
                let id = id.clone();
                let wm = wm.clone();
                move |_window, cx| {
                    wm.update(cx, |wm, cx| wm.set_transfer_backend(&id, cx));
                }
            },
        );
        // 中转开关未开时提示先开启，而不是留一个无反应的按钮。
        if !transfer_station_enabled {
            transfer_button = widgets::tool_icon_btn(
                &p,
                ("sync-transfer", key),
                "CloudIcon",
                p.ink_3,
                I18nKey::BackendTooltipEnableTransfer.text(),
                true,
                |_, _| {},
            );
        }

        let edit_button = widgets::tool_icon_btn(
            &p,
            ("sync-edit", key),
            "EditIcon",
            p.ink_3,
            I18nKey::BackendTooltipEdit.text(),
            !auto_enabled,
            {
                let config = backend.config.clone();
                let backend_panel = backend_panel.clone();
                move |window, cx| {
                    backend_panel.update(cx, |panel, cx| panel.open_edit(&config, window, cx));
                }
            },
        );
        let delete_button = widgets::tool_icon_btn(
            &p,
            ("sync-delete", key),
            "DeleteIcon",
            p.danger,
            I18nKey::BackendTooltipDelete.text(),
            !auto_enabled,
            {
                let id = id.clone();
                let this = this.clone();
                move |_window, cx| {
                    this.update(cx, |panel, cx| {
                        panel.delete_backend_confirm = Some(id.clone());
                        panel.delete_backend_confirm_gen =
                            panel.delete_backend_confirm_gen.wrapping_add(1);
                        panel.delete_backend_confirm_started = Some(std::time::Instant::now());
                        cx.notify();
                    });
                }
            },
        );

        let toggle = WaitToggleSwitch::new(format!("sync-backend-{id}"), id.clone())
            .value(enabled)
            .disabled(!auto_enabled)
            .on_change({
                let id = id.clone();
                let wm = wm.clone();
                move |_window, cx| {
                    wm.update(cx, |wm, cx| wm.toggle_sync_backend(&id, cx));
                }
            });

        // --- 同步间隔：分段控件 ---
        let intervals = [
            (30_u64, I18nKey::SyncInterval30s.text()),
            (60, I18nKey::SyncInterval1m.text()),
            (600, I18nKey::SyncInterval10m.text()),
            (1800, I18nKey::SyncInterval30m.text()),
        ];
        let selected = intervals
            .iter()
            .position(|(secs, _)| *secs == interval)
            .unwrap_or(0);
        let labels: Vec<SharedString> = intervals
            .iter()
            .map(|(_, label)| SharedString::from(*label))
            .collect();
        let tabs = WaitTabs::new(format!("sync-backend-interval-{id}"), labels)
            .selected(selected)
            .on_select({
                let id = id.clone();
                let wm = wm.clone();
                move |index, _window, cx| {
                    let secs = intervals[index.min(intervals.len() - 1)].0;
                    wm.update(cx, |wm, cx| wm.set_backend_sync_interval(&id, secs, cx));
                }
            });

        div()
            .mb(Space::Md.scaled())
            .h(px(height))
            .rounded(Radius::Lg.scaled())
            .border_1()
            .border_color(p.stroke)
            .bg(p.layer)
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(64.))
                    .flex_shrink_0()
                    .px(Space::Lg.scaled())
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(Space::Md.scaled())
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .opacity(content_opacity)
                            .flex()
                            .flex_col()
                            .gap(Space::Sm.scaled())
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(Space::Md.scaled())
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .whitespace_nowrap()
                                            .text_size(typography::font(type_role::BASE))
                                            .font_weight(typography::LABEL_WEIGHT)
                                            .text_color(p.ink)
                                            .child(backend.config.name.clone()),
                                    )
                                    .child(widgets::state_pill(
                                        &p,
                                        ("backend-status", key),
                                        status_text,
                                        tone,
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(Space::Sm.scaled())
                                    .child(
                                        div()
                                            .h(px(18.))
                                            .max_w(px(150.))
                                            .px(Space::Sm.scaled())
                                            .rounded(Radius::Sm.scaled())
                                            .bg(p.tag_bg)
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .whitespace_nowrap()
                                            .text_size(typography::font(type_role::CAPTION))
                                            .text_color(p.tag_ink)
                                            .flex()
                                            .items_center()
                                            .child(backend.service_label.clone()),
                                    )
                                    .child(
                                        div()
                                            .min_w(px(0.))
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .whitespace_nowrap()
                                            .text_size(typography::font(type_role::CAPTION))
                                            .text_color(p.ink_3)
                                            .child(stats),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .opacity(content_opacity)
                            .flex()
                            .items_center()
                            .gap(Space::Xs.scaled())
                            .child(transfer_button)
                            .child(edit_button)
                            .child(delete_button)
                            .child(
                                div()
                                    .when(!auto_enabled, |toggle| toggle.opacity(0.45))
                                    .child(toggle),
                            ),
                    ),
            )
            .child(
                div()
                    .h(px(40.))
                    .flex_shrink_0()
                    .opacity(footer_opacity)
                    .border_t(px(1.))
                    .border_color(p.stroke)
                    .px(Space::Md.scaled())
                    .flex()
                    .items_center()
                    .gap(Space::Sm.scaled())
                    .child(div().flex_1().min_w(px(0.)).child(tabs))
                    .child(
                        widgets::btn_small(
                            &p,
                            ("sync-now", key),
                            if backend.syncing {
                                I18nKey::SyncSyncing.text()
                            } else {
                                I18nKey::SyncNow.text()
                            },
                            None,
                            widgets::Tone::Accent,
                        )
                        .inert(backend.syncing)
                        .on_click({
                            let id = id.clone();
                            let wm = wm.clone();
                            move |_, _window, cx| {
                                wm.update(cx, |wm, cx| wm.sync_backend_now(&id, cx));
                            }
                        }),
                    ),
            )
    }
}

/// 一行「标题 + 说明 + 开关」；设置页所有布尔项共用这一形状。
fn toggle_row(
    p: &Palette,
    id: &'static str,
    label: &str,
    desc: &str,
    value: bool,
    on_change: impl Fn(&mut Window, &mut App) + 'static,
) -> AnyElement {
    let control = WaitToggleSwitch::new(id, label.to_owned())
        .value(value)
        .on_change(on_change);
    widgets::settings_row(p, label, desc, control).into_any_element()
}

fn retention_label(days: u32) -> String {
    if days == 0 {
        I18nKey::TransferKeepForever.text().to_string()
    } else {
        I18nKey::TransferRetentionDays.fmt(&[&days.to_string()])
    }
}

fn transition_f32(
    window: &mut Window,
    cx: &mut App,
    key: (&'static str, u64),
    initial: f32,
    target: f32,
) -> f32 {
    keyed_transition(
        window,
        cx,
        key,
        collapse_duration(),
        initial,
        motion::ease_out,
        target,
    )
}

fn hash_key(value: &str) -> u64 {
    value.bytes().fold(0u64, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(byte as u64)
    })
}

fn card_height(enabled: bool) -> f32 {
    if enabled {
        105.0
    } else {
        64.0
    }
}

fn bool_f32(value: bool) -> f32 {
    if value {
        1.0
    } else {
        0.0
    }
}

fn main_opacity(enabled: bool) -> f32 {
    if enabled {
        1.0
    } else {
        0.45
    }
}
