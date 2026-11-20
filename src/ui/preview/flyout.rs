//! Content host for the native hover flyout.
//!
//! The flyout is one non-activating window. It starts with the compact
//! `PreviewPanelView` and, after the hover dwell (or the "放大查看" button),
//! swaps to the full `PreviewWindowView` **inside the same window**. The native
//! window is a fixed transparent canvas; only the card rect inside it animates,
//! so no second window opens and no native resize happens mid-motion.

use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::core::types::ClipboardItem;
use crate::state::app::AppState;
use crate::ui::design::palette::Palette;
use crate::ui::preview::{PreviewPanelView, PreviewWindowView};
use crate::ui::widgets;

/// Full-preview card size; the prewarm layout and the expand target share it
/// so promotion never changes the TextView's measure width.
pub const EXPANDED_WIDTH: f32 = 760.;
pub const EXPANDED_HEIGHT: f32 = 560.;

pub struct PreviewFlyoutView {
    compact: Entity<PreviewPanelView>,
    expanded: Option<Entity<PreviewWindowView>>,
    /// The expanded view exists but is still laid out off-canvas (prewarm).
    revealed: bool,
    state: Entity<AppState>,
    /// Logical screen origin of the fixed native canvas and the card's logical
    /// screen rect. The card animates inside the canvas; the window never moves.
    origin: (f32, f32),
    card: Option<crate::platform::preview_flyout::LogicalRect>,
    /// Final card size while a grow/shrink animation runs. The expanded view
    /// is laid out at this size once and revealed by the card's rounded clip:
    /// a per-frame width change would make TextView remeasure every block of
    /// the document on every frame.
    anim_size: Option<(f32, f32)>,
}

impl PreviewFlyoutView {
    pub fn new(
        compact: Entity<PreviewPanelView>,
        state: Entity<AppState>,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            compact,
            expanded: None,
            revealed: false,
            state,
            origin: (0., 0.),
            card: None,
            anim_size: None,
        }
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded.is_some()
    }

    pub fn item_id(&self, cx: &App) -> Option<i64> {
        self.expanded
            .as_ref()
            .and_then(|view| view.read(cx).item_id())
    }

    /// Build (or retarget) the full preview off-canvas during the hover dwell:
    /// its one-time parse and full measure land while the pointer rests, so
    /// promotion only reveals an already-laid-out document.
    pub fn prewarm(&mut self, item: ClipboardItem, cx: &mut Context<Self>) {
        if let Some(view) = &self.expanded {
            let view = view.clone();
            view.update(cx, |view, cx| {
                if view.item_id() != Some(item.id) {
                    view.set_item(item, cx);
                }
            });
        } else {
            let state = self.state.clone();
            self.expanded = Some(cx.new(|cx| PreviewWindowView::new(state, item, None, cx)));
        }
        self.revealed = false;
        cx.notify();
    }

    /// Enter/leave the clip-reveal motion: while animating, the card owns the
    /// rounded clip, stroke and shadow, and the expanded view keeps a fixed
    /// layout size; at rest the view draws its own frame again.
    pub fn set_anim_size(&mut self, size: Option<(f32, f32)>, cx: &mut Context<Self>) {
        if self.anim_size == size {
            return;
        }
        self.anim_size = size;
        if let Some(view) = &self.expanded {
            let view = view.clone();
            view.update(cx, |view, cx| view.set_draw_frame(size.is_none(), cx));
        }
        cx.notify();
    }

    /// Place the card inside the fixed canvas. Called once per animation frame
    /// with the same logical screen rect the connector line is fed.
    pub fn set_card_screen_rect(
        &mut self,
        origin: (f32, f32),
        rect: crate::platform::preview_flyout::LogicalRect,
        cx: &mut Context<Self>,
    ) {
        self.origin = origin;
        self.card = Some(rect);
        cx.notify();
    }

    /// Swap the compact sheet for the full preview. The caller resizes the
    /// same native window to the returned size afterwards.
    pub fn expand(&mut self, item: ClipboardItem, cx: &mut Context<Self>) {
        if let Some(view) = &self.expanded {
            let view = view.clone();
            view.update(cx, |view, cx| {
                if view.item_id() != Some(item.id) {
                    view.set_item(item, cx);
                }
            });
        } else {
            let state = self.state.clone();
            let view = cx.new(|cx| PreviewWindowView::new(state, item, None, cx));
            self.expanded = Some(view);
        }
        self.revealed = true;
        cx.notify();
    }

    /// Return to the compact sheet. Called before the native window hides, so
    /// the next hover starts small again.
    pub fn collapse(&mut self, cx: &mut Context<Self>) {
        self.anim_size = None;
        self.revealed = false;
        if self.expanded.take().is_some() {
            cx.notify();
        }
    }
}

