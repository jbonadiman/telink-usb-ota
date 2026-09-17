use std::time::Instant;

const BAR_WIDTH: usize = 30;

// One progress line for a `flash` run: bar, percent, bytes done, rate, ETA.
// Owns the clock and the bar width so callers only report how far they have got.
pub struct Progress {
    total: u64,
    started: Instant,
}

impl Progress {
    pub fn new(total: u64) -> Self {
        Self { total, started: Instant::now() }
    }

    pub fn line(&self, done: u64) -> String {
        Self::render(done, self.total, self.started.elapsed().as_secs_f64())
    }

    fn render(done: u64, total: u64, elapsed_secs: f64) -> String {
        let rate = if elapsed_secs > 0.0 { done as f64 / elapsed_secs } else { 0.0 };
        let eta = eta_secs(done, total, elapsed_secs)
            .map(format_duration)
            .unwrap_or_else(|| "--:--".to_string());
        let pct = (done * 100).checked_div(total).unwrap_or(100);
        format!(
            "{} {pct:>3}% {done:>7}/{total} bytes  {}  ETA {eta}",
            render_bar(done, total, BAR_WIDTH),
            format_rate(rate),
        )
    }
}

// A fixed-width `[####------]` bar. `total == 0` is treated as fully done
// (nothing to do, no division by zero) rather than as a special case callers
// need to handle themselves.
fn render_bar(done: u64, total: u64, width: usize) -> String {
    let filled = if total == 0 {
        width
    } else {
        ((done.min(total) as u128 * width as u128) / total as u128) as usize
    };
    let mut bar = String::with_capacity(width + 2);
    bar.push('[');
    bar.push_str(&"#".repeat(filled));
    bar.push_str(&"-".repeat(width - filled));
    bar.push(']');
    bar
}

fn format_duration(total_secs: u64) -> String {
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

// Linear estimate from the average rate so far. `None` when there isn't
// enough data yet for a rate to mean anything (no progress, or no time
// elapsed to measure it over) -- callers show a placeholder instead of a
// misleading "0s" or a divide-by-zero.
fn eta_secs(done: u64, total: u64, elapsed_secs: f64) -> Option<u64> {
    if done == 0 || elapsed_secs <= 0.0 {
        return None;
    }
    let rate = done as f64 / elapsed_secs;
    let remaining = total.saturating_sub(done) as f64;
    Some((remaining / rate).round() as u64)
}

fn format_rate(bytes_per_sec: f64) -> String {
    if bytes_per_sec < 1024.0 {
        format!("{bytes_per_sec:.0} B/s")
    } else if bytes_per_sec < 1024.0 * 1024.0 {
        format!("{:.1} KB/s", bytes_per_sec / 1024.0)
    } else {
        format!("{:.1} MB/s", bytes_per_sec / (1024.0 * 1024.0))
    }
}

fn colorize(text: &str, code: &str, enabled: bool) -> String {
    if enabled {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

pub fn green(s: &str, enabled: bool) -> String {
    colorize(s, "32", enabled)
}

pub fn yellow(s: &str, enabled: bool) -> String {
    colorize(s, "33", enabled)
}

pub fn red(s: &str, enabled: bool) -> String {
    colorize(s, "31", enabled)
}

pub fn bold(s: &str, enabled: bool) -> String {
    colorize(s, "1", enabled)
}

// NO_COLOR (any value, per no-color.org) always wins over TTY detection.
// Split out from `colors_enabled` below purely so this decision is testable
// without a real terminal or a real environment.
fn should_color(no_color_env: Option<&str>, is_tty: bool) -> bool {
    no_color_env.is_none() && is_tty
}

pub fn colors_enabled() -> bool {
    use std::io::IsTerminal;
    should_color(std::env::var("NO_COLOR").ok().as_deref(), std::io::stdout().is_terminal())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_line_packs_bar_percent_rate_and_eta() {
        assert_eq!(
            Progress::render(50, 100, 10.0),
            "[###############---------------]  50%      50/100 bytes  5 B/s  ETA 00:10"
        );
        assert_eq!(
            Progress::render(0, 100, 0.0),
            "[------------------------------]   0%       0/100 bytes  0 B/s  ETA --:--"
        );
        assert_eq!(
            Progress::render(0, 0, 1.0),
            "[##############################] 100%       0/0 bytes  0 B/s  ETA --:--"
        );
    }

    #[test]
    fn render_bar_shows_partial_fill() {
        assert_eq!(render_bar(0, 100, 10), "[----------]");
        assert_eq!(render_bar(50, 100, 10), "[#####-----]");
        assert_eq!(render_bar(100, 100, 10), "[##########]");
    }

    #[test]
    fn render_bar_treats_zero_total_as_fully_done() {
        assert_eq!(render_bar(0, 0, 10), "[##########]");
    }

    #[test]
    fn format_duration_uses_mm_ss_under_an_hour() {
        assert_eq!(format_duration(5), "00:05");
        assert_eq!(format_duration(65), "01:05");
        assert_eq!(format_duration(3599), "59:59");
    }

    #[test]
    fn format_duration_adds_hours_past_an_hour() {
        assert_eq!(format_duration(3600), "1:00:00");
        assert_eq!(format_duration(3661), "1:01:01");
    }

    #[test]
    fn eta_secs_estimates_from_current_rate() {
        // 50/100 done in 10s -> 5 units/s -> 50 remaining -> 10s left
        assert_eq!(eta_secs(50, 100, 10.0), Some(10));
        assert_eq!(eta_secs(100, 100, 10.0), Some(0));
    }

    #[test]
    fn eta_secs_is_none_without_enough_data_for_a_rate() {
        assert_eq!(eta_secs(0, 100, 5.0), None, "no progress yet, no rate to estimate from");
        assert_eq!(eta_secs(50, 100, 0.0), None, "zero elapsed time, rate is undefined");
    }

    #[test]
    fn format_rate_scales_the_unit() {
        assert_eq!(format_rate(500.0), "500 B/s");
        assert_eq!(format_rate(2048.0), "2.0 KB/s");
        assert_eq!(format_rate(5_242_880.0), "5.0 MB/s");
    }

    #[test]
    fn colorize_wraps_in_ansi_codes_only_when_enabled() {
        assert_eq!(green("ok", true), "\x1b[32mok\x1b[0m");
        assert_eq!(green("ok", false), "ok");
        assert_eq!(yellow("warn", true), "\x1b[33mwarn\x1b[0m");
        assert_eq!(red("err", true), "\x1b[31merr\x1b[0m");
        assert_eq!(bold("hi", true), "\x1b[1mhi\x1b[0m");
    }

    #[test]
    fn should_color_respects_no_color_env_over_tty_detection() {
        assert!(!should_color(Some(""), true), "NO_COLOR set to anything disables color");
        assert!(!should_color(Some("1"), true));
        assert!(should_color(None, true));
        assert!(!should_color(None, false), "not a terminal, e.g. piped output");
    }
}
