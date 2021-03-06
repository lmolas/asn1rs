pub use crate::io::per::unaligned::buffer::BitBuffer;

#[cfg(any(test, feature = "legacy_bit_buffer"))]
#[allow(clippy::module_name_repetitions, deprecated)]
pub mod legacy {
    use super::*;
    use crate::io::per::unaligned::BYTE_LEN;
    use crate::io::uper::Error;
    use crate::io::uper::Error as UperError;
    use crate::io::uper::Reader as UperReader;
    use crate::io::uper::Writer as UperWriter;
    use crate::syn::bitstring::BitVec;
    use byteorder::ByteOrder;
    use byteorder::NetworkEndian;

    pub const UPER_LENGTH_DET_L1: i64 = 127;
    pub const UPER_LENGTH_DET_L2: i64 = 16383;
    // pub const UPER_LENGTH_DET_L3: i64 = 49151;
    // pub const UPER_LENGTH_DET_L4: i64 = 65535;

    pub const SIZE_BITS: usize = 100 * BYTE_LEN;

    #[deprecated]
    pub struct LegacyBitBuffer<'a>(&'a mut BitBuffer);

    // the legacy BitBuffer relies solely on read_bit(), no performance optimisation
    impl UperReader for LegacyBitBuffer<'_> {
        fn read_utf8_string(&mut self) -> Result<String, Error> {
            let len = self.read_length_determinant()?;
            let mut buffer = vec![0_u8; len];
            self.read_bit_string_till_end(&mut buffer[..len], 0)?;
            if let Ok(string) = String::from_utf8(buffer) {
                Ok(string)
            } else {
                Err(Error::InvalidUtf8String)
            }
        }
        fn read_choice_index_extensible(
            &mut self,
            no_of_default_variants: u64,
        ) -> Result<u64, Error> {
            if self.read_bit()? {
                Ok((self.read_int_normally_small()? + no_of_default_variants) as u64)
            } else {
                self.read_choice_index(no_of_default_variants)
            }
        }

        fn read_choice_index(&mut self, no_of_default_variants: u64) -> Result<u64, Error> {
            Ok(self.read_int((0, no_of_default_variants as i64 - 1))? as u64)
        }

        fn read_int(&mut self, range: (i64, i64)) -> Result<i64, Error> {
            let (lower, upper) = range;
            let leading_zeros = ((upper - lower) as u64).leading_zeros();

            let mut buffer = [0_u8; 8];
            let buffer_bits = buffer.len() * BYTE_LEN as usize;
            debug_assert!(buffer_bits == 64);
            self.read_bit_string_till_end(&mut buffer[..], leading_zeros as usize)?;
            let value = NetworkEndian::read_u64(&buffer[..]) as i64;
            Ok(value + lower)
        }

        fn read_int_normally_small(&mut self) -> Result<u64, Error> {
            // X.691-201508 11.6
            let is_small = !self.read_bit()?;
            if is_small {
                // 11.6.1: 6 bit of the number
                let mut buffer = [0u8; std::mem::size_of::<u64>()];
                self.read_bit_string(&mut buffer[7..8], 2, 6)?;
                Ok(u64::from_be_bytes(buffer))
            } else {
                // 11.6.2: (length-determinant + number)
                // this cannot be negative... logically
                let value = self.read_int_max_unsigned()?;
                // u64::try_from(value).map_err(|_| Error::ValueIsNegativeButExpectedUnsigned(value))
                Ok(value)
            }
        }

        fn read_int_max_signed(&mut self) -> Result<i64, Error> {
            let len_in_bytes = self.read_length_determinant()?;
            if len_in_bytes > std::mem::size_of::<i64>() {
                Err(Error::UnsupportedOperation(
                    "Reading bigger data types than 64bit is not supported".into(),
                ))
            } else {
                let mut buffer = [0_u8; std::mem::size_of::<i64>()];
                let offset = (buffer.len() - len_in_bytes) * BYTE_LEN;
                self.read_bit_string_till_end(&mut buffer[..], offset)?;
                let sign_position = buffer.len() - len_in_bytes;
                if buffer[sign_position] & 0x80 != 0 {
                    for value in buffer.iter_mut().take(sign_position) {
                        *value = 0xFF;
                    }
                }
                Ok(i64::from_be_bytes(buffer))
            }
        }

        fn read_int_max_unsigned(&mut self) -> Result<u64, Error> {
            let len_in_bytes = self.read_length_determinant()?;
            if len_in_bytes > std::mem::size_of::<u64>() {
                Err(Error::UnsupportedOperation(
                    "Reading bigger data types than 64bit is not supported".into(),
                ))
            } else {
                let mut buffer = [0_u8; std::mem::size_of::<u64>()];
                let offset = (buffer.len() - len_in_bytes) * BYTE_LEN;
                self.read_bit_string_till_end(&mut buffer[..], offset)?;
                Ok(u64::from_be_bytes(buffer))
            }
        }

        fn read_bitstring(&mut self) -> Result<BitVec, Error> {
            let (bytes, bit_len) = <BitBuffer as crate::io::per::PackedRead>::read_bitstring(
                &mut self.0,
                None,
                None,
                false,
            )?;
            Ok(BitVec::from_bytes(bytes, bit_len))
        }

        fn read_bit_string(
            &mut self,
            buffer: &mut [u8],
            bit_offset: usize,
            bit_length: usize,
        ) -> Result<(), Error> {
            if buffer.len() * BYTE_LEN < bit_offset
                || buffer.len() * BYTE_LEN < bit_offset + bit_length
            {
                return Err(Error::InsufficientSpaceInDestinationBuffer);
            }
            for bit in bit_offset..bit_offset + bit_length {
                let byte_pos = bit / BYTE_LEN;
                let bit_pos = bit % BYTE_LEN;
                let bit_pos = BYTE_LEN - bit_pos - 1; // flip

                if self.read_bit()? {
                    // set bit
                    buffer[byte_pos] |= 0x01 << bit_pos;
                } else {
                    // reset bit
                    buffer[byte_pos] &= !(0x01 << bit_pos);
                }
            }
            Ok(())
        }

        fn read_octet_string(
            &mut self,
            length_range: Option<(i64, i64)>,
        ) -> Result<Vec<u8>, Error> {
            let len = if let Some((min, max)) = length_range {
                self.read_int((min, max))? as usize
            } else {
                self.read_length_determinant()?
            };
            let mut vec = vec![0_u8; len];
            self.read_bit_string_till_end(&mut vec[..], 0)?;
            Ok(vec)
        }

        fn read_bit_string_till_end(
            &mut self,
            buffer: &mut [u8],
            bit_offset: usize,
        ) -> Result<(), Error> {
            let len = (buffer.len() * BYTE_LEN) - bit_offset;
            self.read_bit_string(buffer, bit_offset, len)
        }

        fn read_length_determinant(&mut self) -> Result<usize, Error> {
            if !self.read_bit()? {
                // length <= UPER_LENGTH_DET_L1
                Ok(self.read_int((0, UPER_LENGTH_DET_L1))? as usize)
            } else if !self.read_bit()? {
                // length <= UPER_LENGTH_DET_L2
                Ok(self.read_int((0, UPER_LENGTH_DET_L2))? as usize)
            } else {
                Err(Error::UnsupportedOperation(
                    "Cannot read length determinant for other than i8 and i16".into(),
                ))
            }
        }

        fn read_bit(&mut self) -> Result<bool, UperError> {
            self.0.read_bit()
        }
    }

    // the legacy BitBuffer relies solely on write_bit(), no performance optimisation
    impl UperWriter for LegacyBitBuffer<'_> {
        fn write_utf8_string(&mut self, value: &str) -> Result<(), Error> {
            self.write_length_determinant(value.len())?;
            self.write_bit_string_till_end(value.as_bytes(), 0)?;
            Ok(())
        }
        fn write_choice_index_extensible(
            &mut self,
            index: u64,
            no_of_default_variants: u64,
        ) -> Result<(), Error> {
            if index >= no_of_default_variants {
                self.write_bit(true)?;
                self.write_int_normally_small((index - no_of_default_variants) as u64)
            } else {
                self.write_bit(false)?;
                self.write_choice_index(index, no_of_default_variants)
            }
        }

        fn write_choice_index(
            &mut self,
            index: u64,
            no_of_default_variants: u64,
        ) -> Result<(), Error> {
            self.write_int(index as i64, (0, no_of_default_variants as i64 - 1))
        }

        /// Range is inclusive
        fn write_int(&mut self, value: i64, range: (i64, i64)) -> Result<(), Error> {
            let (lower, upper) = range;
            let value = {
                if value > upper || value < lower {
                    return Err(Error::ValueNotInRange(value, lower, upper));
                }
                (value - lower) as u64
            };
            let leading_zeros = ((upper - lower) as u64).leading_zeros();

            let mut buffer = [0_u8; 8];
            NetworkEndian::write_u64(&mut buffer[..], value);
            let buffer_bits = buffer.len() * BYTE_LEN as usize;
            debug_assert!(buffer_bits == 64);

            self.write_bit_string_till_end(&buffer[..], leading_zeros as usize)?;

            Ok(())
        }
        fn write_int_normally_small(&mut self, value: u64) -> Result<(), Error> {
            // X.691-201508 11.6
            if value <= 63 {
                // 11.6.1: '0'bit + 6 bit of the number
                self.write_bit(false)?;
                let buffer = value.to_be_bytes();
                self.write_bit_string(&buffer[7..8], 2, 6)?; // last 6 bits
                Ok(())
            } else if value <= i64::max_value() as u64 {
                // 11.6.2: '1'bit + (length-determinant + number)
                self.write_bit(true)?;
                self.write_int_max_unsigned(value as _)?;
                Ok(())
            } else {
                Err(Error::ValueExceedsMaxInt)
            }
        }
        fn write_int_max_signed(&mut self, value: i64) -> Result<(), Error> {
            let buffer = value.to_be_bytes();
            let mask = if value.is_negative() { 0xFF } else { 0x00 };
            let byte_len = {
                let mut len = buffer.len();
                while len > 0 && buffer[buffer.len() - len] == mask {
                    len -= 1;
                }
                // otherwise one could not distinguish this positive value
                // from it being a totally different negative value
                if value.is_positive() && value.leading_zeros() % 8 == 0 {
                    len += 1;
                }
                len
            }
            .max(1);
            self.write_length_determinant(byte_len)?;
            let bit_offset = (buffer.len() - byte_len) * BYTE_LEN;
            self.write_bit_string_till_end(&buffer, bit_offset)?;
            Ok(())
        }
        fn write_int_max_unsigned(&mut self, value: u64) -> Result<(), Error> {
            let buffer = value.to_be_bytes();
            let byte_len = {
                let mut len = buffer.len();
                while len > 0 && buffer[buffer.len() - len] == 0x00 {
                    len -= 1;
                }
                len
            }
            .max(1);
            self.write_length_determinant(byte_len)?;
            let bit_offset = (buffer.len() - byte_len) * BYTE_LEN;
            self.write_bit_string_till_end(&buffer, bit_offset)?;
            Ok(())
        }

        fn write_bitstring(&mut self, bits: &BitVec) -> Result<(), Error> {
            <BitBuffer as crate::io::per::PackedWrite>::write_bitstring(
                &mut self.0,
                None,
                None,
                false,
                bits.as_byte_slice(),
                0,
                bits.bit_len(),
            )
        }

        fn write_bit_string(
            &mut self,
            buffer: &[u8],
            bit_offset: usize,
            bit_length: usize,
        ) -> Result<(), Error> {
            if buffer.len() * BYTE_LEN < bit_offset
                || buffer.len() * BYTE_LEN < bit_offset + bit_length
            {
                return Err(Error::InsufficientDataInSourceBuffer);
            }
            for bit in bit_offset..bit_offset + bit_length {
                let byte_pos = bit / BYTE_LEN;
                let bit_pos = bit % BYTE_LEN;
                let bit_pos = BYTE_LEN - bit_pos - 1; // flip

                let bit = (buffer[byte_pos] >> bit_pos & 0x01) == 0x01;
                self.write_bit(bit)?;
            }
            Ok(())
        }

        fn write_octet_string(
            &mut self,
            string: &[u8],
            length_range: Option<(i64, i64)>,
        ) -> Result<(), Error> {
            if let Some((min, max)) = length_range {
                self.write_int(string.len() as i64, (min, max))?;
            } else {
                self.write_length_determinant(string.len())?;
            }
            self.write_bit_string_till_end(string, 0)?;
            Ok(())
        }

        fn write_bit_string_till_end(
            &mut self,
            buffer: &[u8],
            bit_offset: usize,
        ) -> Result<(), Error> {
            let len = (buffer.len() * BYTE_LEN) - bit_offset;
            self.write_bit_string(buffer, bit_offset, len)
        }

        fn write_length_determinant(&mut self, length: usize) -> Result<(), Error> {
            if length <= UPER_LENGTH_DET_L1 as usize {
                self.write_bit(false)?;
                self.write_int(length as i64, (0, UPER_LENGTH_DET_L1))
            } else if length <= UPER_LENGTH_DET_L2 as usize {
                self.write_bit(true)?;
                self.write_bit(false)?;
                self.write_int(length as i64, (0, UPER_LENGTH_DET_L2))
            } else {
                Err(Error::UnsupportedOperation(format!(
            "Writing length determinant for lengths > {} is unsupported, tried for length {}",
            UPER_LENGTH_DET_L2, length
        )))
            }
        }

        fn write_bit(&mut self, bit: bool) -> Result<(), UperError> {
            self.0.write_bit(bit)
        }
    }

    pub fn bit_buffer(size: usize, pos: usize) -> (BitBuffer, Vec<u8>, BitBuffer) {
        let mut bits = BitBuffer::from(vec![
            0b0101_0101_u8.wrapping_shl(pos as u32 % 2);
            size + if pos > 0 { 1 } else { 0 }
        ]);
        for _ in 0..pos {
            bits.read_bit().unwrap();
        }
        (
            bits,
            vec![0_u8; size + if pos > 0 { 1 } else { 0 }],
            BitBuffer::from_bits_with_position(
                vec![0_u8; size + if pos > 0 { 1 } else { 0 }],
                pos,
                pos,
            ),
        )
    }

    pub fn check_result(bits: &mut BitBuffer, offset: usize, len: usize) {
        for i in 0..offset {
            assert!(
                !bits.read_bit().unwrap(),
                "Failed on offset with i={}, offset={}, bits={:?}",
                i,
                offset,
                bits
            );
        }
        for i in 0..len {
            assert_eq!(
                i % 2 == 1,
                bits.read_bit().unwrap(),
                "Failed on data with i={}, offset={}, bits={:?}",
                i,
                offset,
                bits
            );
        }
    }

    pub fn legacy_bit_buffer(size_bits: usize, offset: usize, pos: usize) -> (Vec<u8>, BitBuffer) {
        let (mut bits, mut dest, mut write) = bit_buffer(
            (size_bits + (BYTE_LEN - 1)) / BYTE_LEN + if offset > 0 { 1 } else { 0 },
            pos,
        );
        LegacyBitBuffer(&mut bits)
            .read_bit_string(&mut dest[..], offset, size_bits)
            .unwrap();
        LegacyBitBuffer(&mut write)
            .write_bit_string(&dest[..], offset, size_bits)
            .unwrap();
        (dest, write)
    }

    pub fn new_bit_buffer(size_bits: usize, offset: usize, pos: usize) -> (Vec<u8>, BitBuffer) {
        let (mut bits, mut dest, mut write) = bit_buffer(
            (size_bits + (BYTE_LEN - 1)) / BYTE_LEN + if offset > 0 { 1 } else { 0 },
            pos,
        );
        bits.read_bit_string(&mut dest[..], offset, size_bits)
            .unwrap();
        write
            .write_bit_string(&dest[..], offset, size_bits)
            .unwrap();
        (dest, write)
    }

    pub fn legacy_bit_buffer_with_check(size_bits: usize, offset: usize, pos: usize) {
        let (bits, mut written) = legacy_bit_buffer(size_bits, offset, pos);
        check_result(&mut BitBuffer::from(bits), offset, size_bits);
        check_result(&mut written, 0, size_bits);
    }

    pub fn new_bit_buffer_with_check(size_bits: usize, offset: usize, pos: usize) {
        let (bits, mut written) = new_bit_buffer(size_bits, offset, pos);
        check_result(&mut BitBuffer::from(bits), offset, size_bits);
        check_result(&mut written, 0, size_bits);
    }
}

