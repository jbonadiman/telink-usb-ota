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

impl HidChannel {
    pub fn open(path: &str) -> std::io::Result<Self> {
        let writer = OpenOptions::new().read(true).write(true).open(path)?;
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
