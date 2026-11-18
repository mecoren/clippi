//! Shared tag navigation, rendered in an attached native window on Windows/macOS.
//!
//! Tag behavior:
//! --- - width 56px, rows placed every 24px; ---
//! --- - checked tags slide to x=0 and are fully opaque; ---
//! --- - unchecked pinned tags remain visible with dimmed text; ---
//! --- - unchecked, unpinned tags slide slightly right and fade out; ---
//! --- - left click toggles a visible tag filter, right click toggles pin. ---

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::core::types::TagInfo;
use crate::state::app::AppState;

use super::clipboard_list::ClipboardListView;
use super::components::reorder::{ease_in_out, row_drag_position};
use super::design::motion;
use super::design::palette::Palette;
use super::design::size::{Radius, Space};
use super::design::typography::{self, type_role};
use crate::ui::transition::keyed_transition;

/// Sidebar top offset (must match `top(px(65.))` in root.rs).
/// Aligned with type filter bar top: titlebar(30) + pt(1) + search(28) + mb(6) = 65.
const SIDEBAR_TOP_OFFSET: f32 = 65.0;
/// Height of a single sidebar row (22px row + 2px gap).
const ROW_HEIGHT: f32 = 24.0;
/// Tag chip width; the window is this wide plus the shadow margin.
const SIDEBAR_WIDTH: f32 = 56.0;
/// Bottom padding to keep the last row from touching the window edge.
const SIDEBAR_BOTTOM_PADDING: f32 = 8.0;
/// Transparent margin around the strip inside its native window, sized for the
/// per-tag shadows. The inline (Linux) layout keeps no margin.
#[cfg(any(target_os = "windows", target_os = "macos"))]
const TAG_MARGIN: f32 = crate::platform::tag_window::SHADOW;
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const TAG_MARGIN: f32 = 0.;

/// A reorder preview is local to the sidebar until the button is released.
struct SidebarDrag {
    id: i64,
    ids: Vec<i64>,
    start: Point<Pixels>,
    grab_y: f32,
    top: f32,
    target: usize,
    moved: bool,
}

fn drag_position(pointer_y: f32, grab_y: f32, count: usize) -> (f32, usize) {
    row_drag_position(pointer_y, grab_y, count, ROW_HEIGHT)
}

#[derive(Default)]
struct TooltipHover {
    tag_id: Option<i64>,
}

impl TooltipHover {
    fn update(&mut self, tag_id: i64, hovered: bool) -> bool {
        if hovered {
            if self.tag_id == Some(tag_id) {
                return false;
            }
            self.tag_id = Some(tag_id);
            true
        } else if self.tag_id == Some(tag_id) {
            self.clear()
        } else {
            false
        }
    }

    fn clear(&mut self) -> bool {
        self.tag_id.take().is_some()
    }
}

/// Whether a redraw of the tag window means the pointer left it while a tooltip
/// was pending or shown.
///
/// GPUI 0.2.2's Windows backend handles `WM_MOUSELEAVE` by clearing the window
/// hover flag and scheduling a redraw; unlike macOS it never dispatches
/// `MouseExitEvent`. Without an event the hovered row never receives its
/// `hover(false)` callback when the pointer crosses into the main window, which
/// left the tooltip stuck on screen. The flag change does dirty this window, so
/// the redraw that follows is the platform signal to clear here.
#[cfg(target_os = "windows")]
fn tooltip_orphaned_on_window_exit(hovered_tag: Option<i64>, window_hovered: bool) -> bool {
    hovered_tag.is_some() && !window_hovered
}

/// Sidebar entity for displaying and managing tags.
pub enum SidebarEvent {
    Tooltip(Option<(String, f32)>),
}

impl EventEmitter<SidebarEvent> for Sidebar {}

