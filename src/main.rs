//! Claude Code status line: one row, fitted to terminal width.
//! Input fields: <https://code.claude.com/docs/en/statusline#available-data>
//! Every field may be absent (`rate_limits` only for subscribers, after the first response).

use std::io::Read;
use std::time::Duration;

use chrono::{Datelike, Local, TimeZone};
use serde::Deserialize;

#[derive(Deserialize, Default)]
#[serde(default)]
struct Input {
    model: Model,
    effort: Effort,
    fast_mode: bool,
    cwd: Option<String>,
    workspace: Workspace,
    context_window: CtxWindow,
    rate_limits: RateLimits,
    cost: Cost,
    prompt_cache: PromptCache,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Model {
    display_name: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Effort {
    level: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Workspace {
    current_dir: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CtxWindow {
    used_percentage: Option<f64>,
    context_window_size: Option<u64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RateLimits {
    five_hour: Option<Window>,
    seven_day: Option<Window>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Window {
    used_percentage: Option<f64>,
    resets_at: Option<i64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
#[expect(clippy::struct_field_names, reason = "mirrors the JSON field names")]
struct Cost {
    total_cost_usd: Option<f64>,
    total_duration_ms: u64,
    total_lines_added: u64,
    total_lines_removed: u64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct PromptCache {
    hit_ratio: Option<f64>,
}

const R: &str = "\x1b[0m";
const DIM: &str = "\x1b[2m";
const BOLD_CYAN: &str = "\x1b[1;36m";
const MAG: &str = "\x1b[35m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";

/// A segment kept twice: with ANSI colors for output, plain for width math.
#[derive(Default)]
struct Seg {
    ansi: String,
    plain: String,
}

impl Seg {
    fn add(mut self, color: &str, text: &str) -> Self {
        self.ansi.push_str(color);
        self.ansi.push_str(text);
        self.ansi.push_str(R);
        self.plain.push_str(text);
        self
    }
    /// Display width. All glyphs used here are single-cell (no emoji).
    fn width(&self) -> usize {
        self.plain.chars().count()
    }
}

fn pct_color(p: f64) -> &'static str {
    if p >= 80.0 {
        RED
    } else if p >= 50.0 {
        YELLOW
    } else {
        GREEN
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to 0..=100, so the cell count is 0..=5"
)]
fn bar(p: f64) -> String {
    let filled = ((p.clamp(0.0, 100.0) + 10.0) / 20.0) as usize; // 5 cells, rounded
    (0..5)
        .map(|i| if i < filled.min(5) { '█' } else { '░' })
        .collect()
}

fn reset_time(epoch: i64, with_day: bool) -> Option<String> {
    const DAYS: [&str; 7] = ["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Нд"];
    let t = Local.timestamp_opt(epoch, 0).single()?;
    Some(if with_day {
        format!(
            "{} {}",
            DAYS[t.weekday().num_days_from_monday() as usize],
            t.format("%H:%M")
        )
    } else {
        t.format("%H:%M").to_string()
    })
}

fn limit_seg(label: &str, w: &Window, with_day: bool) -> Option<Seg> {
    let p = w.used_percentage?;
    let mut s = Seg::default()
        .add(DIM, label)
        .add("", " ")
        .add(pct_color(p), &format!("{p:.0}%"));
    if let Some(t) = w.resets_at.and_then(|e| reset_time(e, with_day)) {
        s = s.add(DIM, &format!(" ↻{t}"));
    }
    Some(s)
}

/// Branch and dirty flag via gix: no `git` subprocess. The dirty check runs on its own thread
/// with a deadline, so a huge repo can't stall the status line (branch still shows).
fn git_info(dir: &str) -> Option<(String, bool)> {
    let repo = gix::discover(dir).ok()?;
    let branch = match repo.head_name().ok()? {
        Some(name) => name.shorten().to_string(),
        None => repo.head_id().ok()?.shorten().ok()?.to_string(), // detached HEAD
    };
    let (tx, rx) = std::sync::mpsc::channel();
    // Repository is !Send without gix "parallel" (which measured slower); reopen in the thread.
    let path = repo.path().to_owned();
    std::thread::spawn(move || {
        let dirty = gix::open(path)
            .ok()
            .and_then(|r| r.is_dirty().ok())
            .unwrap_or(false);
        let _ = tx.send(dirty);
    });
    let dirty = rx.recv_timeout(Duration::from_millis(500)).unwrap_or(false);
    Some((branch, dirty))
}

/// Segments in priority order; the lowest ones are dropped when the line is too wide.
fn build(input: &Input, git_info: Option<(String, bool)>) -> Vec<Seg> {
    let mut segs = Vec::new();

    let mut s = Seg::default().add(
        BOLD_CYAN,
        input.model.display_name.as_deref().unwrap_or("?"),
    );
    if let Some(effort) = &input.effort.level {
        s = s.add(DIM, &format!(" {effort}"));
    }
    if input.fast_mode {
        s = s.add(MAG, " fast");
    }
    segs.push(s);

    if let Some(p) = input.context_window.used_percentage {
        let mut s = Seg::default()
            .add(DIM, "ctx ")
            .add(pct_color(p), &format!("{} {p:.0}%", bar(p)));
        if let Some(size) = input.context_window.context_window_size {
            let size = if size >= 1_000_000 {
                format!("/{}M", size / 1_000_000)
            } else {
                format!("/{}k", size / 1000)
            };
            s = s.add(DIM, &size);
        }
        segs.push(s);
    }

    let h5 = input
        .rate_limits
        .five_hour
        .as_ref()
        .and_then(|w| limit_seg("5h", w, false));
    let d7 = input
        .rate_limits
        .seven_day
        .as_ref()
        .and_then(|w| limit_seg("7d", w, true));
    let subscribed = h5.is_some() || d7.is_some();
    segs.extend(h5);
    segs.extend(d7);
    // List-price estimate; meaningless on a subscription, so only shown without rate limits.
    if let (false, Some(cost)) = (subscribed, input.cost.total_cost_usd) {
        segs.push(Seg::default().add(DIM, "≈$").add("", &format!("{cost:.2}")));
    }

    if let Some(dir) = input.workspace.current_dir.as_ref().or(input.cwd.as_ref()) {
        let home = std::env::var("HOME").unwrap_or_default();
        let shown = match dir.strip_prefix(&home) {
            Some(rest) if !home.is_empty() => format!("~{rest}"),
            _ => dir.clone(),
        };
        let mut s = Seg::default().add(DIM, &shown);
        if let Some((branch, dirty)) = git_info {
            s = s.add(MAG, &format!("  {branch}"));
            if dirty {
                s = s.add(YELLOW, "*");
            }
        }
        segs.push(s);
    }

    if let Some(hit) = input.prompt_cache.hit_ratio {
        segs.push(
            Seg::default()
                .add(DIM, "cache ")
                .add("", &format!("{:.0}%", hit * 100.0)),
        );
    }

    let mins = input.cost.total_duration_ms / 60_000;
    segs.push(Seg::default().add("", &format!("{}h{:02}m", mins / 60, mins % 60)));

    let (added, removed) = (input.cost.total_lines_added, input.cost.total_lines_removed);
    if added > 0 || removed > 0 {
        segs.push(
            Seg::default()
                .add(GREEN, &format!("+{added}"))
                .add(DIM, "/")
                .add(RED, &format!("-{removed}")),
        );
    }
    segs
}

const SEP_PLAIN: &str = " │ ";

/// Joins segments in order, stopping at the first one that would overflow `max` cells.
fn fit(segs: &[Seg], max: usize) -> String {
    let sep = format!(" {DIM}│{R} ");
    let (mut out, mut len) = (String::new(), 0);
    for s in segs {
        let sep_width = if len > 0 {
            SEP_PLAIN.chars().count()
        } else {
            0
        };
        let add = s.width() + sep_width;
        if len + add > max {
            break;
        }
        if len > 0 {
            out.push_str(&sep);
        }
        out.push_str(&s.ansi);
        len += add;
    }
    out
}

fn main() {
    let mut raw = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw);
    // Bad input must still print something: a blank status line hides the failure.
    let input: Input = serde_json::from_str(&raw).unwrap_or_default();

    let dir = input
        .workspace
        .current_dir
        .clone()
        .or_else(|| input.cwd.clone());
    let git_info = dir.as_deref().and_then(git_info);

    // COLUMNS is set by Claude Code (tput can't see the terminal); keep a margin for its padding.
    let cols = std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.parse::<usize>().ok())
        .unwrap_or(200);
    println!("{}", fit(&build(&input, git_info), cols.saturating_sub(4)));
    // Don't wait for a dirty-check thread that missed its deadline.
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(s: &str) -> String {
        let mut out = String::new();
        let mut esc = false;
        for c in s.chars() {
            match (esc, c) {
                (false, '\x1b') => esc = true,
                (true, 'm') => esc = false,
                (true, _) => {}
                _ => out.push(c),
            }
        }
        out
    }

    #[test]
    fn empty_input_prints_model_and_time() {
        let i: Input = serde_json::from_str("{}").unwrap();
        assert_eq!(strip(&fit(&build(&i, None), 200)), "? │ 0h00m");
    }

    #[test]
    fn cost_hidden_on_subscription() {
        let j = r#"{"cost":{"total_cost_usd":1.5},
            "rate_limits":{"five_hour":{"used_percentage":23.5}}}"#;
        let line = strip(&fit(&build(&serde_json::from_str(j).unwrap(), None), 200));
        assert!(line.contains("5h 24%") && !line.contains('$'), "{line}");
        let j = r#"{"cost":{"total_cost_usd":0.1234}}"#;
        assert!(
            strip(&fit(&build(&serde_json::from_str(j).unwrap(), None), 200)).contains("≈$0.12")
        );
    }

    #[test]
    fn narrow_terminal_drops_low_priority_and_never_overflows() {
        let j = r#"{"model":{"display_name":"Opus 5.5"},"effort":{"level":"high"},
            "context_window":{"used_percentage":63,"context_window_size":1000000},
            "cost":{"total_duration_ms":5400000,"total_lines_added":120,"total_lines_removed":7}}"#;
        let i: Input = serde_json::from_str(j).unwrap();
        for max in [10, 30, 45, 200] {
            let line = strip(&fit(&build(&i, None), max));
            assert!(line.chars().count() <= max, "{max}: {line}");
        }
        assert_eq!(strip(&fit(&build(&i, None), 31)), "Opus 5.5 high"); // 32 cells needed for ctx
        assert_eq!(
            strip(&fit(&build(&i, None), 32)),
            "Opus 5.5 high │ ctx ███░░ 63%/1M"
        );
    }

    #[test]
    fn bar_rounds_and_clamps() {
        assert_eq!(bar(0.0), "░░░░░");
        assert_eq!(bar(63.0), "███░░");
        assert_eq!(bar(150.0), "█████");
    }
}
