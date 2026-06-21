use crc::{Crc, CRC_32_ISO_HDLC};

pub const CRC32: Crc<u32> = Crc::<u32>::new(&CRC_32_ISO_HDLC);

pub fn crc32(data: &[u8]) -> u32 {
    let mut digest = CRC32.digest();
    digest.update(data);
    digest.finalize()
}

pub fn adler32(data: &[u8]) -> u32 {
    adler::adler32_slice(data)
}

pub fn simple_sum(data: &[u8]) -> u32 {
    data.iter().fold(0u32, |acc, &b| acc + b as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32() {
        let data = b"123456789";
        assert_eq!(crc32(data), 0xCBF43926);
    }

    #[test]
    fn test_adler32() {
        let data = b"123456789";
        assert_eq!(adler32(data), 0x091E01DE);
    }

    #[test]
    fn test_simple_sum() {
        let data = &[1, 2, 3, 4, 5];
        assert_eq!(simple_sum(data), 15);
    }
}
