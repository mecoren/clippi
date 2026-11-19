//! 超椭圆（squircle）圆角路径：Orbit 移动端 / HyperOS 的按钮几何。
//!
//! 普通圆角在「直边 → 圆弧」的接点处曲率突变，放大后能看到一圈硬接缝；
//! 超椭圆用一段三次贝塞尔把直边平滑地接进弧段（连续圆角），整枚按钮读作
//! 一个被压圆的方块，而不是「矩形 + 四个圆角」。
//!
//! 数值与 compose-miuix `SquirclePath.kt` 同源（Orbit 移动端 `miuix_squircle`
//! 的移植来源）：圆角瓦片取 `1.1 × radius`，控制点比例 `0.643`。

use gpui::{point, px, Bounds, Path, PathBuilder, Pixels};

/// 圆角瓦片相对 radius 的倍数：1.0 退化为圆弧，1.1 为连续圆角。
pub const EXTENSION: f32 = 1.1;

/// 三次贝塞尔控制点比例（与 compose-miuix 的 SDF 预烘焙值一致）。
const CONTROL: f32 = 0.643;

/// 圆角瓦片边长：`radius × EXTENSION`，夹到短边的一半（半径再大也只能是
/// 半个胶囊），非正值退化为直角。
pub fn tile(width: f32, height: f32, radius: f32) -> f32 {
    (radius * EXTENSION).clamp(0., width.min(height) / 2.)
}

/// 在 `bounds` 内构造超椭圆填充路径；宽高非正时返回 `None`（无可画内容）。
pub fn fill_path(bounds: Bounds<Pixels>, radius: Pixels) -> Option<Path<Pixels>> {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    if width <= 0. || height <= 0. {
        return None;
    }

    let (x, y) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
    let at = |dx: f32, dy: f32| point(px(x + dx), px(y + dy));
    let tile = tile(width, height, f32::from(radius));
    let mut path = PathBuilder::fill();

    if tile <= 0. {
        path.move_to(at(0., 0.));
        path.line_to(at(width, 0.));
        path.line_to(at(width, height));
        path.line_to(at(0., height));
        path.close();
        return path.build().ok();
    }

    let handle = tile * (1. - CONTROL);
    // 上边 → 右上角 → 右边 → 右下角 → 下边 → 左下角 → 左边 → 左上角。
    path.move_to(at(tile, 0.));
    path.line_to(at(width - tile, 0.));
    path.cubic_bezier_to(at(width, tile), at(width - handle, 0.), at(width, handle));
    path.line_to(at(width, height - tile));
    path.cubic_bezier_to(
        at(width - tile, height),
        at(width, height - handle),
        at(width - handle, height),
    );
    path.line_to(at(tile, height));
    path.cubic_bezier_to(
        at(0., height - tile),
        at(handle, height),
        at(0., height - handle),
    );
    path.line_to(at(0., tile));
    path.cubic_bezier_to(at(tile, 0.), at(0., handle), at(handle, 0.));
    path.close();
    path.build().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;

    #[test]
    fn tile_follows_the_radius_until_it_hits_the_short_side() {
        // 32 高按钮 + 10 圆角：瓦片 11，仍在半高（16）以内。
        assert!((tile(120., 32., 10.) - 11.).abs() < 1e-5);
        // 半径超过半高时夹到半高，避免相邻两角互相咬住。
        assert!((tile(120., 12., 40.) - 6.).abs() < 1e-5);
        // 半径为 0 退化为直角。
        assert_eq!(tile(120., 32., 0.), 0.);
    }

    #[test]
    fn fill_path_needs_a_positive_size() {
        let origin = point(px(4.), px(8.));
        assert!(fill_path(Bounds::new(origin, size(px(0.), px(10.))), px(10.)).is_none());
        assert!(fill_path(Bounds::new(origin, size(px(64.), px(32.))), px(10.)).is_some());
        // 半径为 0 时退化为矩形，仍然可绘制。
        assert!(fill_path(Bounds::new(origin, size(px(64.), px(32.))), px(0.)).is_some());
    }
}