#[allow(deprecated)] // all the legacy stuff is deprecated
#[allow(clippy::identity_op)] // for better readability across multiple tests
#[cfg(all(test, feature = "legacy_bit_buffer"))]
mod tests {
    use super::legacy::*;
    use super::*;
    use crate::io::per::unaligned::BYTE_LEN;
    use crate::io::uper::Error as UperError;
    use crate::io::uper::Reader as UperReader;
    use crate::io::uper::Writer as UperWriter;

    #[test]
    fn test_legacy_bit_string_offset_0_to_7_pos_0_to_7() {
        for offset in 0..BYTE_LEN {
            for pos in 0..BYTE_LEN {
                legacy_bit_buffer_with_check(SIZE_BITS, offset, pos)
            }
        }
    }

    #[test]
    fn test_new_bit_string_offset_0_to_7_pos_0_to_7() {
        for offset in 0..BYTE_LEN {
            for pos in 0..BYTE_LEN {
                new_bit_buffer_with_check(SIZE_BITS, offset, pos)
            }
        }
    }

    #[test]
    pub fn bit_buffer_write_bit_keeps_correct_order() -> Result<(), UperError> {
        let mut buffer = BitBuffer::default();

        buffer.write_bit(true)?;
        buffer.write_bit(false)?;
        buffer.write_bit(false)?;
        buffer.write_bit(true)?;

        buffer.write_bit(true)?;
        buffer.write_bit(true)?;
        buffer.write_bit(true)?;
        buffer.write_bit(false)?;

        assert_eq!(buffer.content(), &[0b1001_1110]);

        buffer.write_bit(true)?;
        buffer.write_bit(false)?;
        buffer.write_bit(true)?;
        buffer.write_bit(true)?;

        buffer.write_bit(true)?;
        buffer.write_bit(true)?;
        buffer.write_bit(true)?;
        buffer.write_bit(false)?;

        assert_eq!(buffer.content(), &[0b1001_1110, 0b1011_1110]);

        buffer.write_bit(true)?;
        buffer.write_bit(false)?;
        buffer.write_bit(true)?;
        buffer.write_bit(false)?;

        assert_eq!(buffer.content(), &[0b1001_1110, 0b1011_1110, 0b1010_0000]);

        let mut buffer = BitBuffer::from_bits(buffer.content().into(), buffer.bit_len());
        assert!(buffer.read_bit()?);
        assert!(!buffer.read_bit()?);
        assert!(!buffer.read_bit()?);
        assert!(buffer.read_bit()?);

        assert!(buffer.read_bit()?);
        assert!(buffer.read_bit()?);
        assert!(buffer.read_bit()?);
        assert!(!buffer.read_bit()?);

        assert!(buffer.read_bit()?);
        assert!(!buffer.read_bit()?);
        assert!(buffer.read_bit()?);
        assert!(buffer.read_bit()?);

        assert!(buffer.read_bit()?);
        assert!(buffer.read_bit()?);
        assert!(buffer.read_bit()?);
        assert!(!buffer.read_bit()?);

        assert!(buffer.read_bit()?);
        assert!(!buffer.read_bit()?);
        assert!(buffer.read_bit()?);
        assert!(!buffer.read_bit()?);

        assert_eq!(buffer.read_bit(), Err(UperError::EndOfStream));

        Ok(())
    }

