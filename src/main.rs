mod crc;
mod hid;
mod packet;
mod progress;

use hid::{HidChannel, REPORT_SIZE};
use progress::{bold, format_duration, format_rate, green, red, render_bar, yellow};
use std::io::Write as _;
use std::time::{Duration, Instant};

const ACK_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_RETRIES: u32 = 20;
const BAR_WIDTH: usize = 30;

fn usage() -> ! {
    eprintln!("usage:");
    eprintln!("  telink-ota <hidraw-device> version");
    eprintln!("  telink-ota <hidraw-device> flash <firmware.bin> --confirm-flash");
    eprintln!();
    eprintln!("flags:");
    eprintln!("  --confirm-flash   required by flash (writes to the device)");
    eprintln!("  --verbose, -v     print raw report bytes for every exchange");
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        usage();
    }
    let device = &args[1];
    let cmd = args[2].as_str();
    let verbose = args.iter().any(|a| a == "--verbose" || a == "-v");
    let color = progress::colors_enabled();

    // Validate everything about the requested command *before* opening the
    // device, so an unconfirmed `flash` invocation never touches it at all.
    match cmd {
        "version" => {}
        "flash" => {
            if args.len() < 4 {
                usage();
            }
            if !args.iter().any(|a| a == "--confirm-flash") {
                eprintln!("refusing to flash without --confirm-flash (this writes to the device)");
                std::process::exit(2);
            }
        }
        _ => usage(),
    }

    let mut chan = HidChannel::open(device).unwrap_or_else(|e| {
        eprintln!("{}", red(&format!("failed to open {device}: {e}"), color));
        std::process::exit(1);
    });

    if cmd == "version" {
        let pkt = packet::version_packet();
        chan.write_report(&pkt).unwrap_or_else(|e| {
            eprintln!("{}", red(&format!("write failed: {e}"), color));
            std::process::exit(1);
        });
        match chan.read_report(ACK_TIMEOUT) {
            Some(resp) => println!("response {resp:02x?}"),
            None => eprintln!("{}", yellow(&format!("no response within {ACK_TIMEOUT:?}"), color)),
        }
    } else {
        flash(&mut chan, &args[3], verbose, color);
    }
}

fn flash(chan: &mut HidChannel, path: &str, verbose: bool, color: bool) {
    let image = std::fs::read(path).unwrap_or_else(|e| {
        eprintln!("{}", red(&format!("failed to read {path}: {e}"), color));
        std::process::exit(1);
    });
    let total = image.len();
    if let Err(why) = packet::image_len_error(total) {
        eprintln!("{}", red(&format!("{path}: {why}"), color));
        std::process::exit(1);
    }
    println!(
        "{} {total} bytes ({} chunks)",
        bold("image:", color),
        total.div_ceil(packet::CHUNK_LEN)
    );

    send_and_report(chan, &packet::start_packet(), "start", verbose, color);

    let started = Instant::now();
    let mut offset: u32 = 0;
    while (offset as usize) < total {
        let mut data = [0xFFu8; packet::CHUNK_LEN];
        let start = offset as usize;
        let end = (start + packet::CHUNK_LEN).min(total);
        data[..end - start].copy_from_slice(&image[start..end]);

        send_chunk_with_retry(chan, offset, &data, verbose, color);

        offset += packet::CHUNK_LEN as u32;
        let done = (offset as usize).min(total) as u64;
        let elapsed = started.elapsed().as_secs_f64();
        let rate = if elapsed > 0.0 { done as f64 / elapsed } else { 0.0 };
        let eta = progress::eta_secs(done, total as u64, elapsed)
            .map(format_duration)
            .unwrap_or_else(|| "--:--".to_string());
        let pct = if total == 0 { 100 } else { done * 100 / total as u64 };
        print!(
            "\r{} {pct:>3}% {done:>7}/{total} bytes  {}  ETA {eta}",
            render_bar(done, total as u64, BAR_WIDTH),
            format_rate(rate),
        );
        std::io::stdout().flush().ok();
    }
    println!();
    println!("{}", green("all chunks written", color));

    let last_chunk_offset = offset - packet::CHUNK_LEN as u32;
    send_and_report(chan, &packet::finish_packet(last_chunk_offset), "finish", verbose, color);
}

fn send_and_report(chan: &mut HidChannel, pkt: &[u8; REPORT_SIZE], label: &str, verbose: bool, color: bool) {
    chan.write_report(pkt).unwrap_or_else(|e| {
        eprintln!("{}", red(&format!("write failed ({label}): {e}"), color));
        std::process::exit(1);
    });
    match chan.read_report(ACK_TIMEOUT) {
        Some(resp) => {
            println!("{label}: {}", green("ok", color));
            if verbose {
                println!("  response {resp:02x?}");
            }
        }
        None => println!("{label}: {}", yellow(&format!("no response within {ACK_TIMEOUT:?}"), color)),
    }
}

// Retry loop. See packet::chunk_ack_ok for exactly what a success response
// looks like -- confirmed live against real hardware. The write-then-read-ack
// turn-order itself (one write, one read, no pipelining) is a static-analysis
// reconstruction of the vendor updater's own write loop.
fn send_chunk_with_retry(chan: &mut HidChannel, offset: u32, data: &[u8; packet::CHUNK_LEN], verbose: bool, color: bool) {
    let pkt = packet::chunk_packet(offset, data);

    for attempt in 1..=MAX_RETRIES {
        if let Err(e) = chan.write_report(&pkt) {
            eprintln!("\n{}", red(&format!("write failed at offset {offset:#x}: {e}"), color));
            std::process::exit(1);
        }
        if chan.read_report(ACK_TIMEOUT).is_some_and(|resp| packet::chunk_ack_ok(&resp, offset)) {
            return;
        }
        if attempt == MAX_RETRIES {
            eprintln!("\n{}", red(&format!("no ack for offset {offset:#x} after {MAX_RETRIES} attempts"), color));
            std::process::exit(1);
        }
        if verbose {
            eprintln!("\n{}", yellow(&format!("retrying offset {offset:#x} (attempt {attempt})"), color));
        }
    }
}
