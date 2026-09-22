use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

// Report ID (1 byte) + 32-byte report, matching the live device's HID report
// descriptor and the vendor updater's own ReadFile/WriteFile length of 0x21.
pub const REPORT_SIZE: usize = 33;

pub struct HidChannel {
    writer: File,
    rx: Receiver<[u8; REPORT_SIZE]>,
}

// The path is opened read-write and every command writes a report to it, so a
// path that is not a character device is refused before anything is written.
// A regular file would lose its first report to the version query, and a block
// device would lose the start of the disk.
#[cfg(unix)]
fn is_character_device(file: &File) -> std::io::Result<bool> {
    use std::os::unix::fs::FileTypeExt;
    Ok(file.metadata()?.file_type().is_char_device())
}

// Windows has no character/block distinction, and this tool's hidraw path is
// Linux-only, so the check stays Unix-only to keep the cross-build compiling.
#[cfg(not(unix))]
fn is_character_device(_file: &File) -> std::io::Result<bool> {
    Ok(true)
}

impl HidChannel {
    pub fn open(path: &str) -> std::io::Result<Self> {
        let writer = OpenOptions::new().read(true).write(true).open(path)?;
        if !is_character_device(&writer)? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "not a character device (expected a /dev/hidraw* node)",
            ));
        }
        let mut reader = writer.try_clone()?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || loop {
            let mut buf = [0u8; REPORT_SIZE];
            match reader.read(&mut buf) {
                Ok(n) if n > 0 => {
                    if tx.send(buf).is_err() {
                        break;
                    }
                }
                _ => break,
            }
        });
        Ok(Self { writer, rx })
    }

    pub fn write_report(&mut self, report: &[u8; REPORT_SIZE]) -> std::io::Result<()> {
        self.writer.write_all(report)
    }

    pub fn read_report(&self, timeout: Duration) -> Option<[u8; REPORT_SIZE]> {
        self.rx.recv_timeout(timeout).ok()
    }
}
