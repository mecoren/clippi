//! 帧间隔采样：AC-12「主列表滚动帧率不低于改造前」的量测钩子。
//!
//! 只有环境变量 `CLIPPI_FRAME_TRACE` 指向一个可写文件路径时才工作；
//! 其余时候 `tick` 只查一次缓存的环境变量就返回，不影响正常渲染。
//!
//! 采样方式：每帧记录与上一帧的间隔，按 120 帧一组把 `n / avg / p95 / max`
//! 追加到文件（行格式固定，便于脚本解析）。采样期间主动请求下一帧，让
//! 空闲状态也以显示器刷新率出帧，滚动时的掉帧才会体现在统计里。

use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use gpui::{App, Window};

/// 每组统计的帧数。
const BATCH: u32 = 120;
/// 超过这个间隔视为"停顿"（例如窗口隐藏后重新出帧），不参与统计。
const MAX_GAP: Duration = Duration::from_millis(250);

struct Sample {
    file: std::fs::File,
    last: Option<Instant>,
    frames: u32,
    /// 1 ms 一格的直方图：索引 = 间隔毫秒数（上限 [`MAX_GAP`]）。
    buckets: [u32; 256],
    max_ms: f64,
}

impl Sample {
    fn record(&mut self, now: Instant) {
        let Some(last) = self.last.replace(now) else {
            return;
        };
        let gap = now.duration_since(last);
        if gap > MAX_GAP {
            return;
        }
        let ms = gap.as_secs_f64() * 1000.;
        self.max_ms = self.max_ms.max(ms);
        self.buckets[(ms as usize).min(255)] += 1;
        self.frames += 1;
        if self.frames >= BATCH {
            self.flush();
        }
    }

    fn flush(&mut self) {
        let Some(stats) = summarize(&self.buckets, self.max_ms) else {
            self.reset();
            return;
        };
        let _ = writeln!(
            self.file,
            "n={} avg={:.2} p95={} max={:.2}",
            stats.count, stats.avg_ms, stats.p95_ms, stats.max_ms
        );
        let _ = self.file.flush();
        self.reset();
    }

    fn reset(&mut self) {
        self.frames = 0;
        self.buckets = [0; 256];
        self.max_ms = 0.;
    }
}

/// 一组帧间隔的统计结果。
struct Stats {
    count: u32,
    avg_ms: f64,
    p95_ms: usize,
    max_ms: f64,
}

/// 由直方图算出均值与 p95；没有任何样本时返回 `None`。
///
/// p95 取累计到 95% 的那个桶的上界（桶宽 1 ms），误差 ≤ 1 ms，
/// 足够做改造前后的对比，不必保留全部原始样本。
fn summarize(buckets: &[u32; 256], max_ms: f64) -> Option<Stats> {
    let count: u32 = buckets.iter().sum();
    if count == 0 {
        return None;
    }
    let mut sum = 0f64;
    for (index, hits) in buckets.iter().enumerate() {
        sum += index as f64 * *hits as f64;
    }
    let target = (count as f64 * 0.95).ceil() as u32;
    let mut seen = 0u32;
    let mut p95 = 0usize;
    for (index, hits) in buckets.iter().enumerate() {
        seen += *hits;
        if seen >= target {
            p95 = index + 1;
            break;
        }
    }
    Some(Stats {
        count,
        avg_ms: sum / count as f64,
        p95_ms: p95,
        max_ms,
    })
}

fn sample() -> Option<&'static Mutex<Sample>> {
    static SAMPLE: OnceLock<Option<Mutex<Sample>>> = OnceLock::new();
    SAMPLE
        .get_or_init(|| {
            let path = std::env::var("CLIPPI_FRAME_TRACE").ok()?;
            if path.is_empty() {
                return None;
            }
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .ok()?;
            Some(Mutex::new(Sample {
                file,
                last: None,
                frames: 0,
                buckets: [0; 256],
                max_ms: 0.,
            }))
        })
        .as_ref()
}

/// 每帧调用一次；未开启时是一次 `OnceLock` 读取。
pub fn tick(window: &mut Window, cx: &App) {
    let Some(sample) = sample() else {
        return;
    };
    {
        let mut sample = sample.lock().unwrap_or_else(|e| e.into_inner());
        sample.record(Instant::now());
    }
    // 采样期间持续出帧：否则空闲时没有新帧，统计不到滚动之外的基线。
    if !cx.reduce_motion() {
        window.request_animation_frame();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buckets_with(pairs: &[(usize, u32)]) -> [u32; 256] {
        let mut buckets = [0u32; 256];
        for (index, hits) in pairs {
            buckets[*index] = *hits;
        }
        buckets
    }

    #[test]
    fn empty_histogram_has_no_stats() {
        assert!(summarize(&buckets_with(&[]), 0.).is_none());
    }

    #[test]
    fn average_and_p95_come_from_the_histogram() {
        // 90 帧 7 ms + 10 帧 20 ms：均值 = (630 + 200) / 100，p95 落在 20 ms 桶。
        let buckets = buckets_with(&[(7, 90), (20, 10)]);
        let stats = summarize(&buckets, 20.5).expect("stats");
        assert_eq!(stats.count, 100);
        assert!((stats.avg_ms - 8.3).abs() < 1e-6, "{}", stats.avg_ms);
        assert_eq!(stats.p95_ms, 21);
        assert!((stats.max_ms - 20.5).abs() < 1e-6);
    }

    #[test]
    fn p95_stays_inside_the_majority_bucket_when_it_covers_95_percent() {
        // 96 帧 16 ms + 4 帧 40 ms：95% 的累计仍在 16 ms 桶内。
        let buckets = buckets_with(&[(16, 96), (40, 4)]);
        let stats = summarize(&buckets, 40.).expect("stats");
        assert_eq!(stats.p95_ms, 17);
    }
}