    #[test]
    fn bit_buffer_bit_string_till_end() -> Result<(), UperError> {
        let content = &[0xFF, 0x74, 0xA6, 0x0F];
        let mut buffer = BitBuffer::default();
        buffer.write_bit_string_till_end(content, 0)?;
        assert_eq!(buffer.content(), content);

        {
            let mut buffer2 = BitBuffer::from_bits(buffer.content().into(), buffer.bit_len());
            let mut content2 = vec![0_u8; content.len()];
            buffer2.read_bit_string_till_end(&mut content2[..], 0)?;
            assert_eq!(&content[..], &content2[..]);
        }

        let mut content2 = vec![0xFF_u8; content.len()];
        buffer.read_bit_string_till_end(&mut content2[..], 0)?;
        assert_eq!(&content[..], &content2[..]);

        Ok(())
    }

    #[test]
    fn bit_buffer_bit_string_till_end_with_offset() -> Result<(), UperError> {
        let content = &[0b1111_1111, 0b0111_0100, 0b1010_0110, 0b0000_1111];
        let mut buffer = BitBuffer::default();
        buffer.write_bit_string_till_end(content, 7)?;
        assert_eq!(
            buffer.content(),
            &[0b1011_1010, 0b0101_0011, 0b0000_0111, 0b1000_0000]
        );

        {
            let mut buffer2 = BitBuffer::from_bits(buffer.content().into(), buffer.bit_len());
            let mut content2 = vec![0xFF_u8; content.len()];
            content2[0] = content[0] & 0b1111_1110; // since we are skipping the first 7 bits
            buffer2.read_bit_string_till_end(&mut content2[..], 7)?;
            assert_eq!(&content[..], &content2[..]);
        }

        let mut content2 = vec![0_u8; content.len()];
        content2[0] = content[0] & 0b1111_1110; // since we are skipping the first 7 bits
        buffer.read_bit_string_till_end(&mut content2[..], 7)?;
        assert_eq!(&content[..], &content2[..]);

        Ok(())
    }

