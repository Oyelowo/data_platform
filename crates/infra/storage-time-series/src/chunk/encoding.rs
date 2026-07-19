//! Time-series chunk compression encodings.
//!
//! This module implements:
//!
//! * Delta-of-delta timestamp compression with variable bit-width fallback.
//! * Gorilla XOR compression for `f64` values.
//! * Length-prefixed byte blobs, optionally compressed with Zstd.

use bytes::{Buf, BufMut};

use crate::format::{Sample, Timestamp, Value};
use crate::options::CompressionKind;

/// Encode a sequence of time-ordered samples into a chunk payload.
pub fn encode_samples(samples: &[Sample], compression: CompressionKind) -> crate::Result<Vec<u8>> {
    let mut buf = Vec::new();
    buf.put_u32_le(samples.len() as u32);

    // Extract timestamps and values into separate arrays.
    let timestamps: Vec<Timestamp> = samples.iter().map(|s| s.timestamp).collect();
    encode_timestamps(&timestamps, &mut buf)?;

    match compression {
        CompressionKind::None => encode_values_none(samples, &mut buf)?,
        CompressionKind::Gorilla => encode_values_gorilla(samples, &mut buf)?,
        CompressionKind::Zstd => encode_values_zstd(samples, &mut buf)?,
    }

    Ok(buf)
}

/// Decode a chunk payload into a vector of samples.
pub fn decode_samples(buf: &[u8], compression: CompressionKind) -> crate::Result<Vec<Sample>> {
    if buf.len() < 4 {
        return Err(crate::Error::corruption("chunk payload too short"));
    }
    let mut cursor = buf;
    let count = cursor.get_u32_le() as usize;
    let timestamps = decode_timestamps(&mut cursor, count)?;
    let values = match compression {
        CompressionKind::None => decode_values_none(cursor, count)?,
        CompressionKind::Gorilla => decode_values_gorilla(cursor, count)?,
        CompressionKind::Zstd => decode_values_zstd(cursor, count)?,
    };
    if timestamps.len() != values.len() {
        return Err(crate::Error::corruption("timestamp/value count mismatch"));
    }
    Ok(timestamps
        .into_iter()
        .zip(values)
        .map(|(timestamp, value)| Sample { timestamp, value })
        .collect())
}

// ---------------------------------------------------------------------------
// Timestamps: delta-of-delta encoded as signed varints.
// ---------------------------------------------------------------------------

fn encode_timestamps(timestamps: &[Timestamp], buf: &mut Vec<u8>) -> crate::Result<()> {
    if timestamps.is_empty() {
        return Ok(());
    }
    buf.put_u64_le(timestamps[0]);
    if timestamps.len() == 1 {
        return Ok(());
    }
    let mut deltas: Vec<i64> = Vec::with_capacity(timestamps.len() - 1);
    let mut prev = timestamps[0] as i64;
    for &ts in &timestamps[1..] {
        let curr = ts as i64;
        deltas.push(curr - prev);
        prev = curr;
    }

    // Write first delta explicitly.
    encode_varint_signed(deltas[0], buf);
    // Write count of remaining deltas.
    encode_varint((deltas.len() - 1) as u64, buf);
    // Write delta-of-deltas as signed varints.
    let mut prev_delta = deltas[0];
    for &delta in &deltas[1..] {
        encode_varint_signed(delta - prev_delta, buf);
        prev_delta = delta;
    }
    Ok(())
}

fn decode_timestamps(cursor: &mut &[u8], count: usize) -> crate::Result<Vec<Timestamp>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    if cursor.len() < 8 {
        return Err(crate::Error::corruption("timestamp header truncated"));
    }
    let first = cursor.get_u64_le();
    if count == 1 {
        return Ok(vec![first]);
    }
    let first_delta = decode_varint_signed(cursor);
    let remaining = decode_varint(cursor) as usize;
    if remaining + 2 != count {
        return Err(crate::Error::corruption("timestamp count mismatch"));
    }

    let mut timestamps = Vec::with_capacity(count);
    timestamps.push(first);
    let mut prev_ts = first as i64 + first_delta;
    timestamps.push(prev_ts as Timestamp);
    let mut prev_delta = first_delta;

    for _ in 0..remaining {
        let dod = decode_varint_signed(cursor);
        prev_delta += dod;
        prev_ts += prev_delta;
        timestamps.push(prev_ts as Timestamp);
    }
    Ok(timestamps)
}

