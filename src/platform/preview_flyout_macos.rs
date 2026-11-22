use std::rc::Rc;
use std::sync::OnceLock;

use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::{msg_send, rc::Retained, sel};
use objc2_app_kit::{NSColor, NSWindow, NSWindowOrderingMode};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};

use super::dock_flyout_bounds;
use crate::platform::monitor;

/// Logical screen rectangle (top-left origin): `(x, y, width, height)`.
pub type LogicalRect = (f32, f32, f32, f32);

pub struct Attachment {
    main: Retained<NSWindow>,
    flyout: Retained<NSWindow>,
}

unsafe extern "C" fn cannot_become_key(_: *mut AnyObject, _: Sel) -> Bool {
    Bool::NO
}

/// GPUI's GPUIPanel overrides canBecomeKeyWindow to YES. Restrict only this
/// instance, like the tag sidebar does, instead of changing GPUI's shared class.
fn nonactivating_class(panel: &NSWindow) -> &'static AnyClass {
    static CLASS: OnceLock<&'static AnyClass> = OnceLock::new();
    CLASS.get_or_init(|| {
        let mut builder = ClassBuilder::new(c"ClippiPreviewFlyoutPanel", panel.class())
            .expect("unique preview flyout panel class");
        unsafe {
            builder.add_method(
                sel!(canBecomeKeyWindow),
                cannot_become_key as unsafe extern "C" fn(*mut AnyObject, Sel) -> Bool,
            );
            builder.add_method(
                sel!(canBecomeMainWindow),
                cannot_become_key as unsafe extern "C" fn(*mut AnyObject, Sel) -> Bool,
            );
        }
        builder.register()
    })
}