    #[test]
    fn bit_buffer_bit_string() -> Result<(), UperError> {
        let content = &[0b1111_1111, 0b0111_0100, 0b1010_0110, 0b0000_1111];
        let mut buffer = BitBuffer::default();
        buffer.write_bit_string(content, 7, 12)?;
        assert_eq!(buffer.content(), &[0b1011_1010, 0b0101_0000]);

        {
            let mut buffer2 = BitBuffer::from_bits(buffer.content().into(), buffer.bit_len());
            let mut content2 = vec![0_u8; content.len()];
            // since we are skipping the first 7 bits
            let content = &[
                content[0] & 0x01,
                content[1],
                content[2] & 0b1110_0000,
                0x00,
            ];
            buffer2.read_bit_string(&mut content2[..], 7, 12)?;
            assert_eq!(&content[..], &content2[..]);
        }

        let mut content2 = vec![0x00_u8; content.len()];
        // since we are skipping the first 7 bits
        let content = &[
            content[0] & 0x01,
            content[1],
            content[2] & 0b1110_0000,
            0x00,
        ];
        buffer.read_bit_string(&mut content2[..], 7, 12)?;
        assert_eq!(&content[..], &content2[..]);

        Ok(())
    }

    #[test]
    fn bit_buffer_length_determinant_0() -> Result<(), UperError> {
        const DET: usize = 0;
        let mut buffer = BitBuffer::default();
        buffer.write_length_determinant(DET)?;
        assert_eq!(buffer.content(), &[0x00 | DET as u8]);

        {
            let mut buffer2 = BitBuffer::from_bits(buffer.content().into(), buffer.bit_len());
            assert_eq!(DET, buffer2.read_length_determinant()?);
        }

        assert_eq!(DET, buffer.read_length_determinant()?);

        Ok(())
    }