// ---------------------------------------------------------------------------
// Values: None (raw), Gorilla (f64), Zstd (bytes).
// ---------------------------------------------------------------------------

fn encode_values_none(samples: &[Sample], buf: &mut Vec<u8>) -> crate::Result<()> {
    for sample in samples {
        match &sample.value {
            Value::F64(v) => {
                buf.push(0u8);
                buf.extend_from_slice(&v.to_be_bytes());
            }
            Value::Bytes(b) => {
                buf.push(1u8);
                encode_varint(b.len() as u64, buf);
                buf.extend_from_slice(b);
            }
        }
    }
    Ok(())
}

fn decode_values_none(mut cursor: &[u8], count: usize) -> crate::Result<Vec<Value>> {
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        if cursor.is_empty() {
            return Err(crate::Error::corruption("truncated none value header"));
        }
        match cursor[0] {
            0 => {
                if cursor.len() < 9 {
                    return Err(crate::Error::corruption("truncated none f64 value"));
                }
                let bytes = cursor[1..9].try_into().map_err(|_| {
                    crate::Error::corruption("cannot read none f64 bytes")
                })?;
                values.push(Value::F64(f64::from_be_bytes(bytes)));
                cursor = &cursor[9..];
            }
            1 => {
                cursor = &cursor[1..];
                let len = decode_varint(&mut cursor) as usize;
                if cursor.len() < len {
                    return Err(crate::Error::corruption("truncated none bytes value"));
                }
                values.push(Value::Bytes(cursor[..len].to_vec()));
                cursor = &cursor[len..];
            }
            other => {
                return Err(crate::Error::corruption(format!(
                    "unknown none value tag {other}"
                )))
            }
        }
    }
    Ok(values)
}

fn encode_values_gorilla(samples: &[Sample], buf: &mut Vec<u8>) -> crate::Result<()> {
    if samples.is_empty() {
        return Ok(());
    }
    let mut bits = BitWriter::new();
    // Header: first value as-is.
    let first = samples[0].value_f64()?;
    bits.write_u64(64, first.to_bits())?;
    let mut prev_bits: u64 = first.to_bits();
    let mut prev_leading: u32 = 0;
    let mut prev_trailing: u32 = 0;

    for sample in &samples[1..] {
        let bits_value = sample.value_f64()?.to_bits();
        let xor = prev_bits ^ bits_value;
        if xor == 0 {
            bits.write(1, 0)?;
        } else {
            bits.write(1, 1)?;
            let leading = xor.leading_zeros();
            let trailing = xor.trailing_zeros();
            if leading >= prev_leading && trailing >= prev_trailing {
                bits.write(1, 0)?;
                let meaningful = 64 - prev_leading - prev_trailing;
                if meaningful > 0 {
                    bits.write(meaningful as usize, xor >> prev_trailing)?;
                }
            } else {
                bits.write(1, 1)?;
                bits.write(6, leading as u64)?;
                let meaningful = 64 - leading - trailing;
                bits.write(6, meaningful as u64)?;
                if meaningful > 0 {
                    bits.write(meaningful as usize, xor >> trailing)?;
                }
                prev_leading = leading;
                prev_trailing = trailing;
            }
        }
        prev_bits = bits_value;
    }
    bits.flush(buf);
    Ok(())
}

fn decode_values_gorilla(cursor: &[u8], count: usize) -> crate::Result<Vec<Value>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let mut bits = BitReader::new(cursor);
    let mut values = Vec::with_capacity(count);
    let first_bits = bits.read(64)?;
    let mut prev_bits = first_bits;
    values.push(Value::F64(f64::from_bits(prev_bits)));
    let mut prev_leading: u32 = 0;
    let mut prev_trailing: u32 = 0;

    for _ in 1..count {
        let same = bits.read(1)?;
        if same == 0 {
            values.push(Value::F64(f64::from_bits(prev_bits)));
            continue;
        }
        let same_block = bits.read(1)?;
        let (_leading, meaningful) = if same_block == 1 {
            let leading = bits.read(6)? as u32;
            let meaningful = bits.read(6)? as u32;
            prev_leading = leading;
            prev_trailing = 64 - leading - meaningful;
            (leading, meaningful)
        } else {
            (prev_leading, 64 - prev_leading - prev_trailing)
        };
        let xor = if meaningful == 0 {
            0
        } else {
            bits.read(meaningful as usize)? << prev_trailing
        };
        let value_bits = prev_bits ^ xor;
        values.push(Value::F64(f64::from_bits(value_bits)));
        prev_bits = value_bits;
    }
    Ok(values)
}