pub struct Sidebar {
    tooltip_hover: TooltipHover,
    tooltip_task: Option<Task<()>>,
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    attachment: Option<Rc<crate::platform::tag_window::Attachment>>,
    state: Entity<AppState>,
    list_view: Entity<ClipboardListView>,
    rendered_tag_ids: Vec<i64>,
    unchecked_unpinned_since: HashMap<i64, Instant>,
    transition_generations: HashMap<i64, u64>,
    drag: Option<SidebarDrag>,
    settling: Option<(i64, f32, Instant)>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl Sidebar {
    pub fn new(state: Entity<AppState>, list_view: Entity<ClipboardListView>) -> Self {
        Self {
            tooltip_hover: TooltipHover::default(),
            tooltip_task: None,
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            attachment: None,
            state,
            list_view,
            rendered_tag_ids: Vec::new(),
            unchecked_unpinned_since: HashMap::new(),
            transition_generations: HashMap::new(),
            drag: None,
            settling: None,
            bounds: Rc::new(Cell::new(Bounds::default())),
        }
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    pub fn attach_window(
        &mut self,
        attachment: Rc<crate::platform::tag_window::Attachment>,
        cx: &mut Context<Self>,
    ) {
        self.attachment = Some(attachment);
        self.refresh_window(cx);
    }

    pub fn refresh_window(&self, cx: &mut Context<Self>) {
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        if let Some(attachment) = &self.attachment {
            let state = self.state.read(cx);
            let count = state
                .tags
                .iter()
                .filter(|tag| {
                    state.filters.tag_ids.contains(&tag.id)
                        || state.settings.pinned_tag_ids.contains(&tag.id)
                })
                .count();
            // Keep the surface alive until exiting rows finish their animation.
            attachment.set_rows(count.max(self.rendered_tag_ids.len()));
        }
        cx.notify();
    }

    fn hover_tooltip(
        &mut self,
        tag_id: i64,
        hovered: bool,
        label: &str,
        y: f32,
        cx: &mut Context<Self>,
    ) {
        if self.drag.is_some() || !self.tooltip_hover.update(tag_id, hovered) {
            return;
        }
        self.tooltip_task = None;
        cx.emit(SidebarEvent::Tooltip(None));
        if hovered {
            let label = label.to_owned();
            self.tooltip_task = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                let _ = this.update(cx, |sidebar, cx| {
                    if sidebar.tooltip_hover.tag_id == Some(tag_id) {
                        cx.emit(SidebarEvent::Tooltip(Some((label, y))));
                    }
                });
            }));
        }
    }

    fn clear_tooltip(&mut self, cx: &mut Context<Self>) {
        self.tooltip_task = None;
        if self.tooltip_hover.clear() {
            cx.emit(SidebarEvent::Tooltip(None));
        }
    }

    pub fn cancel_drag(&mut self, cx: &mut Context<Self>) -> bool {
        self.clear_tooltip(cx);
        let drag = self.drag.take();
        let cancelled = drag.is_some();
        if let Some(drag) = drag.filter(|drag| drag.moved) {
            self.settling = Some((drag.id, drag.top, Instant::now()));
        }
        if cancelled {
            cx.notify();
        }
        cancelled
    }

    fn move_drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        if !drag.moved && (position - drag.start).magnitude() <= 4. {
            return;
        }
        drag.moved = true;
        (drag.top, drag.target) = drag_position(
            f32::from(position.y - self.bounds.get().top()),
            drag.grab_y,
            drag.ids.len(),
        );
        cx.notify();
    }

    fn finish_drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        // Include the final pointer position even when no last move event arrived.
        self.move_drag(position, cx);
        let Some(drag) = self.drag.take() else {
            return;
        };
        if drag.moved {
            self.settling = Some((drag.id, drag.top, Instant::now()));
            if let Some(source) = drag.ids.iter().position(|id| *id == drag.id) {
                self.state.update(cx, |state, cx| {
                    state.reorder_tag(drag.id, drag.ids[drag.target], source < drag.target);
                    cx.notify();
                });
            }
        } else if self.bounds.get().contains(&position) {
            let items = self.state.update(cx, |state, cx| {
                state.toggle_tag_filter(drag.id);
                cx.notify();
                state.visible_items()
            });
            self.list_view
                .update(cx, |list, cx| list.set_items(items, cx));
        }
        cx.notify();
    }

    /// 主题变化（设置页切换浅/暗）：取色每帧走 `Palette`，这里只需重绘。
    pub fn set_theme(&mut self, cx: &mut Context<Self>) {
        cx.notify();
    }
}

impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        super::font::sync_window_rem_size(window);
        let (tags, active_tag_ids, pinned_tag_ids) = {
            let app_state = self.state.read(cx);
            (
                app_state.tags.clone(),
                app_state.filters.tag_ids.clone(),
                app_state.settings.pinned_tag_ids.clone(),
            )
        };

        let now = Instant::now();
        let active_or_pinned_ids: Vec<i64> = tags
            .iter()
            .filter(|tag| active_tag_ids.contains(&tag.id) || pinned_tag_ids.contains(&tag.id))
            .map(|tag| tag.id)
            .collect();

        let previous_rendered_ids = self.rendered_tag_ids.clone();
        for previous_id in previous_rendered_ids.clone() {
            if !active_or_pinned_ids.contains(&previous_id) {
                self.unchecked_unpinned_since
                    .entry(previous_id)
                    .or_insert(now);
            }
        }
        // 下架行继续参与动画的窗口：慢速档（250 ms），与滑出时长同源。
        self.unchecked_unpinned_since.retain(|id, started_at| {
            !active_or_pinned_ids.contains(id) && now.duration_since(*started_at) < motion::slow()
        });

        if !self.unchecked_unpinned_since.is_empty() {
            window.request_animation_frame();
        }
        let mut display_tags = ordered_sidebar_tags(
            &tags,
            &active_tag_ids,
            &pinned_tag_ids,
            &self.unchecked_unpinned_since,
        );
        for tag in &display_tags {
            let newly_entering = active_tag_ids.contains(&tag.id)
                && !pinned_tag_ids.contains(&tag.id)
                && !previous_rendered_ids.contains(&tag.id);
            if newly_entering {
                *self.transition_generations.entry(tag.id).or_insert(0) += 1;
            }
        }
        let transition_generations = self.transition_generations.clone();
        self.rendered_tag_ids = display_tags.iter().map(|tag| tag.id).collect();

        // ── Collapse tags that don't fit in the available vertical space ──
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        let main_height = self
            .attachment
            .as_ref()
            .map_or(480., |attachment| attachment.available_height());
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let main_height = f32::from(window.viewport_size().height);
        let available_height = (main_height - SIDEBAR_TOP_OFFSET - SIDEBAR_BOTTOM_PADDING).max(0.);
        let max_visible = (available_height / ROW_HEIGHT).floor() as usize;

        #[cfg(any(target_os = "windows", target_os = "macos"))]
        if let Some(attachment) = &self.attachment {
            attachment.set_rows(display_tags.len());
        }
        let hidden_count = if display_tags.len() > max_visible {
            display_tags
                .len()
                .saturating_sub(max_visible.saturating_sub(1))
        } else {
            0
        };
        if hidden_count > 0 {
            // Keep last slot for the "+N" overflow indicator row.
            let overflow_start = max_visible.saturating_sub(1);
            let overflow_tags: Vec<_> = display_tags.drain(overflow_start..).collect();
            // Clean up animation state for tags that are now invisible.
            for tag in &overflow_tags {
                self.rendered_tag_ids.retain(|id| *id != tag.id);
                self.unchecked_unpinned_since.remove(&tag.id);
                self.transition_generations.remove(&tag.id);
            }
        }
        // ── end collapse ──

        let visible_ids: Vec<_> = display_tags.iter().map(|tag| tag.id).collect();
        if self
            .tooltip_hover
            .tag_id
            .is_some_and(|tag_id| !visible_ids.contains(&tag_id))
        {
            self.clear_tooltip(cx);
        }
        #[cfg(target_os = "windows")]
        if tooltip_orphaned_on_window_exit(self.tooltip_hover.tag_id, window.is_window_hovered()) {
            self.clear_tooltip(cx);
        }
        if self
            .drag
            .as_ref()
            .is_some_and(|drag| drag.ids != visible_ids)
        {
            self.cancel_drag(cx);
        }
        let mut preview_ids = visible_ids.clone();
        let dragging_id = self
            .drag
            .as_ref()
            .filter(|drag| drag.moved)
            .map(|drag| drag.id);
        let dragged_top = self.drag.as_ref().map(|drag| drag.top).unwrap_or(0.);
        let settling = self.settling;
        if self
            .settling
            .is_some_and(|(_, _, start)| start.elapsed() >= motion::fast())
        {
            self.settling = None;
        }
        if let Some(drag) = &self.drag {
            if let Some(source) = preview_ids.iter().position(|id| *id == drag.id) {
                preview_ids.remove(source);
                preview_ids.insert(drag.target, drag.id);
            }
        }
        // Paint the dragged row last, inside the clipped sidebar, above its siblings.
        display_tags.sort_by_key(|tag| Some(tag.id) == dragging_id);
        let rows_height = visible_ids.len() as f32 * ROW_HEIGHT;
        let sidebar_bounds = self.bounds.clone();
        let sidebar_for_move = cx.entity();
        let sidebar_for_exit = cx.entity();
        let sidebar_for_up = cx.entity();
        let sidebar_for_press = cx.entity();
        // 取色统一走 `Palette`：本窗口浮在桌面上，行用半透明 `layer` 面 +
        // 一圈描边分层，不再投影（投影只在对话框保留）。
        let p = Palette::current(cx);
        // 这一列浮在桌面/其它窗口之上，行底必须不透底：取 `layer-strong`
        // 叠在窗口底上的不透明等效色，暗色下才不会变成一块几乎透明。
        let text_1 = p.ink;
        let row_bg_default = p.on_card("layer-strong");
        let row_bg_hover = p.on_card("hover-strong");
        let row_border = p.stroke_strong;
        let state_for_click = self.state.clone();
        let sidebar_for_notify = cx.entity().clone();
        let duration = motion::slow();

        div()
            .font(typography::ui_font(cx))
            .relative()
            .w(px(SIDEBAR_WIDTH))
            .h(px(
                rows_height + if hidden_count > 0 { ROW_HEIGHT } else { 0. }
            ))
            // The shadow margin around the strip is transparent; the window clips
            // the rows on the right, where they meet the main panel's stroke.
            .ml(px(TAG_MARGIN))
            .mt(px(TAG_MARGIN))
            .bg(p.background.opacity(0.))
            .child(
                canvas(
                    move |bounds, _, _| sidebar_bounds.set(bounds),
                    move |_, _, window, _| {
                        // Crossing into the main window need not deliver another
                        // MouseMoveEvent to this window, so row hover cannot clear it.
                        window.on_mouse_event(move |_: &MouseExitEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture {
                                sidebar_for_exit.update(cx, |sidebar, cx| {
                                    sidebar.clear_tooltip(cx);
                                });
                            }
                        });
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture
                                && sidebar_for_move.read(cx).drag.is_some()
                            {
                                sidebar_for_move.update(cx, |sidebar, cx| {
                                    if event.pressed_button == Some(MouseButton::Left) {
                                        sidebar.move_drag(event.position, cx);
                                    } else {
                                        sidebar.cancel_drag(cx);
                                    }
                                });
                                cx.stop_propagation();
                            }
                        });
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                            if phase == DispatchPhase::Capture
                                && event.button == MouseButton::Left
                                && sidebar_for_up.read(cx).drag.is_some()
                            {
                                sidebar_for_up.update(cx, |sidebar, cx| {
                                    sidebar.finish_drag(event.position, cx)
                                });
                                cx.stop_propagation();
                            }
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
            .children(display_tags.into_iter().map(move |tag| {
                let checked = active_tag_ids.contains(&tag.id);
                let pinned = pinned_tag_ids.contains(&tag.id);
                let interactable = checked || pinned;
                let entering = checked && !pinned && !previous_rendered_ids.contains(&tag.id);
                let target_row_x = if checked { px(0.0) } else { px(22.0) };
                let target_opacity = if interactable { 1.0 } else { 0.0 };
                let target_text_opacity = if checked { 1.0 } else { 0.3 };
                let target_bar_height = if pinned { px(16.0) } else { px(6.0) };
                let initial_row_x = if entering { px(22.0) } else { target_row_x };
                let initial_opacity = if entering { 0.0 } else { target_opacity };
                let initial_text_opacity = if entering { 0.3 } else { target_text_opacity };
                let initial_bar_height = if entering { px(6.0) } else { target_bar_height };
                let bar_color = parse_tag_color(&tag.color, p.accent.into());
                let label = tag.name;
                let tag_id = tag.id;
                let transition_generation =
                    transition_generations.get(&tag_id).copied().unwrap_or(0);
                let tag_key = (tag_id as u64).wrapping_add(transition_generation << 32);
                let state_for_right = state_for_click.clone();
                let sidebar_for_right = sidebar_for_notify.clone();

                let row_x = keyed_transition(
                    window,
                    cx,
                    ("sidebar-tag-x", tag_key),
                    duration,
                    initial_row_x,
                    ease_in_out,
                    target_row_x,
                );

                let opacity = keyed_transition(
                    window,
                    cx,
                    ("sidebar-tag-opacity", tag_key),
                    duration,
                    initial_opacity,
                    ease_in_out,
                    target_opacity,
                );

                let text_opacity = keyed_transition(
                    window,
                    cx,
                    ("sidebar-tag-text-opacity", tag_key),
                    duration,
                    initial_text_opacity,
                    ease_in_out,
                    target_text_opacity,
                );

                let bar_height = keyed_transition(
                    window,
                    cx,
                    ("sidebar-tag-bar-height", tag_key),
                    duration,
                    initial_bar_height,
                    ease_in_out,
                    target_bar_height,
                );

                let original_slot = visible_ids.iter().position(|id| *id == tag_id).unwrap_or(0);
                let target_slot = preview_ids
                    .iter()
                    .position(|id| *id == tag_id)
                    .unwrap_or(original_slot);
                let animated_y = keyed_transition(
                    window,
                    cx,
                    ("sidebar-tag-y", tag_id as u64),
                    motion::fast(),
                    px(original_slot as f32 * ROW_HEIGHT),
                    ease_in_out,
                    px(target_slot as f32 * ROW_HEIGHT),
                );
                let row_y = if Some(tag_id) == dragging_id {
                    px(dragged_top)
                } else if let Some((_, top, started)) = settling.filter(|(id, _, _)| *id == tag_id)
                {
                    let delta =
                        (started.elapsed().as_secs_f32() / motion::fast().as_secs_f32()).min(1.);
                    if delta < 1. {
                        window.request_animation_frame();
                    }
                    px(top + (target_slot as f32 * ROW_HEIGHT - top) * ease_in_out(delta))
                } else {
                    animated_y
                };

                let tooltip_label = label.clone();
                let tooltip_move_label = label.clone();
                let row_visual = std::rc::Rc::new(move || {
                    // 行高与 `ROW_HEIGHT` 是同一套固定布局（窗口尺寸、拖拽位移都
                    // 按它算），所以这里用不随字号缩放的像素值，避免大字号下压行。
                    div()
                        .w(px(SIDEBAR_WIDTH))
                        .h(px(ROW_HEIGHT - 2.))
                        .opacity(opacity)
                        .rounded(Radius::Sm.px())
                        .bg(row_bg_default)
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(Space::Xs.px())
                        .child(
                            div()
                                .w(px(3.))
                                .h(bar_height)
                                .rounded(Radius::Sm.px())
                                .bg(bar_color),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .h_full()
                                .flex()
                                .items_center()
                                .text_size(typography::font(type_role::BODY))
                                .font_weight(typography::LABEL_WEIGHT)
                                .text_color(text_1)
                                .opacity(text_opacity)
                                .overflow_hidden()
                                .child(label.clone()),
                        )
                        // Hairline stroke drawn on top so it never changes the
                        // row's layout. Its right side is clipped by the window and
                        // meets the main panel's stroke.
                        .child(
                            div()
                                .absolute()
                                .inset_0()
                                .rounded(Radius::Sm.px())
                                .border_1()
                                .border_color(row_border),
                        )
                });

                let row = row_visual()
                    .id(("sidebar-tag", tag_id as u64))
                    .on_hover(cx.listener(move |sidebar, hovered, _, cx| {
                        sidebar.hover_tooltip(
                            tag_id,
                            *hovered,
                            &tooltip_label,
                            SIDEBAR_TOP_OFFSET + f32::from(row_y),
                            cx,
                        );
                    }))
                    // GPUI may retain the row's hover flag across a window exit.
                    // A move on re-entry must also be able to start a fresh delay.
                    .on_mouse_move(cx.listener(move |sidebar, _, _, cx| {
                        sidebar.hover_tooltip(
                            tag_id,
                            true,
                            &tooltip_move_label,
                            SIDEBAR_TOP_OFFSET + f32::from(row_y),
                            cx,
                        );
                    }))
                    .absolute()
                    .left(row_x)
                    .top(row_y)
                    .when(Some(tag_id) == dragging_id, |row| row.bg(row_bg_hover))
                    .cursor(if interactable {
                        CursorStyle::PointingHand
                    } else {
                        CursorStyle::Arrow
                    })
                    .when(interactable, |row| {
                        row.hover(move |style| style.bg(row_bg_hover))
                    });

                if interactable {
                    let sidebar = sidebar_for_press.clone();
                    let ids = visible_ids.clone();
                    row.on_mouse_down(MouseButton::Left, move |event, _, cx| {
                        cx.stop_propagation();
                        sidebar.update(cx, |sidebar, cx| {
                            sidebar.clear_tooltip(cx);
                            sidebar.settling = None;
                            sidebar.drag = Some(SidebarDrag {
                                id: tag_id,
                                ids: ids.clone(),
                                start: event.position,
                                grab_y: f32::from(
                                    event.position.y - sidebar.bounds.get().top() - row_y,
                                ),
                                top: f32::from(row_y),
                                target: original_slot,
                                moved: false,
                            });
                            cx.notify();
                        });
                    })
                    .on_mouse_down(MouseButton::Right, move |_, _, cx| {
                        state_for_right.update(cx, |state, cx| {
                            state.toggle_pinned_tag(tag_id);
                            cx.notify();
                        });
                        sidebar_for_right.update(cx, |sidebar, cx| {
                            sidebar.clear_tooltip(cx);
                            cx.notify();
                        });
                    })
                } else {
                    row
                }
            }))
            .when(hidden_count > 0, move |parent| {
                // The surface is exactly one row wide, so the pill's open right
                // side ends on the main panel's stroke like the tag rows do.
                parent.child(
                    div()
                        .absolute()
                        .top(px(rows_height))
                        .w(px(36.))
                        .flex()
                        .justify_end()
                        .child(
                            div()
                                .h(px(18.))
                                .rounded_l(Radius::Pill.px())
                                .bg(p.on_card("layer-strong"))
                                .border_1()
                                .border_color(p.stroke_strong)
                                .px(Space::Xs.scaled())
                                .flex()
                                .items_center()
                                .text_size(typography::font(type_role::CAPTION))
                                .text_color(p.ink_2)
                                .cursor(CursorStyle::PointingHand)
                                .child(format!("+{hidden_count}")),
                        ),
                )
            })
    }
}