    #[test]
    fn bit_buffer_length_determinant_1() -> Result<(), UperError> {
        const DET: usize = 1;
        let mut buffer = BitBuffer::default();
        buffer.write_length_determinant(DET)?;
        assert_eq!(buffer.content(), &[0x00 | DET as u8]);

        {
            let mut buffer2 = BitBuffer::from_bits(buffer.content().into(), buffer.bit_len());
            assert_eq!(DET, buffer2.read_length_determinant()?);
        }

        assert_eq!(DET, buffer.read_length_determinant()?);
        Ok(())
    }

    #[test]
    fn bit_buffer_length_determinant_127() -> Result<(), UperError> {
        const DET: usize = 126;
        let mut buffer = BitBuffer::default();
        buffer.write_length_determinant(DET)?;
        assert_eq!(buffer.content(), &[0x00 | DET as u8]);

        {
            let mut buffer2 = BitBuffer::from_bits(buffer.content().into(), buffer.bit_len());
            assert_eq!(DET, buffer2.read_length_determinant()?);
        }

        assert_eq!(DET, buffer.read_length_determinant()?);
        Ok(())
    }

    #[test]
    fn bit_buffer_length_determinant_128() -> Result<(), UperError> {
        const DET: usize = 128;
        let mut buffer = BitBuffer::default();
        buffer.write_length_determinant(DET)?;
        // detects that the value is greater than 127, so
        //   10xx_xxxx xxxx_xxxx (header)
        // | --00_0000 1000_0000 (128)
        // =======================
        //   1000_0000 1000_0000
        assert_eq!(buffer.content(), &[0x80 | 0x00, 0x00 | DET as u8]);

        {
            let mut buffer2 = BitBuffer::from_bits(buffer.content().into(), buffer.bit_len());
            assert_eq!(DET, buffer2.read_length_determinant()?);
        }

        assert_eq!(DET, buffer.read_length_determinant()?);
        Ok(())
    }

    #[test]
    fn bit_buffer_length_determinant_16383() -> Result<(), UperError> {
        const DET: usize = 16383;
        let mut buffer = BitBuffer::default();
        buffer.write_length_determinant(DET)?;
        // detects that the value is greater than 127, so
        //   10xx_xxxx xxxx_xxxx (header)
        // | --11_1111 1111_1111 (16383)
        // =======================
        //   1011_1111 1111_1111
        assert_eq!(
            buffer.content(),
            &[0x80 | (DET >> 8) as u8, 0x00 | (DET & 0xFF) as u8]
        );

        {
            let mut buffer2 = BitBuffer::from_bits(buffer.content().into(), buffer.bit_len());
            assert_eq!(DET, buffer2.read_length_determinant()?);
        }

        assert_eq!(DET, buffer.read_length_determinant()?);
        Ok(())
    }

    fn check_int_max(buffer: &mut BitBuffer, int: i64) -> Result<(), UperError> {
        {
            let mut buffer2 = BitBuffer::from_bits(buffer.content().into(), buffer.bit_len());
            assert_eq!(int, buffer2.read_int_max_signed()?);
        }

        assert_eq!(int, buffer.read_int_max_signed()?);
        Ok(())
    }

    #[test]
    fn bit_buffer_int_max_neg_12() -> Result<(), UperError> {
        const INT: i64 = -12;
        let mut buffer = BitBuffer::default();
        buffer.write_int_max_signed(INT)?;
        // Can be represented in 1 byte,
        // therefore the first byte is written
        // with 0x00 (header) | 1 (byte len).
        // The second byte is then the actual value
        assert_eq!(buffer.content(), &[0x00 | 1, INT as u8]);
        check_int_max(&mut buffer, INT)?;
        Ok(())
    }

    #[test]
    fn bit_buffer_int_max_0() -> Result<(), UperError> {
        const INT: i64 = 0;
        let mut buffer = BitBuffer::default();
        buffer.write_int_max_signed(INT)?;
        // Can be represented in 1 byte,
        // therefore the first byte is written
        // with 0x00 (header) | 1 (byte len).
        // The second byte is then the actual value
        assert_eq!(buffer.content(), &[0x00 | 1, INT as u8]);
        check_int_max(&mut buffer, INT)?;
        Ok(())
    }

    #[test]
    fn bit_buffer_int_max_127() -> Result<(), UperError> {
        const INT: i64 = 127; // u4::max_value() as u64
        let mut buffer = BitBuffer::default();
        buffer.write_int_max_signed(INT)?;
        // Can be represented in 1 byte,
        // therefore the first byte is written
        // with 0x00 (header) | 1 (byte len).
        // The second byte is then the actual value
        assert_eq!(buffer.content(), &[0x00 | 1, INT as u8]);
        check_int_max(&mut buffer, INT)?;
        Ok(())
    }