fn encode_values_zstd(samples: &[Sample], buf: &mut Vec<u8>) -> crate::Result<()> {
    // Serialize raw values, then compress the whole block.
    let mut raw = Vec::new();
    encode_values_none(samples, &mut raw)?;
    let compressed = zstd::bulk::compress(&raw, 3)
        .map_err(|e| crate::Error::compression(format!("zstd: {e}")))?;
    buf.put_u32_le(compressed.len() as u32);
    buf.extend_from_slice(&compressed);
    Ok(())
}

fn decode_values_zstd(cursor: &[u8], count: usize) -> crate::Result<Vec<Value>> {
    if cursor.len() < 4 {
        return Err(crate::Error::corruption("zstd value header truncated"));
    }
    let mut c = cursor;
    let len = c.get_u32_le() as usize;
    if c.len() < len {
        return Err(crate::Error::corruption("zstd value payload truncated"));
    }
    let raw = zstd::bulk::decompress(&c[..len], 16 * 1024 * 1024)
        .map_err(|e| crate::Error::compression(format!("zstd: {e}")))?;
    decode_values_none(&raw, count)
}

// ---------------------------------------------------------------------------
// Variable-length integer helpers.
// ---------------------------------------------------------------------------

fn encode_varint(mut value: u64, buf: &mut Vec<u8>) {
    while value >= 0x80 {
        buf.push((value as u8) | 0x80);
        value >>= 7;
    }
    buf.push(value as u8);
}

fn encode_varint_signed(value: i64, buf: &mut Vec<u8>) {
    encode_varint(((value << 1) ^ (value >> 63)) as u64, buf);
}

fn decode_varint(cursor: &mut &[u8]) -> u64 {
    let mut result = 0u64;
    let mut shift = 0;
    loop {
        if cursor.is_empty() {
            break;
        }
        let byte = cursor[0];
        *cursor = &cursor[1..];
        result |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
    }
    result
}

fn decode_varint_signed(cursor: &mut &[u8]) -> i64 {
    let z = decode_varint(cursor);
    ((z >> 1) as i64) ^ -((z & 1) as i64)
}

// ---------------------------------------------------------------------------
// Bit reader/writer.
// ---------------------------------------------------------------------------

struct BitWriter {
    buffer: u8,
    bits_in_buffer: u8,
    output: Vec<u8>,
}

impl BitWriter {
    fn new() -> Self {
        Self {
            buffer: 0,
            bits_in_buffer: 0,
            output: Vec::new(),
        }
    }

    fn write(&mut self, n: usize, value: u64) -> crate::Result<()> {
        if n == 0 {
            return Ok(());
        }
        if n > 64 {
            return Err(crate::Error::corruption("bit write exceeds 64 bits"));
        }
        let mut n = n;
        while n > 0 {
            let space = 8 - self.bits_in_buffer;
            let take = n.min(space as usize);
            let shift = n - take;
            let mask = if take == 8 {
                0xff
            } else {
                (1u64 << take) - 1
            };
            let bits = ((value >> shift) & mask) as u8;
            if take == 8 {
                self.output.push(bits);
            } else {
                self.buffer = (self.buffer << take) | bits;
                self.bits_in_buffer += take as u8;
                if self.bits_in_buffer == 8 {
                    self.output.push(self.buffer);
                    self.buffer = 0;
                    self.bits_in_buffer = 0;
                }
            }
            n -= take;
        }
        Ok(())
    }

    fn write_u64(&mut self, n: usize, value: u64) -> crate::Result<()> {
        self.write(n, value)
    }

    fn flush(mut self, buf: &mut Vec<u8>) {
        if self.bits_in_buffer > 0 {
            self.buffer <<= 8 - self.bits_in_buffer;
            self.output.push(self.buffer);
        }
        buf.extend_from_slice(&self.output);
    }
}

