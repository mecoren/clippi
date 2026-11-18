//! macOS reading of the "Reduce motion" preference.

use objc2_app_kit::NSWorkspace;

/// Whether the system draws animations; the inverse of Accessibility →
/// Display → "Reduce motion".
pub fn system_animations_enabled() -> bool {
    !NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}
