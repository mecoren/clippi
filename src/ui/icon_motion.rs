//! 图标悬停动效：指针进入按钮时，按钮图标按语义播放一次短动效。
//!
//! 移植自 Argus `frontend/icon_motion.rs`：一次播放只跑一遍，结束精确回到
//! 静止（`Pose::REST`）；不做循环。暂停帧由播放自身的时钟采样，所以控件在
//! 播放途中重绘也能接着走，不会从头再来。
//!
//! 图标换名（收藏 ↔ 取消收藏、展开 ↔ 收起）时播 `Enter`：新图标以缩小淡入
//! 替换旧图标。
//!
//! 播放状态放在线程本地：`widgets::icon_moving` 每次渲染读它，`trigger` 在
//! 指针悬停回调里写入。`reduce_motion` 的判定在触发点完成（那里有 `App`），
//! 播放渲染不需要再拿 `App`。

use std::cell::RefCell;
use std::collections::HashMap;
use std::f32::consts::PI;
use std::time::{Duration, Instant};

use gpui::SharedString;

use super::design::motion;

/// 播放表中的条目上限：通知、对话框会带来不断变化的名字，超过后整体清空。
const MAX_PLAYS: usize = 512;

/// 图标在指针进入时做什么。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Motion {
    /// 从该角度转回原位。起点必须是图标自身的对称位（整圈或对称角）。
    Spin(f32),
    /// 摆动该角度再回来。
    Swing(f32),
    /// 位移该距离（px）再回来。
    Nudge(f32, f32),
    /// 轻微放大再落定。
    Pop,
    /// 新图标入场。
    Enter,
}

/// 某一时刻的绘制姿态。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub scale: (f32, f32),
    /// 顺时针，弧度。
    pub turn: f32,
    pub shift: (f32, f32),
    pub alpha: f32,
}

impl Pose {
    pub const REST: Pose = Pose {
        scale: (1., 1.),
        turn: 0.,
        shift: (0., 0.),
        alpha: 1.,
    };
}

/// 快速出去、慢慢回来：两端为 0，约 1/5 处到 1。
fn there_and_back(t: f32) -> f32 {
    (PI * motion::ease_enter(t)).sin()
}

impl Motion {
    /// 按图标语义取动效。带方向的按方向位移，会转的（齿轮、星形）按对称角
    /// 旋转，其余轻放大；未登记的名字回退 `Pop`。
    pub fn of(name: &str) -> Self {
        match name {
            // 齿轮六齿：转 1/6 圈后回到自身。
            "SettingsIcon" => Self::Spin(-60.),
            // 星形五角：1/5 圈是对称位。
            "StarIcon" | "StarFilledIcon" => Self::Spin(-72.),
            "PlusIcon" | "CloseIcon" => Self::Spin(-90.),
            "PinIcon" => Self::Swing(-18.),
            "DeleteIcon" | "EditIcon" | "ClearIcon" => Self::Swing(-10.),
            "BanIcon" | "ClipboardBlockIcon" => Self::Swing(-12.),
            "TagIcon" | "NoteIcon" | "LinkIcon" => Self::Swing(18.),
            "DownloadIcon" | "ChevronDownIcon" => Self::Nudge(0., 2.),
            "UploadIcon" | "CheckIcon" => Self::Nudge(0., -2.),
            "OpenIcon" | "ShareIcon" => Self::Nudge(1.5, -1.5),
            "BackIcon" | "ChevronLeftIcon" => Self::Nudge(-2., 0.),
            "ChevronRightIcon" => Self::Nudge(2., 0.),
            "KeyboardIcon" => Self::Nudge(0., 1.),
            _ => Self::Pop,
        }
    }

    pub fn duration(self) -> Duration {
        Duration::from_millis(match self {
            Self::Spin(degrees) if degrees.abs() >= 180. => 620,
            Self::Spin(_) => 420,
            Self::Swing(_) => 460,
            Self::Nudge(..) => 420,
            Self::Pop => 320,
            Self::Enter => 260,
        })
    }

    /// 从 `since` 开始的播放走到 `now` 的进度（0..1）；没有播放或已结束时
    /// 返回 `None`。播放只跑一次：之后重新绘制的控件显示静止图标。
    pub fn played(self, since: Option<Instant>, now: Instant) -> Option<f32> {
        let elapsed = now.saturating_duration_since(since?).as_secs_f32();
        let t = elapsed / self.duration().as_secs_f32();
        (t < 1.).then_some(t)
    }

