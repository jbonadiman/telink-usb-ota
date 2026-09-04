use crate::crc::crc16_modbus;
use crate::hid::REPORT_SIZE;

const REPORT_ID: u8 = 0x05;
// Fixed 4-byte magic seen in every outgoing report in the vendor updater's own
// write loop. Not validated by the device firmware itself (per disassembly of
// the OTA command dispatcher) -- this is host->device framing only, kept for
// wire compatibility with the real updater tool.
const MAGIC: [u8; 4] = [0x04, 0x52, 0x28, 0x00];

pub const CHUNK_LEN: usize = 16;

fn build_report(payload: &[u8]) -> [u8; REPORT_SIZE] {
    assert!(payload.len() + 9 <= REPORT_SIZE, "payload too long for a single report");
    let len = payload.len();
    let mut r = [0u8; REPORT_SIZE];
    r[0] = REPORT_ID;
    r[1] = (len + 9) as u8;
    r[2] = 0x01;
    r[3] = (len + 7) as u8;
    r[4] = (len + 3) as u8;
    r[5..9].copy_from_slice(&MAGIC);
    r[9..9 + len].copy_from_slice(payload);
    r
}

// Data chunk: 2-byte address (offset >> 4) + 16 payload bytes + CRC-16/MODBUS
// over those 18 bytes. Device-confirmed via disassembly of the OTA command
// dispatcher: it reads this same 18-byte window and the CRC immediately after
// it, and rejects the packet on mismatch.
pub fn chunk_packet(offset: u32, data: &[u8; CHUNK_LEN]) -> [u8; REPORT_SIZE] {
    let addr = (offset >> 4) as u16;
    let mut payload = [0u8; 20];
    payload[0..2].copy_from_slice(&addr.to_le_bytes());
    payload[2..18].copy_from_slice(data);
    let crc = crc16_modbus(&payload[0..18]);
    payload[18..20].copy_from_slice(&crc.to_le_bytes());
    build_report(&payload)
}

// Control packets reuse the address field as a 16-bit little-endian opcode
// (0xFF00/0xFF01/0xFF02 all have a 0xFF high byte, so they can never collide
// with a real chunk address on a 128 KB image). Byte order confirmed against
// the device's own dispatch code: it reads the low byte first, then the high
// byte, i.e. little-endian -- matching the same field's encoding for ordinary
// chunk addresses.
fn control_packet(opcode: u16, extra: &[u8]) -> [u8; REPORT_SIZE] {
    let mut payload = Vec::with_capacity(2 + extra.len());
    payload.extend_from_slice(&opcode.to_le_bytes());
    payload.extend_from_slice(extra);
    build_report(&payload)
}

// CMD_OTA_FW_VERSION. Device-confirmed opcode, response format not decoded.
pub fn version_packet() -> [u8; REPORT_SIZE] {
    control_packet(0xFF00, &[])
}

// CMD_OTA_START. Device-confirmed opcode (resets the device's SRAM-resident
// OTA state struct). NOT seen in the original host-side trace of the vendor
// tool's write loop -- sending it is the semantically correct way to reset the
// device's last-index tracking, but whether the real vendor tool relies on
// this or on some other implicit reset is unconfirmed.
pub fn start_packet() -> [u8; REPORT_SIZE] {
    control_packet(0xFF01, &[])
}

// CMD_OTA_END. addr/complement fields device-confirmed against the OTA
// dispatcher's END branch (bytes carry addr_lo, addr_hi, !addr_lo, !addr_hi).
pub fn finish_packet(last_chunk_offset: u32) -> [u8; REPORT_SIZE] {
    let addr = (last_chunk_offset >> 4) as u16;
    let [lo, hi] = addr.to_le_bytes();
    control_packet(0xFF02, &[lo, hi, !lo, !hi])
}

// Whether a chunk-write response indicates success. Live-confirmed against a
// real device: bytes [9,10] carry the *next expected* chunk index (little-
// endian), i.e. one past the address just sent -- not an echo of that address,
// as earlier static analysis had assumed.
pub fn chunk_ack_ok(resp: &[u8; REPORT_SIZE], offset: u32) -> bool {
    let next_expected = (offset >> 4) as u16 + 1;
    resp[2] == 0x01 && u16::from_le_bytes([resp[9], resp[10]]) == next_expected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_packet_matches_documented_layout() {
        let data = [0xAAu8; CHUNK_LEN];
        let r = chunk_packet(0x30, &data); // offset 0x30 -> address 0x0003
        assert_eq!(r[0], 0x05);
        assert_eq!(r[1], 0x1D); // len(20) + 9
        assert_eq!(r[2], 0x01);
        assert_eq!(r[3], 0x1B); // len(20) + 7
        assert_eq!(r[4], 0x17); // len(20) + 3
        assert_eq!(&r[5..9], &MAGIC);
        assert_eq!(&r[9..11], &[0x03, 0x00]); // address, LE16
        assert_eq!(&r[11..27], &data);
        let crc = crc16_modbus(&r[9..27]);
        assert_eq!(&r[27..29], &crc.to_le_bytes());
    }

    #[test]
    fn control_opcodes_are_little_endian() {
        // 0xFF00/0xFF01/0xFF02 all decode with a 0xFF high byte and a low byte
        // in {0,1,2}, matching the device's own
        // `(pkt[0x0e] << 8) | pkt[0x0d]`.
        assert_eq!(&version_packet()[9..11], &[0x00, 0xFF]);
        assert_eq!(&start_packet()[9..11], &[0x01, 0xFF]);
        assert_eq!(&finish_packet(0x30)[9..11], &[0x02, 0xFF]);
    }

    #[test]
    fn finish_packet_carries_address_and_complement() {
        let r = finish_packet(0x30); // last chunk offset 0x30 -> address 0x0003
        assert_eq!(&r[11..15], &[0x03, 0x00, !0x03u8, !0x00u8]);
    }

    #[test]
    fn chunk_ack_ok_recognizes_a_real_captured_success_response() {
        // Captured live from `flash` against a real Telink TC32 keyboard (a
        // CIDOO V65 v2): response to chunk_packet(0, [0x00..=0x0F]). Bytes
        // [9,10] = 01 00 -- the *next* expected index (1), not offset 0's own
        // address (0x0000) echoed back.
        let resp: [u8; REPORT_SIZE] = [
            0x05, 0x1d, 0x01, 0x1b, 0x17, 0x04, 0x52, 0x28, 0x00, 0x01, 0x00, 0x00, 0x01, 0x02,
            0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x7b,
            0xf3, 0x00, 0x00, 0x00, 0x00,
        ];
        assert!(chunk_ack_ok(&resp, 0));
    }

    #[test]
    fn chunk_ack_ok_rejects_an_echo_of_the_just_sent_address() {
        // What the old (wrong) check expected: [9,10] == the address just
        // sent, rather than the next expected index. Must not be accepted.
        let mut resp: [u8; REPORT_SIZE] = [0u8; REPORT_SIZE];
        resp[2] = 0x01;
        resp[9] = 0x00;
        resp[10] = 0x00;
        assert!(!chunk_ack_ok(&resp, 0));
    }
}