    #[test]
    fn bit_buffer_int_max_128() -> Result<(), UperError> {
        const INT: i64 = 128; // u4::max_value() as u64 + 1
        let mut buffer = BitBuffer::default();
        buffer.write_int_max_signed(INT)?;
        assert_eq!(buffer.content(), &[0x02, 0x00, 0x80]);
        check_int_max(&mut buffer, INT)?;
        Ok(())
    }

    #[test]
    fn bit_buffer_int_max_255() -> Result<(), UperError> {
        const INT: i64 = 255; // u8::max_value() as u64
        let mut buffer = BitBuffer::default();
        buffer.write_int_max_signed(INT)?;
        assert_eq!(buffer.content(), &[0x02, 0x00, 0xFF]);
        check_int_max(&mut buffer, INT)?;
        Ok(())
    }

    #[test]
    fn bit_buffer_int_max_256() -> Result<(), UperError> {
        const INT: i64 = 256; // u8::max_value() as u64 + 1
        let mut buffer = BitBuffer::default();
        buffer.write_int_max_signed(INT)?;
        // Can be represented in 2 bytes,
        // therefore the first byte is written
        // with 0x00 (header) | 2 (byte len).
        // The second byte is then the actual value
        assert_eq!(
            buffer.content(),
            &[
                0x00 | 2,
                ((INT & 0xFF_00) >> 8) as u8,
                ((INT & 0x00_ff) >> 0) as u8,
            ]
        );
        check_int_max(&mut buffer, INT)?;
        Ok(())
    }

    #[test]
    fn bit_buffer_int_max_65535() -> Result<(), UperError> {
        const INT: i64 = 65_535; // u16::max_value() as u64
        let mut buffer = BitBuffer::default();
        buffer.write_int_max_signed(INT)?;
        assert_eq!(buffer.content(), &[0x03, 0x00, 0xFF, 0xFF]);
        check_int_max(&mut buffer, INT)?;
        Ok(())
    }

    #[test]
    fn bit_buffer_int_max_65536() -> Result<(), UperError> {
        const INT: i64 = 65_536; // u16::max_value() as u64 + 1
        let mut buffer = BitBuffer::default();
        buffer.write_int_max_signed(INT)?;
        assert_eq!(buffer.content(), &[0x03, 0x01, 0x00, 0x00]);
        check_int_max(&mut buffer, INT)?;
        Ok(())
    }

    #[test]
    fn bit_buffer_int_max_16777215() -> Result<(), UperError> {
        const INT: i64 = 16_777_215; // u24::max_value() as u64
        let mut buffer = BitBuffer::default();
        buffer.write_int_max_signed(INT)?;
        assert_eq!(buffer.content(), &[0x04, 0x00, 0xFF, 0xFF, 0xFF]);
        check_int_max(&mut buffer, INT)?;
        Ok(())
    }

    #[test]
    fn bit_buffer_int_max_16777216() -> Result<(), UperError> {
        const INT: i64 = 16_777_216; // u24::max_value() as u64 + 1
        let mut buffer = BitBuffer::default();
        buffer.write_int_max_signed(INT)?;
        // Can be represented in 4 bytes,
        // therefore the first byte is written
        // with 0x00 (header) | 4 (byte len).
        // The second byte is then the actual value
        assert_eq!(
            buffer.content(),
            &[
                0x00 | 4,
                ((INT & 0xFF_00_00_00) >> 24) as u8,
                ((INT & 0x00_FF_00_00) >> 16) as u8,
                ((INT & 0x00_00_FF_00) >> 8) as u8,
                ((INT & 0x00_00_00_FF) >> 0) as u8,
            ]
        );
        check_int_max(&mut buffer, INT)?;
        Ok(())
    }

    #[test]
    fn bit_buffer_int_max_4294967295() -> Result<(), UperError> {
        const INT: i64 = 4_294_967_295; // u32::max_value() as u64
        let mut buffer = BitBuffer::default();
        buffer.write_int_max_signed(INT)?;
        assert_eq!(buffer.content(), &[0x05, 0x00, 0xFF, 0xFF, 0xFF, 0xFF]);
        check_int_max(&mut buffer, INT)?;
        Ok(())
    }

    #[test]
    fn bit_buffer_int_max_4294967296() -> Result<(), UperError> {
        const INT: i64 = 4_294_967_296; // u32::max_value() as u64 + 1
        let mut buffer = BitBuffer::default();
        buffer.write_int_max_signed(INT)?;
        assert_eq!(buffer.content(), &[0x05, 0x01, 0x00, 0x00, 0x00, 0x00]);
        check_int_max(&mut buffer, INT)?;
        Ok(())
    }

    #[test]
    fn bit_buffer_int_max_i64_max() -> Result<(), UperError> {
        const INT: i64 = i64::max_value();
        let mut buffer = BitBuffer::default();
        buffer.write_int_max_signed(INT)?;
        // Can be represented in 8 bytes,
        // therefore the first byte is written
        // with 0x00 (header) | 8 (byte len).
        // The second byte is then the actual value
        assert_eq!(
            buffer.content(),
            &[
                0x00 | 8,
                ((INT as u64 & 0xFF_00_00_00_00_00_00_00_u64) >> 56) as u8,
                ((INT as u64 & 0x00_FF_00_00_00_00_00_00_u64) >> 48) as u8,
                ((INT as u64 & 0x00_00_FF_00_00_00_00_00_u64) >> 40) as u8,
                ((INT as u64 & 0x00_00_00_FF_00_00_00_00_u64) >> 32) as u8,
                ((INT as u64 & 0x00_00_00_00_FF_00_00_00_u64) >> 24) as u8,
                ((INT as u64 & 0x00_00_00_00_00_FF_00_00_u64) >> 16) as u8,
                ((INT as u64 & 0x00_00_00_00_00_00_FF_00_u64) >> 8) as u8,
                ((INT as u64 & 0x00_00_00_00_00_00_00_FF_u64) >> 0) as u8,
            ]
        );
        check_int_max(&mut buffer, INT)?;
        Ok(())
    }

