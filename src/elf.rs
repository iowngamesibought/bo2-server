//! Bounded ELF64 big-endian address mapping. No client bytes are embedded.
pub struct Elf<'a> {
    data: &'a [u8],
    loads: Vec<(u64, u64, u64)>, // virtual address, file offset, file size
}

impl<'a> Elf<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, &'static str> {
        if data.len() < 64 || &data[..7] != b"\x7fELF\x02\x02\x01" {
            return Err("expected a decrypted big-endian ELF64 executable");
        }
        let u16_at = |i| u16::from_be_bytes(data[i..i + 2].try_into().unwrap());
        let u32_at = |i| u32::from_be_bytes(data[i..i + 4].try_into().unwrap());
        let u64_at = |i| u64::from_be_bytes(data[i..i + 8].try_into().unwrap());
        let count = usize::from(u16_at(56));
        if u16_at(16) != 2
            || u16_at(18) != 21
            || u32_at(20) != 1
            || u16_at(52) != 64
            || u16_at(54) != 56
            || !(1..=128).contains(&count)
        {
            return Err("unsupported ELF header or program table");
        }
        let offset = usize::try_from(u64_at(32)).map_err(|_| "program table overflow")?;
        if offset < 64
            || offset
                .checked_add(count * 56)
                .is_none_or(|end| end > data.len())
        {
            return Err("program table exceeds input bounds");
        }
        let mut loads = Vec::new();
        for i in 0..count {
            let at = offset + i * 56;
            if u32_at(at) != 1 {
                continue;
            }
            let (file, address, size, memory) = (
                u64_at(at + 8),
                u64_at(at + 16),
                u64_at(at + 32),
                u64_at(at + 40),
            );
            if size > memory
                || file
                    .checked_add(size)
                    .is_none_or(|end| end > data.len() as u64)
                || address.checked_add(memory).is_none()
            {
                return Err("invalid load segment bounds");
            }
            if size > 0 {
                loads.push((address, file, size));
            }
        }
        loads.sort_unstable();
        if loads.is_empty() || loads.windows(2).any(|w| w[0].0 + w[0].2 > w[1].0) {
            return Err("missing or overlapping file-backed load segments");
        }
        Ok(Self { data, loads })
    }

    pub fn read(&self, address: u64, length: usize) -> Result<&'a [u8], &'static str> {
        let end = address
            .checked_add(length as u64)
            .ok_or("address overflow")?;
        for &(base, offset, size) in &self.loads {
            if length > 0 && address >= base && end <= base + size {
                let start = (offset + address - base) as usize;
                return Ok(&self.data[start..start + length]);
            }
        }
        Err("address range is not file-backed by one load segment")
    }
}

#[cfg(test)]
pub(crate) fn fixture(offset: usize) -> Vec<u8> {
    let mut data = vec![0; offset + 16];
    data[..7].copy_from_slice(b"\x7fELF\x02\x02\x01");
    for (at, value) in [(16, 2u16), (18, 21), (52, 64), (54, 56), (56, 1)] {
        data[at..at + 2].copy_from_slice(&value.to_be_bytes());
    }
    data[20..24].copy_from_slice(&1u32.to_be_bytes());
    data[32..40].copy_from_slice(&64u64.to_be_bytes());
    data[64..68].copy_from_slice(&1u32.to_be_bytes());
    for (at, value) in [(72, offset as u64), (80, 0x1000), (96, 16), (104, 32)] {
        data[at..at + 8].copy_from_slice(&value.to_be_bytes());
    }
    data[offset..offset + 16].copy_from_slice(b"example_token___");
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maps_addresses_independent_of_file_position_and_excludes_zero_fill() {
        for offset in [128, 512] {
            let data = fixture(offset);
            let elf = Elf::parse(&data).unwrap();
            assert_eq!(elf.read(0x1000, 13).unwrap(), b"example_token");
            assert!(elf.read(0x1010, 1).is_err());
            assert!(elf.read(u64::MAX, 2).is_err());
            assert!(elf.read(0x1000, 0).is_err());
        }
    }
    #[test]
    fn rejects_truncation_wrong_headers_overflow_and_overlap() {
        let good = fixture(256);
        for len in 0..good.len() {
            assert!(Elf::parse(&good[..len]).is_err());
        }
        for at in [0, 4, 5, 6, 17, 19, 23, 53, 55, 57] {
            let mut bad = good.clone();
            bad[at] = 0xff;
            assert!(Elf::parse(&bad).is_err());
        }
        for at in [32, 72, 80, 96, 104] {
            let mut bad = good.clone();
            bad[at..at + 8].fill(0xff);
            assert!(Elf::parse(&bad).is_err());
        }
        let mut bad = good.clone();
        bad[56..58].copy_from_slice(&2u16.to_be_bytes());
        let segment = bad[64..120].to_vec();
        bad[120..176].copy_from_slice(&segment);
        assert!(Elf::parse(&bad).is_err());
    }
}
