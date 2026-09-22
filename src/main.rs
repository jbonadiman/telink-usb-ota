mod crc;
mod hid;
mod packet;
mod progress;
mod session;

use hid::HidChannel;
use progress::Console;

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
        Command::Version => session::version(&mut chan, &console),
        Command::Flash { path } => session::flash(&mut chan, &path, &console),
    }
}