    #[test]
    fn bit_buffer_write_int_detects_not_in_range_positive_only() {
        let mut buffer = BitBuffer::default();
        // lower check
        assert_eq!(
            buffer.write_int(0, (10, 127)),
            Err(UperError::ValueNotInRange(0, 10, 127))
        );
        // upper check
        assert_eq!(
            buffer.write_int(128, (10, 127)),
            Err(UperError::ValueNotInRange(128, 10, 127))
        );
    }

    #[test]
    fn bit_buffer_write_int_detects_not_in_range_negative() {
        let mut buffer = BitBuffer::default();
        // lower check
        assert_eq!(
            buffer.write_int(-11, (-10, -1)),
            Err(UperError::ValueNotInRange(-11, -10, -1))
        );
        // upper check
        assert_eq!(
            buffer.write_int(0, (-10, -1)),
            Err(UperError::ValueNotInRange(0, -10, -1))
        );
    }

    #[test]
    fn bit_buffer_write_int_detects_not_in_range_with_negative() {
        let mut buffer = BitBuffer::default();
        // lower check
        assert_eq!(
            buffer.write_int(-11, (-10, 1)),
            Err(UperError::ValueNotInRange(-11, -10, 1))
        );
        // upper check
        assert_eq!(
            buffer.write_int(2, (-10, 1)),
            Err(UperError::ValueNotInRange(2, -10, 1))
        );
    }

    fn check_int(buffer: &mut BitBuffer, int: i64, range: (i64, i64)) -> Result<(), UperError> {
        {
            let mut buffer2 = BitBuffer::from_bits(buffer.content().into(), buffer.bit_len());
            assert_eq!(int, buffer2.read_int(range)?);
        }
        assert_eq!(int, buffer.read_int(range)?);
        Ok(())
    }

    #[test]
    fn bit_buffer_int_7bits() -> Result<(), UperError> {
        const INT: i64 = 10;
        const RANGE: (i64, i64) = (0, 127);
        let mut buffer = BitBuffer::default();
        buffer.write_int(INT, RANGE)?;
        // [0; 127] are 128 numbers, so they
        // have to fit in 7 bit
        assert_eq!(buffer.content(), &[(INT as u8) << 1]);
        check_int(&mut buffer, INT, RANGE)?;
        // be sure write_bit writes at the 8th bit
        buffer.write_bit(true)?;
        assert_eq!(buffer.content(), &[(INT as u8) << 1 | 0b0000_0001]);
        Ok(())
    }

    #[test]
    fn bit_buffer_int_neg() -> Result<(), UperError> {
        const INT: i64 = -10;
        const RANGE: (i64, i64) = (-128, 127);
        let mut buffer = BitBuffer::default();
        buffer.write_int(INT, RANGE)?;
        // [-128; 127] are 255 numbers, so they
        // have to fit in one byte
        assert_eq!(buffer.content(), &[(INT - RANGE.0) as u8]);
        check_int(&mut buffer, INT, RANGE)?;
        Ok(())
    }

    #[test]
    fn bit_buffer_int_neg_extended_range() -> Result<(), UperError> {
        const INT: i64 = -10;
        const RANGE: (i64, i64) = (-128, 128);
        let mut buffer = BitBuffer::default();
        buffer.write_int(INT, RANGE)?;
        // [-128; 127] are 256 numbers, so they
        // don't fit in one byte but in 9 bits
        assert_eq!(
            buffer.content(),
            &[
                ((INT - RANGE.0) as u8) >> 1,
                (((INT - RANGE.0) as u8) << 7) | 0b0000_0000
            ]
        );
        // be sure write_bit writes at the 10th bit
        buffer.write_bit(true)?;
        assert_eq!(
            buffer.content(),
            &[
                ((INT - RANGE.0) as u8) >> 1,
                ((INT - RANGE.0) as u8) << 7 | 0b0100_0000
            ]
        );
        check_int(&mut buffer, INT, RANGE)?;
        Ok(())
    }

    #[test]
    fn bit_buffer_octet_string_with_range() -> Result<(), UperError> {
        // test scenario from https://github.com/alexvoronov/geonetworking/blob/57a43113aeabc25f005ea17f76409aed148e67b5/camdenm/src/test/java/net/gcdc/camdenm/UperEncoderDecodeTest.java#L169
        const BYTES: &[u8] = &[0x2A, 0x2B, 0x96, 0xFF];
        const RANGE: (i64, i64) = (1, 20);
        let mut buffer = BitBuffer::default();
        buffer.write_octet_string(BYTES, Some(RANGE))?;
        assert_eq!(&[0x19, 0x51, 0x5c, 0xb7, 0xf8], &buffer.content(),);
        Ok(())
    }

    #[test]
    fn bit_buffer_octet_string_without_range() -> Result<(), UperError> {
        const BYTES: &[u8] = &[0x2A, 0x2B, 0x96, 0xFF];
        let mut buffer = BitBuffer::default();
        buffer.write_octet_string(BYTES, None)?;
        assert_eq!(&[0x04, 0x2a, 0x2b, 0x96, 0xff], &buffer.content(),);
        Ok(())
    }

    #[test]
    fn bit_buffer_octet_string_empty() -> Result<(), UperError> {
        const BYTES: &[u8] = &[];
        let mut buffer = BitBuffer::default();
        buffer.write_octet_string(BYTES, None)?;
        assert_eq!(&[0x00], &buffer.content(),);
        Ok(())
    }