/// Parse a hex color string like "#EF4444" into an Rgba；颜色缺失时回落到
/// 调用方给的强调色，不再硬编码一档品牌绿。
fn parse_tag_color(hex: &str, fallback: Rgba) -> Rgba {
    let s = hex.trim_start_matches('#');
    if s.len() == 6 {
        let r = u32::from_str_radix(&s[0..2], 16).unwrap_or(0x7e);
        let g = u32::from_str_radix(&s[2..4], 16).unwrap_or(0xcb);
        let b = u32::from_str_radix(&s[4..6], 16).unwrap_or(0xa3);
        rgba((r << 24) | (g << 16) | (b << 8) | 0xff)
    } else {
        fallback
    }
}

fn ordered_sidebar_tags(
    tags: &[TagInfo],
    active_tag_ids: &[i64],
    pinned_tag_ids: &[i64],
    unchecked_unpinned_since: &HashMap<i64, Instant>,
) -> Vec<TagInfo> {
    // Pinning controls visibility, while the shared tag order controls position.
    tags.iter()
        .filter(|tag| {
            pinned_tag_ids.contains(&tag.id)
                || active_tag_ids.contains(&tag.id)
                || unchecked_unpinned_since.contains_key(&tag.id)
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "windows")]
    use super::tooltip_orphaned_on_window_exit;
    use super::{drag_position, ordered_sidebar_tags, TooltipHover};
    use crate::core::types::TagInfo;
    use std::{collections::HashMap, time::Instant};

    #[test]
    fn tooltip_window_exit_allows_reentering_the_same_tag() {
        let mut hover = TooltipHover::default();
        assert!(hover.update(1, true));
        assert!(!hover.update(1, true));
        assert!(hover.clear());
        assert_eq!(hover.tag_id, None);
        assert!(hover.update(1, true));
        assert!(hover.update(1, false));
        assert_eq!(hover.tag_id, None);
    }

    #[test]
    fn tooltip_ignores_a_previous_tags_late_leave_event() {
        let mut hover = TooltipHover::default();
        assert!(hover.update(1, true));
        assert!(hover.update(2, true));
        assert!(!hover.update(1, false));
        assert_eq!(hover.tag_id, Some(2));
        assert!(hover.update(2, false));
        assert!(!hover.update(2, false));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn tooltip_is_dropped_when_the_tag_window_loses_the_pointer() {
        assert!(tooltip_orphaned_on_window_exit(Some(3), false));
        assert!(!tooltip_orphaned_on_window_exit(Some(3), true));
        assert!(!tooltip_orphaned_on_window_exit(None, false));
    }

    #[test]
    fn sidebar_drag_clamps_to_rows_and_respects_the_grab_offset() {
        assert_eq!(drag_position(-100., 8., 4), (0., 0));
        assert_eq!(drag_position(1000., 8., 4), (72., 3));
        assert_eq!(drag_position(43., 8., 4), (35., 1));
        assert_eq!(drag_position(45., 8., 4), (37., 2));
        assert_eq!(drag_position(500., 8., 1), (0., 0));
        assert_eq!(drag_position(500., 8., 0), (0., 0));
    }

    #[test]
    fn sidebar_keeps_shared_order_for_pinned_active_and_fading_tags() {
        let tags: Vec<_> = [4, 2, 1, 3, 5]
            .into_iter()
            .map(|id| TagInfo {
                id,
                uid: id.to_string(),
                name: id.to_string(),
                color: "FF0000".into(),
                updated_at: String::new(),
            })
            .collect();
        let fading = HashMap::from([(1, Instant::now())]);
        let ordered = ordered_sidebar_tags(&tags, &[2, 4], &[3, 4], &fading);
        assert_eq!(
            ordered.iter().map(|tag| tag.id).collect::<Vec<_>>(),
            [4, 2, 1, 3]
        );
    }
}
