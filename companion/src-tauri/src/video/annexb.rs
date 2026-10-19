//! H.264 Annex B helpers: split a byte stream on start codes.

/// The NAL units in `data`, without their start codes.
pub fn nal_units(data: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    let ends: Vec<usize> = starts
        .iter()
        .skip(1)
        .map(|&s| {
            // Trim the start code (and a 4-byte code's leading zero) before the next unit.
            let mut e = s - 3;
            while e > 0 && data[e - 1] == 0 {
                e -= 1;
            }
            e
        })
        .chain(std::iter::once(data.len()))
        .collect();
    starts.into_iter().zip(ends).filter(|(s, e)| e > s).map(move |(s, e)| &data[s..e])
}

/// NAL unit type (5 = IDR slice, 7 = SPS, 8 = PPS, 1 = non-IDR slice, 9 = AUD).
pub fn nal_type(nal: &[u8]) -> u8 {
    nal.first().map_or(0, |b| b & 0x1f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_3_and_4_byte_start_codes() {
        let data = [0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x68, 3, 0, 0, 0, 1, 0x65, 4, 5];
        let units: Vec<&[u8]> = nal_units(&data).collect();
        assert_eq!(units, vec![&[0x67, 1, 2][..], &[0x68, 3][..], &[0x65, 4, 5][..]]);
        assert_eq!(units.iter().map(|n| nal_type(n)).collect::<Vec<_>>(), vec![7, 8, 5]);
    }
}