    /// `t`（0..1）处的姿态；`t >= 1` 时回到静止。
    pub fn pose(self, t: f32) -> Pose {
        if t >= 1. {
            return Pose::REST;
        }
        let t = t.max(0.);
        match self {
            Self::Spin(degrees) => Pose {
                turn: degrees.to_radians() * (1. - motion::ease_move(t)),
                ..Pose::REST
            },
            Self::Swing(degrees) => Pose {
                turn: degrees.to_radians() * there_and_back(t),
                ..Pose::REST
            },
            Self::Nudge(x, y) => {
                let far = there_and_back(t);
                Pose {
                    shift: (x * far, y * far),
                    ..Pose::REST
                }
            }
            Self::Pop => {
                let scale = 1. + 0.14 * there_and_back(t);
                Pose {
                    scale: (scale, scale),
                    ..Pose::REST
                }
            }
            Self::Enter => {
                // 落定前略微超过目标尺寸。
                let scale = 0.6 + 0.4 * motion::bezier(0.34, 1.56, 0.64, 1., t);
                Pose {
                    scale: (scale, scale),
                    alpha: motion::ease_out((t / 0.5).min(1.)),
                    ..Pose::REST
                }
            }
        }
    }
}

/// 一个图标的播放记录。
struct Play {
    /// 播放次数：每播一次递增，作为动画元素 id，保证播放期间持续有帧。
    plays: u64,
    /// 本次播放的起点。用播放自己的时钟采样，不依赖 GPUI 动画的内部状态。
    since: Option<Instant>,
    /// 上一次绘制的图标名，用于判断是否换图标。
    name: String,
    /// 本次播放是新图标入场，而不是指针进入。
    entered: bool,
}

