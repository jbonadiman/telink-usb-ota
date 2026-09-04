mod crc;
mod hid;
mod packet;

use hid::{HidChannel, REPORT_SIZE};
use std::io::Write as _;
use std::time::Duration;

const ACK_TIMEOUT: Duration = Duration::from_millis(500);
const MAX_RETRIES: u32 = 20;

fn usage() -> ! {
    eprintln!("usage:");
    eprintln!("  telink-ota <hidraw-device> version");
    eprintln!("  telink-ota <hidraw-device> flash <firmware.bin> --confirm-flash");
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        usage();
    }
    let device = &args[1];
    let cmd = args[2].as_str();

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
        eprintln!("failed to open {device}: {e}");
        std::process::exit(1);
    });

    match cmd {
        "version" => send_and_report(&mut chan, &packet::version_packet(), "version query"),
        "flash" => flash(&mut chan, &args[3]),
        _ => unreachable!(),
    }
}

fn flash(chan: &mut HidChannel, path: &str) {
    let image = std::fs::read(path).unwrap_or_else(|e| {
        eprintln!("failed to read {path}: {e}");
        std::process::exit(1);
    });
    let total = image.len();
    println!("image: {total} bytes ({} chunks)", total.div_ceil(packet::CHUNK_LEN));

    send_and_report(chan, &packet::start_packet(), "start");

    let mut offset: u32 = 0;
    while (offset as usize) < total {
        let mut data = [0xFFu8; packet::CHUNK_LEN];
        let start = offset as usize;
        let end = (start + packet::CHUNK_LEN).min(total);
        data[..end - start].copy_from_slice(&image[start..end]);

        send_chunk_with_retry(chan, offset, &data);

        offset += packet::CHUNK_LEN as u32;
        print!("\r{:>7}/{total} bytes", (offset as usize).min(total));
        std::io::stdout().flush().ok();
    }
    println!();

    let last_chunk_offset = offset - packet::CHUNK_LEN as u32;
    send_and_report(chan, &packet::finish_packet(last_chunk_offset), "finish");
}

fn send_and_report(chan: &mut HidChannel, pkt: &[u8; REPORT_SIZE], label: &str) {
    chan.write_report(pkt).unwrap_or_else(|e| {
        eprintln!("write failed ({label}): {e}");
        std::process::exit(1);
    });
    match chan.read_report(ACK_TIMEOUT) {
        Some(resp) => println!("{label}: response {resp:02x?}"),
        None => eprintln!("{label}: no response within {ACK_TIMEOUT:?}"),
    }
}

// Retry loop. See packet::chunk_ack_ok for exactly what a success response
// looks like -- confirmed live against real hardware. The write-then-read-ack
// turn-order itself (one write, one read, no pipelining) is a static-analysis
// reconstruction of the vendor updater's own write loop.
fn send_chunk_with_retry(chan: &mut HidChannel, offset: u32, data: &[u8; packet::CHUNK_LEN]) {
    let pkt = packet::chunk_packet(offset, data);

    for attempt in 1..=MAX_RETRIES {
        if let Err(e) = chan.write_report(&pkt) {
            eprintln!("\nwrite failed at offset {offset:#x}: {e}");
            std::process::exit(1);
        }
        if let Some(resp) = chan.read_report(ACK_TIMEOUT) {
            if packet::chunk_ack_ok(&resp, offset) {
                return;
            }
        }
        if attempt == MAX_RETRIES {
            eprintln!("\nno ack for offset {offset:#x} after {MAX_RETRIES} attempts");
            std::process::exit(1);
        }
    }
}