    #[test]
    fn test_int_normally_small_5() -> Result<(), UperError> {
        // example from larmouth-asn1-book, p.296, Figure III-25
        let mut buffer = BitBuffer::default();
        buffer.write_int_normally_small(5)?;
        // first 7 bits are relevant
        assert_eq!(&[0b0000_101_0], &buffer.content());
        assert_eq!(5, buffer.read_int_normally_small()?);
        Ok(())
    }

    #[test]
    fn test_int_normally_small_60() -> Result<(), UperError> {
        // example from larmouth-asn1-book, p.296, Figure III-25
        let mut buffer = BitBuffer::default();
        buffer.write_int_normally_small(60)?;
        // first 7 bits
        assert_eq!(&[0b0111_100_0], &buffer.content());
        assert_eq!(60, buffer.read_int_normally_small()?);
        Ok(())
    }

    #[test]
    fn test_int_normally_small_254() -> Result<(), UperError> {
        // example from larmouth-asn1-book, p.296, Figure III-25
        let mut buffer = BitBuffer::default();
        buffer.write_int_normally_small(254)?;
        // first 17 bits are relevant
        // assert_eq!(&[0x1, 0b0000_0001, 0b1111_1110], &buffer.content());
        assert_eq!(
            //  Bit for greater 63
            //  |
            //  V |-len 1 byte-| |-value 254-| |-rest-|
            &[0b1_000_0000, 0b1__111_1111, 0b0_000_0000],
            &buffer.content()
        );
        Ok(())
    }

    #[test]
    fn test_write_choice_index_extensible() -> Result<(), UperError> {
        fn write_once(
            index: u64,
            no_of_default_variants: u64,
        ) -> Result<(usize, Vec<u8>), UperError> {
            let mut buffer = BitBuffer::default();
            buffer.write_choice_index_extensible(index, no_of_default_variants)?;
            let bits = buffer.bit_len();
            Ok((bits, buffer.into()))
        }
        assert_eq!((2, vec![0x00]), write_once(0, 2)?);
        assert_eq!((2, vec![0x40]), write_once(1, 2)?);
        assert_eq!((8, vec![0x80]), write_once(2, 2)?);
        assert_eq!((8, vec![0x81]), write_once(3, 2)?);
        Ok(())
    }

    #[test]
    fn test_read_choice_index_extensible() -> Result<(), UperError> {
        fn read_once(data: &[u8], bits: usize, no_of_variants: u64) -> Result<u64, UperError> {
            let mut buffer = BitBuffer::default();
            buffer.write_bit_string(data, 0, bits)?;
            buffer.read_choice_index_extensible(no_of_variants)
        }
        assert_eq!(0, read_once(&[0x00], 2, 2)?);
        assert_eq!(1, read_once(&[0x40], 2, 2)?);
        assert_eq!(2, read_once(&[0x80], 8, 2)?);
        assert_eq!(3, read_once(&[0x81], 8, 2)?);
        Ok(())
    }

    #[test]
    fn test_sub_string_with_length_delimiter_prefix() {
        let mut buffer = BitBuffer::default();
        buffer
            .write_substring_with_length_determinant_prefix(&|writer| {
                writer.write_int_max_signed(1337)
            })
            .unwrap();
        assert_eq!(&[0x03, 0x02, 0x05, 0x39], buffer.content());
        let mut inner = buffer
            .read_substring_with_length_determinant_prefix()
            .unwrap();
        assert_eq!(1337, inner.read_int_max_signed().unwrap());
    }

    #[test]
    fn test_sub_string_with_length_delimiter_prefix_not_aligned() {
        let mut buffer = BitBuffer::default();
        buffer.write_bit(false).unwrap();
        buffer.write_bit(false).unwrap();
        buffer.write_bit(false).unwrap();
        buffer.write_bit(false).unwrap();
        buffer
            .write_substring_with_length_determinant_prefix(&|writer| {
                writer.write_int_max_signed(1337)
            })
            .unwrap();
        assert_eq!(&[0x00, 0x30, 0x20, 0x53, 0x90], buffer.content());
        assert_eq!(false, buffer.read_bit().unwrap());
        assert_eq!(false, buffer.read_bit().unwrap());
        assert_eq!(false, buffer.read_bit().unwrap());
        assert_eq!(false, buffer.read_bit().unwrap());
        let mut inner = buffer
            .read_substring_with_length_determinant_prefix()
            .unwrap();
        assert_eq!(1337, inner.read_int_max_signed().unwrap());
    }
    #[test]
    fn test_sub_string_with_length_delimiter_prefix_raw_not_aligned() {
        let mut buffer = ([0_u8; 1024], 0_usize);
        let writer = &mut (&mut buffer.0[..], &mut buffer.1) as &mut dyn UperWriter;
        writer.write_bit(false).unwrap();
        writer.write_bit(false).unwrap();
        writer.write_bit(false).unwrap();
        writer.write_bit(false).unwrap();
        writer
            .write_substring_with_length_determinant_prefix(&|writer| {
                writer.write_int_max_signed(1337)
            })
            .unwrap();
        assert_eq!(&[0x00, 0x30, 0x20, 0x53, 0x90], &buffer.0[..5]);
        buffer.1 = 0;
        let reader = &mut (&buffer.0[..], &mut buffer.1) as &mut dyn UperReader;
        assert_eq!(false, reader.read_bit().unwrap());
        assert_eq!(false, reader.read_bit().unwrap());
        assert_eq!(false, reader.read_bit().unwrap());
        assert_eq!(false, reader.read_bit().unwrap());
        let mut inner = reader
            .read_substring_with_length_determinant_prefix()
            .unwrap();
        assert_eq!(1337, inner.read_int_max_signed().unwrap());
    }
}