thread_local! {
    static PLAYS: RefCell<HashMap<SharedString, Play>> = RefCell::new(HashMap::new());
    /// 界面动效总开关。悬停触发点在回调里已经查过 `App::reduce_motion`，
    /// 但「图标换名播 Enter」发生在绘制期，拿不到 `App`，只能读这里。
    static ENABLED: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// 由 [`super::motion_pref::apply`] 写入，与 `App::set_reduce_motion` 同步。
pub fn set_enabled(enabled: bool) {
    ENABLED.with(|flag| flag.set(enabled));
}

fn enabled() -> bool {
    ENABLED.with(std::cell::Cell::get)
}

/// 指针进入控件：开始一次悬停动效。`reduce_motion` 由调用点判定。
pub fn trigger(control: &SharedString) {
    PLAYS.with(|plays| {
        let mut all = plays.borrow_mut();
        // 从未绘制过的控件（或注册表刚被清空）不建档：下一次绘制会自己登记，
        // 否则空名字会在首帧误判成「换图标」而播 Enter。
        let Some(play) = all.get_mut(control) else {
            return;
        };
        play.plays += 1;
        play.since = Some(Instant::now());
        play.entered = false;
    });
}

fn new_play(name: String) -> Play {
    Play {
        plays: 0,
        since: None,
        name,
        entered: false,
    }
}

/// 绘制前同步图标名并返回本次要播的动效与起点。
///
/// 返回 `(motion, plays, since)`：`since` 为 `None` 表示静止，不需要动画帧。
pub fn prepare(control: &SharedString, name: &str) -> (Motion, u64, Option<Instant>) {
    PLAYS.with(|plays| {
        let mut all = plays.borrow_mut();
        if all.len() > MAX_PLAYS && !all.contains_key(control) {
            all.clear();
        }
        let play = all
            .entry(control.clone())
            .or_insert_with(|| new_play(name.to_owned()));
        // 控件换了图标：新图标入场（界面动效关闭时只更新名字，不播）。
        if play.name != name {
            let motion_enabled = enabled();
            play.name = name.to_owned();
            if motion_enabled {
                play.plays += 1;
                play.since = Some(Instant::now());
                play.entered = true;
            } else {
                play.since = None;
                play.entered = false;
            }
        }
        let motion = if play.entered {
            Motion::Enter
        } else {
            Motion::of(name)
        };
        let started = play
            .since
            .filter(|&since| motion.played(Some(since), Instant::now()).is_some());
        (motion, play.plays, started)
    })
}

#[cfg(test)]
mod tests {
    use super::{there_and_back, Motion, Pose};
    use std::time::{Duration, Instant};

    /// 一次播放只跑一次，按自己的时钟推进。
    #[test]
    fn a_play_runs_once() {
        let since = Instant::now();
        let at = |ms: u64| since + Duration::from_millis(ms);
        let motion = Motion::Spin(-60.);
        assert_eq!(motion.played(None, at(0)), None);
        let t = motion.played(Some(since), at(210)).unwrap();
        assert!((t - 0.5).abs() < 1e-3, "{t}");
        assert_eq!(motion.played(Some(since), since + motion.duration()), None);
        assert_eq!(motion.played(Some(since), at(60_000)), None);
        assert_eq!(motion.played(Some(at(5)), since), Some(0.));
    }

    fn at_rest(pose: Pose) -> bool {
        (pose.scale.0 - 1.).abs() < 1e-4
            && (pose.scale.1 - 1.).abs() < 1e-4
            && pose.turn.abs() < 1e-4
            && pose.shift.0.abs() < 1e-4
            && pose.shift.1.abs() < 1e-4
            && pose.alpha > 0.9999
    }

    const ALL: [Motion; 7] = [
        Motion::Spin(-60.),
        Motion::Spin(90.),
        Motion::Swing(-18.),
        Motion::Nudge(2., 0.),
        Motion::Nudge(1.5, -1.5),
        Motion::Pop,
        Motion::Enter,
    ];

    #[test]
    fn every_motion_ends_on_the_icon_at_rest() {
        for motion in ALL {
            assert_eq!(motion.pose(1.), Pose::REST, "{motion:?}");
            assert_eq!(motion.pose(1.5), Pose::REST, "{motion:?}");
            assert!(motion.duration().as_millis() <= 700, "{motion:?}");
            let last = motion.pose(0.995);
            assert!((last.scale.0 - 1.).abs() < 0.02, "{motion:?}");
            assert!((last.scale.1 - 1.).abs() < 0.02, "{motion:?}");
            assert!(last.turn.abs() < 0.03, "{motion:?}");
            assert!(
                last.shift.0.abs() < 0.1 && last.shift.1.abs() < 0.1,
                "{motion:?}"
            );
            assert!(last.alpha > 0.99, "{motion:?}");
        }
    }

    #[test]
    fn going_and_returning_motions_start_at_rest_too() {
        assert!(there_and_back(0.).abs() < 1e-5);
        assert!(there_and_back(0.2) > 0.9);
        for motion in [Motion::Swing(-18.), Motion::Nudge(2., 0.), Motion::Pop] {
            assert!(at_rest(motion.pose(0.)), "{motion:?}");
        }
        let far = (0..=100)
            .map(|step| Motion::Nudge(2., -1.).pose(step as f32 / 100.).shift)
            .fold((0f32, 0f32), |far, at| (far.0.max(at.0), far.1.min(at.1)));
        assert!(far.0 > 1.9 && far.0 <= 2.);
        assert!(far.1 < -0.95 && far.1 >= -1.);
    }

    #[test]
    fn a_spin_starts_on_a_symmetry_and_turns_one_way() {
        let start = Motion::Spin(-60.).pose(0.).turn;
        assert!((start + std::f32::consts::FRAC_PI_3).abs() < 1e-4);
        let mut last = start;
        for step in 1..=100 {
            let turn = Motion::Spin(-60.).pose(step as f32 / 100.).turn;
            assert!(turn >= last - 1e-4);
            last = turn;
        }
    }

    #[test]
    fn a_rename_does_not_play_when_motion_is_off() {
        let control: gpui::SharedString = "test-icon-motion-off".into();
        super::set_enabled(false);
        let (_, plays, since) = super::prepare(&control, "StarIcon");
        assert_eq!(since, None);
        let (_, plays_after_rename, since_after_rename) =
            super::prepare(&control, "StarFilledIcon");
        assert_eq!(since_after_rename, None, "关闭动效时不得请求动画帧");
        assert_eq!(plays, plays_after_rename, "关闭动效时不得增加播放次数");
        super::set_enabled(true);
    }

    #[test]
    fn icons_get_the_motion_their_shape_asks_for() {
        assert_eq!(Motion::of("SettingsIcon"), Motion::Spin(-60.));
        assert_eq!(Motion::of("StarFilledIcon"), Motion::of("StarIcon"));
        assert_eq!(Motion::of("ShareIcon"), Motion::of("OpenIcon"));
        assert_eq!(Motion::of("DeleteIcon"), Motion::Swing(-10.));
        assert_eq!(Motion::of("ChevronDownIcon"), Motion::Nudge(0., 2.));
        assert_eq!(Motion::of("BackIcon"), Motion::Nudge(-2., 0.));
        assert_eq!(Motion::of("CopyIcon"), Motion::Pop);
        assert_eq!(Motion::of("NoSuchIcon"), Motion::Pop);
        // 新图标从小而淡长到原尺寸。
        let coming = Motion::Enter.pose(0.);
        assert!(coming.scale.0 < 0.7 && coming.alpha < 0.1);
    }
}
