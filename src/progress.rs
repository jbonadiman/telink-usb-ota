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

// One line of live transfer progress: bar, percent, bytes, rate, ETA.
pub fn render_progress(done: u64, total: u64, elapsed_secs: f64, width: usize) -> String {
    let rate = if elapsed_secs > 0.0 { done as f64 / elapsed_secs } else { 0.0 };
    let eta = eta_secs(rate, total.saturating_sub(done))
        .map(format_duration)
        .unwrap_or_else(|| "--:--".to_string());
    let pct = (done * 100).checked_div(total).unwrap_or(100);
    format!(
        "{} {pct:>3}% {done:>7}/{total} bytes  {}  ETA {eta}",
        render_bar(done, total, width),
        format_rate(rate),
    )
}

// Linear estimate from the average rate so far. `None` when there isn't
// enough data yet for a rate to mean anything (no progress, or no time
// elapsed to measure it over) -- callers show a placeholder instead of a
// misleading "0s" or a divide-by-zero.
fn eta_secs(rate: f64, remaining: u64) -> Option<u64> {
    if rate <= 0.0 {
        return None;
    }
    Some((remaining as f64 / rate).round() as u64)
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

// The tool's terminal settings, decided once and threaded as one value
// instead of a `color` flag on every function. All fatal errors exit through
// `fail`, so there is a single exit path and a single color decision, and
// every colored string comes from one of these methods.
pub struct Console {
    color: bool,
    pub verbose: bool,
}

impl Console {
    pub fn new(verbose: bool) -> Self {
        Self { color: colors_enabled(), verbose }
    }

    pub fn green(&self, s: &str) -> String {
        colorize(s, "32", self.color)
    }

    pub fn yellow(&self, s: &str) -> String {
        colorize(s, "33", self.color)
    }

    pub fn bold(&self, s: &str) -> String {
        colorize(s, "1", self.color)
    }

    pub fn fail(&self, msg: &str) -> ! {
        eprintln!("{}", colorize(msg, "31", self.color));
        std::process::exit(1);
    }
}

// NO_COLOR (any value, per no-color.org) always wins over TTY detection.
// Split out from `colors_enabled` below purely so this decision is testable
// without a real terminal or a real environment.
fn should_color(no_color_env: Option<&str>, is_tty: bool) -> bool {
    no_color_env.is_none() && is_tty
}

fn colors_enabled() -> bool {
    use std::io::IsTerminal;
    should_color(std::env::var("NO_COLOR").ok().as_deref(), std::io::stdout().is_terminal())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // 5 units/s with 50 remaining -> 10s left
        assert_eq!(eta_secs(5.0, 50), Some(10));
        assert_eq!(eta_secs(10.0, 0), Some(0));
    }

    #[test]
    fn eta_secs_is_none_without_a_rate() {
        assert_eq!(eta_secs(0.0, 100), None, "no progress yet, no rate to estimate from");
    }

    #[test]
    fn render_progress_shows_bar_percent_bytes_rate_and_eta() {
        let line = render_progress(50, 100, 10.0, 10);
        assert!(line.starts_with("[#####-----]"), "{line}");
        assert!(line.contains(" 50%"), "{line}");
        assert!(line.contains("50/100 bytes"), "{line}");
        assert!(line.contains("5 B/s"), "{line}");
        assert!(line.ends_with("ETA 00:10"), "{line}");
    }

    #[test]
    fn render_progress_handles_an_empty_transfer() {
        let line = render_progress(0, 0, 1.0, 10);
        assert!(line.contains("100%"), "{line}");
        assert!(line.ends_with("ETA --:--"), "{line}");
    }

    #[test]
    fn render_progress_shows_no_eta_without_elapsed_time() {
        assert!(render_progress(50, 100, 0.0, 10).ends_with("ETA --:--"));
    }

    #[test]
    fn format_rate_scales_the_unit() {
        assert_eq!(format_rate(500.0), "500 B/s");
        assert_eq!(format_rate(2048.0), "2.0 KB/s");
        assert_eq!(format_rate(5_242_880.0), "5.0 MB/s");
    }

    #[test]
    fn colorize_wraps_in_ansi_codes_only_when_enabled() {
        assert_eq!(colorize("ok", "32", true), "\x1b[32mok\x1b[0m");
        assert_eq!(colorize("ok", "32", false), "ok");
        assert_eq!(colorize("err", "31", true), "\x1b[31merr\x1b[0m");
    }

    #[test]
    fn console_wraps_each_method_in_its_own_code() {
        let color = Console { color: true, verbose: false };
        assert_eq!(color.green("ok"), "\x1b[32mok\x1b[0m");
        assert_eq!(color.yellow("warn"), "\x1b[33mwarn\x1b[0m");
        assert_eq!(color.bold("hi"), "\x1b[1mhi\x1b[0m");
        assert_eq!(Console { color: false, verbose: false }.green("ok"), "ok");
    }

    #[test]
    fn should_color_respects_no_color_env_over_tty_detection() {
        assert!(!should_color(Some(""), true), "NO_COLOR set to anything disables color");
        assert!(!should_color(Some("1"), true));
        assert!(should_color(None, true));
        assert!(!should_color(None, false), "not a terminal, e.g. piped output");
    }
}