struct BitReader<'a> {
    data: &'a [u8],
    byte_offset: usize,
    bit_offset: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_offset: 0,
            bit_offset: 0,
        }
    }

    fn read(&mut self, n: usize) -> crate::Result<u64> {
        if n == 0 {
            return Ok(0);
        }
        if n > 64 {
            return Err(crate::Error::corruption("bit read exceeds 64 bits"));
        }
        let mut result: u64 = 0;
        let mut remaining = n;
        while remaining > 0 {
            if self.byte_offset >= self.data.len() {
                return Err(crate::Error::corruption("bit reader exhausted"));
            }
            let available = 8 - self.bit_offset;
            let take = remaining.min(available as usize);
            let shift = available as usize - take;
            let mask: u16 = if take == 8 {
                0xff
            } else {
                (1u16 << take) - 1
            };
            let bits = ((self.data[self.byte_offset] >> shift) as u16 & mask) as u64;
            result = (result << take) | bits;
            self.bit_offset += take as u8;
            if self.bit_offset == 8 {
                self.bit_offset = 0;
                self.byte_offset += 1;
            }
            remaining -= take;
        }
        Ok(result)
    }

}

impl Sample {
    /// Helper to extract the f64 value.
    fn value_f64(&self) -> crate::Result<f64> {
        match self.value {
            Value::F64(v) => Ok(v),
            Value::Bytes(_) => Err(crate::Error::invalid_argument(
                "expected f64 value",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gorilla_roundtrip() {
        let samples: Vec<Sample> = (0..100u64)
            .map(|i| Sample {
                timestamp: i,
                value: Value::F64(i as f64 + (i as f64) / 100.0),
            })
            .collect();
        let mut buf = Vec::new();
        encode_values_gorilla(&samples, &mut buf).unwrap();
        let decoded = decode_values_gorilla(&buf, samples.len()).unwrap();
        assert_eq!(
            samples.iter().map(|s| s.value.clone()).collect::<Vec<_>>(),
            decoded
        );
    }

    #[test]
    fn gorilla_same_value_uses_one_bit() {
        let samples = vec![
            Sample {
                timestamp: 0,
                value: Value::F64(1.0),
            },
            Sample {
                timestamp: 1,
                value: Value::F64(1.0),
            },
            Sample {
                timestamp: 2,
                value: Value::F64(1.0),
            },
        ];
        let mut buf = Vec::new();
        encode_values_gorilla(&samples, &mut buf).unwrap();
        assert!(!buf.is_empty());
        let decoded = decode_values_gorilla(&buf, samples.len()).unwrap();
        assert_eq!(decoded, vec![Value::F64(1.0); 3]);
    }

    #[test]
    fn timestamp_roundtrip() {
        let timestamps: Vec<Timestamp> = vec![1000, 1010, 1025, 1040, 1060];
        let mut buf = Vec::new();
        encode_timestamps(&timestamps, &mut buf).unwrap();
        let mut cursor = buf.as_slice();
        let decoded = decode_timestamps(&mut cursor, timestamps.len()).unwrap();
        assert_eq!(timestamps, decoded);
    }

    #[test]
    fn sample_roundtrip_all_compression_kinds() {
        let samples: Vec<Sample> = (0..50u64)
            .map(|i| Sample {
                timestamp: 1_000_000_000 + i * 10,
                value: Value::F64(i as f64),
            })
            .collect();
        for compression in [CompressionKind::None, CompressionKind::Gorilla] {
            let encoded = encode_samples(&samples, compression).unwrap();
            let decoded = decode_samples(&encoded, compression).unwrap();
            assert_eq!(samples, decoded);
        }
    }

    #[test]
    fn zstd_bytes_roundtrip() {
        let samples: Vec<Sample> = (0..10u64)
            .map(|i| Sample {
                timestamp: i,
                value: Value::Bytes(format!("payload-{i}").into_bytes()),
            })
            .collect();
        let encoded = encode_samples(&samples, CompressionKind::Zstd).unwrap();
        let decoded = decode_samples(&encoded, CompressionKind::Zstd).unwrap();
        assert_eq!(samples, decoded);
    }

    #[test]
    fn empty_sample_roundtrip() {
        let samples: Vec<Sample> = Vec::new();
        for compression in [CompressionKind::None, CompressionKind::Gorilla, CompressionKind::Zstd] {
            let encoded = encode_samples(&samples, compression).unwrap();
            let decoded = decode_samples(&encoded, compression).unwrap();
            assert!(decoded.is_empty());
        }
    }
}