impl Render for PreviewFlyoutView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bounds = window.bounds();
        let (x, y, width, height) = match self.card {
            Some(rect) => (
                rect.0 - self.origin.0,
                rect.1 - self.origin.1,
                rect.2,
                rect.3,
            ),
            None => (
                0.,
                0.,
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
            ),
        };
        // The list's cursor watchdog tests the card's screen rect, not the
        // canvas: the canvas spans the work area and would swallow every hover.
        self.compact.update(cx, |panel, _| {
            panel.set_screen_bounds(Bounds::new(
                point(px(self.origin.0 + x), px(self.origin.1 + y)),
                size(px(width), px(height)),
            ));
        });
        // The compact sheet stays mounted underneath the expanded view so its
        // entrance-transition state survives: collapsing back onto the same
        // item reveals it already at full opacity instead of replaying the
        // fade (which read as the preview blinking out and back in).
        let expanded = self.expanded.clone();
        let anim_size = self.anim_size;
        let p = Palette::current(cx);
        let compact = self.compact.clone();
        let mut card = div()
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(width))
            .h(px(height))
            // Clip-reveal motion: the card paints the rounded surface and
            // clips the content, which keeps its final layout size so the
            // document is laid out once instead of remeasured every frame.
            // gpui's overflow clip is rectangular, so the rounded edge must
            // come from the card's own background/border, not from a child.
            .when(anim_size.is_some(), |card| {
                card.bg(p.background)
                    .rounded(px(crate::core::frontend::PANEL_RADIUS))
                    .overflow_hidden()
                    .border(px(1.))
                    .border_color(p.stroke)
                    .when(cfg!(target_os = "windows"), |card| {
                        card.shadow(widgets::flyout_shadow(p.dark))
                    })
            })
            // Under the transparent animated surface the compact excerpt
            // would show through the full document; it returns at rest.
            .when(expanded.is_none() || anim_size.is_none(), |card| {
                match anim_size {
                    // Same clip-reveal contract as the expanded document: the
                    // sheet is laid out once at its final size while the card
                    // rect springs, so a height change between two items never
                    // re-flows the text on every animation frame.
                    Some((w, h)) => card.child(
                        div()
                            .absolute()
                            .left(px(0.))
                            .top(px(0.))
                            .w(px(w))
                            .h(px(h))
                            .child(compact),
                    ),
                    None => card.child(compact),
                }
            });
        if let Some(view) = expanded {
            let wrapper = div().absolute();
            let wrapper = match anim_size {
                Some((w, h)) => wrapper.left(px(0.)).top(px(0.)).w(px(w)).h(px(h)).occlude(),
                None if self.revealed => wrapper.inset_0().occlude(),
                // Prewarm: measure the document at its final size while the
                // pointer rests, but paint nothing and swallow no hover. The
                // clip-reveal then promotes an already-laid-out TextView, so
                // the grow animation never reparses or remeasures the body.
                None => wrapper
                    .left(px(0.))
                    .top(px(0.))
                    .w(px(EXPANDED_WIDTH))
                    .h(px(EXPANDED_HEIGHT))
                    .opacity(0.),
            };
            card = card.child(wrapper.child(view));
        }
        div().size_full().relative().child(card)
    }
}
