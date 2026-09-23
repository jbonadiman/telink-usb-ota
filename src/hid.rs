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
// path that is not a hidraw node is refused before anything is written. A
// regular file would lose its first report to the version query, a block device
// would lose the start of the disk, and another character device (e.g. /dev/mem
// or /dev/port, both root-only) would take a 33-byte write to an interface this
// tool has no business touching. The check is on the opened handle, so a
// symlink swapped in between a separate check and the open cannot get past it.
#[cfg(target_os = "linux")]
fn is_hidraw_device(file: &File) -> std::io::Result<bool> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    let md = file.metadata()?;
    if !md.file_type().is_char_device() {
        return Ok(false);
    }
    // hidraw has no fixed major -- it is allocated at boot, and on a current
    // kernel 241 is cec while hidraw gets its own -- so read the owning class
    // from sysfs instead of hardcoding a number. The rdev comes from the opened
    // fd, so a symlink cannot point the check at a different node than the
    // write.
    let rdev = md.rdev();
    let major = (rdev >> 8) & 0xfff;
    let minor = (rdev & 0xff) | ((rdev >> 12) & 0xfff00);
    let link = format!("/sys/dev/char/{major}:{minor}/subsystem");
    Ok(std::fs::read_link(link).map(|p| p.ends_with("hidraw")).unwrap_or(false))
}

// hidraw is Linux-only and the Windows cross-build has no character/block
// distinction, so the check is a no-op on every other target.
#[cfg(not(target_os = "linux"))]
fn is_hidraw_device(_file: &File) -> std::io::Result<bool> {
    Ok(true)
}

impl HidChannel {
    pub fn open(path: &str) -> std::io::Result<Self> {
        let writer = OpenOptions::new().read(true).write(true).open(path)?;
        if !is_hidraw_device(&writer)? {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "not a hidraw device (expected a /dev/hidraw* node)",
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
