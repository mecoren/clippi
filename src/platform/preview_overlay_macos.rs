//! macOS implementation of the connector overlay policy.
//!
//! The overlay is a borderless, transparent NSWindow added as a child of the
//! flyout (so it stacks above the flyout and the main window) and marked
//! `ignoresMouseEvents`, which makes AppKit pass every click/hover through to
//! the windows underneath. **Untested on macOS** (no device available).

use std::rc::Rc;

use objc2::rc::Retained;
use objc2_app_kit::{NSColor, NSWindow, NSWindowOrderingMode};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};

use crate::platform::monitor;

pub struct Overlay {
    overlay: Retained<NSWindow>,
}

impl Overlay {
    /// `owner` is the flyout's NSWindow; `overlay` is this window's NSWindow.
    pub fn install(owner: isize, overlay: isize) -> Result<Rc<Self>, &'static str> {
        let _main_thread =
            MainThreadMarker::new().ok_or("connector overlay requires the main thread")?;
        // SAFETY: live NSWindow pointers obtained from GPUI handles.
        let (owner, overlay) = unsafe {
            (
                Retained::retain(owner as *mut NSWindow).ok_or("missing flyout window")?,
                Retained::retain(overlay as *mut NSWindow)
                    .ok_or("missing connector overlay window")?,
            )
        };
        crate::platform::frame_pacing::configure(&overlay);
        overlay.setOpaque(false);
        overlay.setBackgroundColor(Some(&NSColor::clearColor()));
        overlay.setHasShadow(false);
        overlay.setIgnoresMouseEvents(true);
        unsafe {
            overlay.setLevel(owner.level());
            if overlay.parentWindow().as_deref() != Some(&*owner) {
                owner.addChildWindow_ordered(&overlay, NSWindowOrderingMode::Above);
            }
        }
        Ok(Rc::new(Self { overlay }))
    }

    /// Move, resize and show at a logical-screen rectangle (top-left origin).
    pub fn place(&self, x: f32, y: f32, width: f32, height: f32) -> bool {
        let Some(primary_height) = monitor::primary_screen_height() else {
            return false;
        };
        let cocoa_y = primary_height - (y + height) as f64;
        let frame = NSRect::new(
            NSPoint::new(x as f64, cocoa_y),
            NSSize::new(width as f64, height as f64),
        );
        if self.overlay.frame() != frame {
            self.overlay.setFrame_display(frame, false);
        }
        self.overlay.orderFront(None);
        true
    }

    pub fn hide(&self) {
        self.overlay.orderOut(None);
    }
}