impl Attachment {
    pub fn install(main: isize, flyout: isize) -> Result<Rc<Self>, &'static str> {
        let _main_thread =
            MainThreadMarker::new().ok_or("preview flyout requires the main thread")?;
        // SAFETY: these are live NSWindow pointers obtained from GPUI handles.
        let (main, flyout) = unsafe {
            (
                Retained::retain(main as *mut NSWindow).ok_or("missing main window")?,
                Retained::retain(flyout as *mut NSWindow).ok_or("missing preview flyout window")?,
            )
        };
        crate::platform::frame_pacing::configure(&flyout);
        flyout.setOpaque(false);
        flyout.setBackgroundColor(Some(&NSColor::clearColor()));
        flyout.setHasShadow(true);
        unsafe {
            // GPUI creates WindowKind::PopUp as an NSPanel; make it borderless
            // and non-activating so hovering/clicking never steals search focus.
            let _: () = msg_send![&*flyout, setStyleMask: (1usize << 7)];
            let _: () = msg_send![&*flyout, setBecomesKeyOnlyIfNeeded: true];
            let _: () = msg_send![&*flyout, setHidesOnDeactivate: false];
            AnyObject::set_class(&flyout, nonactivating_class(&flyout));
        }
        Ok(Rc::new(Self { main, flyout }))
    }

    /// Docked compact rect for a source anchor, in logical screen pixels
    /// (top-left origin). `card_top`/`card_height` describe the 1px pointer
    /// anchor; the caller springs [`Attachment::place`] towards this target.
    /// Returns `None` while the main window is hidden or miniaturized.
    pub fn docked_rect(
        &self,
        card_top: f32,
        card_height: f32,
        width: f32,
        height: f32,
    ) -> Option<LogicalRect> {
        if !self.main.isVisible() || self.main.isMiniaturized() {
            return None;
        }
        let primary_height = monitor::primary_screen_height()?;
        let frame = self.main.frame();
        let main_rect = monitor::cocoa_rect_to_top_left(
            primary_height,
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
        );
        let screen = self.main.screen()?;
        let visible = screen.visibleFrame();
        let work = monitor::cocoa_rect_to_top_left(
            primary_height,
            visible.origin.x,
            visible.origin.y,
            visible.size.width,
            visible.size.height,
        );

        let anchor_y = main_rect.y as f32 + super::centered_dock_top(card_top, card_height, height);
        Some(dock_flyout_bounds(
            (
                main_rect.x as f32,
                main_rect.y as f32,
                main_rect.width as f32,
                main_rect.height as f32,
            ),
            anchor_y,
            (
                work.x as f32,
                work.y as f32,
                work.width as f32,
                work.height as f32,
            ),
            (width, height),
        ))
    }

    pub fn hide(&self) {
        self.flyout.orderOut(None);
    }

    /// Start/stop routing wheel input over the compact sheet to the list.
    ///
    /// Windows needs an explicit native forward because the wheel follows the
    /// window under the pointer; Cocoa already delivers it to the scroll view
    /// under the cursor and the list keeps scrolling on its own, so this is a
    /// no-op here. 未在 macOS 实测（Windows 为主验证平台）。
    pub fn set_wheel_to_main(&self, _forward: bool) {}

    /// Expanded rect for the full preview, grown from the current frame while
    /// preserving the side and staying centered on the card's center line
    /// (Cocoa coordinates internally).
    pub fn expand_rect(&self, width: f32, height: f32) -> Option<LogicalRect> {
        let frame = self.flyout.frame();
        let main = self.main.frame();
        let on_right =
            frame.origin.x + frame.size.width * 0.5 >= main.origin.x + main.size.width * 0.5;
        let mut x = if on_right {
            frame.origin.x
        } else {
            frame.origin.x + frame.size.width - width as f64
        };
        let center_y = frame.origin.y + frame.size.height / 2.;
        let mut y = center_y - height as f64 / 2.;

        if let Some(screen) = self.flyout.screen() {
            let visible = screen.visibleFrame();
            x = x
                .max(visible.origin.x)
                .min(visible.origin.x + visible.size.width - width as f64);
            y = y
                .max(visible.origin.y)
                .min(visible.origin.y + visible.size.height - height as f64);
        }

        let primary_height = monitor::primary_screen_height()?;
        Some((
            x as f32,
            primary_height as f32 - (y + height as f64) as f32,
            width,
            height,
        ))
    }

    /// Work area of the main window's screen in logical screen pixels
    /// (top-left origin). The connector overlay covers it once instead of
    /// chasing every retarget.
    pub fn work_area(&self) -> Option<LogicalRect> {
        let screen = self.main.screen()?;
        let visible = screen.visibleFrame();
        let primary_height = monitor::primary_screen_height()?;
        let work = monitor::cocoa_rect_to_top_left(
            primary_height,
            visible.origin.x,
            visible.origin.y,
            visible.size.width,
            visible.size.height,
        );
        Some((
            work.x as f32,
            work.y as f32,
            work.width as f32,
            work.height as f32,
        ))
    }

    /// The window's current rect in logical screen pixels (top-left origin).
    pub fn current_rect(&self) -> Option<LogicalRect> {
        let primary_height = monitor::primary_screen_height()?;
        let frame = self.flyout.frame();
        let rect = monitor::cocoa_rect_to_top_left(
            primary_height,
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
        );
        Some((
            rect.x as f32,
            rect.y as f32,
            rect.width as f32,
            rect.height as f32,
        ))
    }

    /// Move, resize and show at a logical-screen rectangle.
    pub fn place(&self, rect: LogicalRect) -> bool {
        let Some(primary_height) = monitor::primary_screen_height() else {
            return false;
        };
        let (x, y, width, height) = (rect.0 as f64, rect.1 as f64, rect.2 as f64, rect.3 as f64);
        let cocoa_y = primary_height - (y + height);
        let target = NSRect::new(NSPoint::new(x, cocoa_y), NSSize::new(width, height));
        if self.flyout.frame() != target {
            self.flyout.setFrame_display(target, true);
        }
        if self.flyout.parentWindow().as_deref() != Some(&*self.main) {
            unsafe {
                self.main
                    .addChildWindow_ordered(&self.flyout, NSWindowOrderingMode::Above);
            }
        }
        self.flyout.setLevel(self.main.level());
        self.flyout.orderFront(None);
        true
    }

    /// Logical screen origin of the native window. On macOS the window is the
    /// card itself, so the in-app card offset is always zero.
    pub fn canvas_origin(&self) -> Option<(f32, f32)> {
        let rect = self.current_rect()?;
        Some((rect.0, rect.1))
    }

    /// Whether the flyout window is currently visible.
    pub fn is_shown(&self) -> bool {
        self.flyout.isVisible()
    }
}
