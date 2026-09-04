pub fn crc16_modbus(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xA001 } else { crc >> 1 };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_published_check_value() {
        // Standard CRC-16/MODBUS check value (poly 0x8005, refin/refout, init 0xFFFF).
        assert_eq!(crc16_modbus(b"123456789"), 0x4b37);
    }
}
