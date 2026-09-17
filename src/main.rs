mod crc;
mod hid;
mod packet;
mod progress;

use hid::{HidChannel, REPORT_SIZE};
use progress::Console;
use std::io::Write as _;
use std::time::{Duration, Instant};

const ACK_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_RETRIES: u32 = 20;
const BAR_WIDTH: usize = 30;

enum Command {
    Version,
    Flash { path: String },
}

struct Options {
    device: String,
    command: Command,
    verbose: bool,
}

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

// Parses and validates the whole command line *before* anything opens the
// device, so an unconfirmed `flash` invocation never touches it at all.
fn parse_args(args: &[String]) -> Options {
    if args.len() < 3 {
        usage();
    }
    let verbose = args.iter().any(|a| a == "--verbose" || a == "-v");
    let command = match args[2].as_str() {
        "version" => Command::Version,
        "flash" => {
            if args.len() < 4 {
                usage();
            }
            if !args.iter().any(|a| a == "--confirm-flash") {
                eprintln!("refusing to flash without --confirm-flash (this writes to the device)");
                std::process::exit(2);
            }
            Command::Flash { path: args[3].clone() }
        }
        _ => usage(),
    };
    Options { device: args[1].clone(), command, verbose }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let opts = parse_args(&args);
    let console = Console::new(opts.verbose);

    let mut chan = HidChannel::open(&opts.device)
        .unwrap_or_else(|e| console.fail(&format!("failed to open {}: {e}", opts.device)));

    match opts.command {
        Command::Version => query_version(&mut chan, &console),
        Command::Flash { path } => flash(&mut chan, &path, &console),
    }
}

fn query_version(chan: &mut HidChannel, console: &Console) {
    let pkt = packet::version_packet();
    if let Err(e) = chan.write_report(&pkt) {
        console.fail(&format!("write failed: {e}"));
    }
    match chan.read_report(ACK_TIMEOUT) {
        Some(resp) => println!("response {resp:02x?}"),
        None => eprintln!("{}", console.yellow(&format!("no response within {ACK_TIMEOUT:?}"))),
    }
}

fn flash(chan: &mut HidChannel, path: &str, console: &Console) {
    let image = std::fs::read(path)
        .unwrap_or_else(|e| console.fail(&format!("failed to read {path}: {e}")));
    let total = image.len();
    if let Err(why) = packet::image_len_error(total) {
        console.fail(&format!("{path}: {why}"));
    }
    println!(
        "{} {total} bytes ({} chunks)",
        console.bold("image:"),
        total.div_ceil(packet::CHUNK_LEN)
    );

    send_and_report(chan, &packet::start_packet(), "start", console);

    let started = Instant::now();
    let mut offset: u32 = 0;
    while (offset as usize) < total {
        let mut data = [0xFFu8; packet::CHUNK_LEN];
        let start = offset as usize;
        let end = (start + packet::CHUNK_LEN).min(total);
        data[..end - start].copy_from_slice(&image[start..end]);

        send_chunk_with_retry(chan, offset, &data, console);

        offset += packet::CHUNK_LEN as u32;
        let done = (offset as usize).min(total) as u64;
        print!(
            "\r{}",
            progress::render_progress(done, total as u64, started.elapsed().as_secs_f64(), BAR_WIDTH)
        );
        std::io::stdout().flush().ok();
    }
    println!();
    println!("{}", console.green("all chunks written"));

    let last_chunk_offset = offset - packet::CHUNK_LEN as u32;
    send_and_report(chan, &packet::finish_packet(last_chunk_offset), "finish", console);
}

fn send_and_report(chan: &mut HidChannel, pkt: &[u8; REPORT_SIZE], label: &str, console: &Console) {
    if let Err(e) = chan.write_report(pkt) {
        console.fail(&format!("write failed ({label}): {e}"));
    }
    match chan.read_report(ACK_TIMEOUT) {
        Some(resp) => {
            println!("{label}: {}", console.green("ok"));
            if console.verbose {
                println!("  response {resp:02x?}");
            }
        }
        None => println!("{label}: {}", console.yellow(&format!("no response within {ACK_TIMEOUT:?}"))),
    }
}

// Retry loop. See packet::chunk_ack_ok for exactly what a success response
// looks like -- confirmed live against real hardware. The write-then-read-ack
// turn-order itself (one write, one read, no pipelining) is a static-analysis
// reconstruction of the vendor updater's own write loop.
fn send_chunk_with_retry(chan: &mut HidChannel, offset: u32, data: &[u8; packet::CHUNK_LEN], console: &Console) {
    let pkt = packet::chunk_packet(offset, data);

    for attempt in 1..=MAX_RETRIES {
        if let Err(e) = chan.write_report(&pkt) {
            eprintln!();
            console.fail(&format!("write failed at offset {offset:#x}: {e}"));
        }
        if let Some(resp) = chan.read_report(ACK_TIMEOUT) {
            if packet::chunk_ack_ok(&resp, offset) {
                return;
            }
        }
        if console.verbose && attempt < MAX_RETRIES {
            eprintln!("\n{}", console.yellow(&format!("retrying offset {offset:#x} (attempt {attempt})")));
        }
        if attempt == MAX_RETRIES {
            eprintln!();
            console.fail(&format!("no ack for offset {offset:#x} after {MAX_RETRIES} attempts"));
        }
    }
}
