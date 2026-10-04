use anyhow::{Result, ensure};
use nyanpasu_config::application::CoreLogCompression;

use super::model::MAX_RECORD_BYTES;

const PRESET: &[u8] = include_bytes!("preset.zdict");
const HEADER: usize = 6;
const SAMPLE_BYTES: usize = 2 * 1024 * 1024;
const DICTIONARY_BYTES: usize = 32 * 1024;

#[derive(Default)]
struct Samples {
    bytes: Vec<u8>,
    sizes: Vec<usize>,
}

pub(super) struct LogEncoder {
    compressor: Option<zstd::bulk::Compressor<'static>>,
    dictionary: Vec<u8>,
    samples: Option<Samples>,
}

impl LogEncoder {
    pub fn new(mode: CoreLogCompression) -> Result<Self> {
        let enabled = mode != CoreLogCompression::None;
        Ok(Self {
            compressor: enabled
                .then(|| zstd::bulk::Compressor::with_dictionary(3, PRESET))
                .transpose()?,
            dictionary: if enabled { PRESET.to_vec() } else { Vec::new() },
            samples: (mode == CoreLogCompression::Trained).then(Samples::default),
        })
    }

    pub fn dictionary(&self) -> &[u8] {
        &self.dictionary
    }

    pub fn encode(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        if let Some(samples) = &mut self.samples {
            let length = bytes
                .len()
                .min(16 * 1024)
                .min(SAMPLE_BYTES - samples.bytes.len());
            if length > 0 {
                samples.bytes.extend_from_slice(&bytes[..length]);
                samples.sizes.push(length);
            }
        }
        let compressed = self
            .compressor
            .as_mut()
            .map(|encoder| encoder.compress(bytes))
            .transpose()?;
        let compressed = compressed
            .as_deref()
            .filter(|encoded| encoded.len() < bytes.len());
        let body = compressed.unwrap_or(bytes);
        let mut encoded = Vec::with_capacity(HEADER + body.len());
        encoded.extend_from_slice(&[1, u8::from(compressed.is_some())]);
        encoded.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        encoded.extend_from_slice(body);
        Ok(encoded)
    }

    /// Called between batches. A successful change must start a new shard.
    pub fn train(&mut self) -> Result<bool> {
        if self
            .samples
            .as_ref()
            .is_none_or(|samples| samples.bytes.len() < SAMPLE_BYTES)
        {
            return Ok(false);
        }
        let samples = self.samples.take().unwrap();
        match zstd::dict::from_continuous(&samples.bytes, &samples.sizes, DICTIONARY_BYTES) {
            Ok(dictionary) => {
                self.compressor
                    .as_mut()
                    .unwrap()
                    .set_dictionary(3, &dictionary)?;
                self.dictionary = dictionary;
                Ok(true)
            }
            Err(error) => {
                tracing::warn!(%error, "Core log dictionary training failed; keeping preset");
                Ok(false)
            }
        }
    }
}

pub(super) fn original_size(bytes: &[u8]) -> Result<usize> {
    ensure!(
        bytes.len() >= HEADER && bytes[0] == 1 && bytes[1] <= 1,
        "Invalid core log envelope"
    );
    let length = u32::from_le_bytes(bytes[2..HEADER].try_into().unwrap()) as usize;
    ensure!(
        length <= MAX_RECORD_BYTES && bytes.len() <= MAX_RECORD_BYTES + HEADER,
        "Core log record exceeds its size limit"
    );
    Ok(length)
}

pub(super) struct LogDecoder(zstd::bulk::Decompressor<'static>);
impl LogDecoder {
    pub fn new(dictionary: &[u8]) -> Result<Self> {
        Ok(Self(zstd::bulk::Decompressor::with_dictionary(dictionary)?))
    }

    pub fn decode(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        let length = original_size(bytes)?;
        let decoded = if bytes[1] == 0 {
            bytes[HEADER..].to_vec()
        } else {
            self.0.decompress(&bytes[HEADER..], length)?
        };
        ensure!(decoded.len() == length, "Core log envelope length mismatch");
        Ok(decoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic(number: usize) -> Vec<u8> {
        let payload = match number % 4 {
            0 => format!(
                "[TCP] 192.0.2.{}:{} --> example{}.test:443 match DomainSuffix(example.test) using Proxy[Example]",
                number % 250 + 1,
                10000 + number % 50000,
                number % 100
            ),
            1 => format!(
                "[DNS] example{}.test --> [198.51.100.{}] A from udp://192.0.2.53:53",
                number % 100,
                number % 250 + 1
            ),
            2 => format!(
                "[UDP] dial DIRECT 192.0.2.{}:53 error: i/o timeout",
                number % 250 + 1
            ),
            _ => format!(
                "Start initial configuration in progress; provider example{} updated; initial compatible provider default",
                number % 100
            ),
        };
        serde_json::to_vec(&serde_json::json!({
            "source": {"capture": "synthetic", "instance_id": "example-instance", "core_kind": "Mihomo"},
            "received_at": 1700000000000_u64 + number as u64,
            "time": "2024-01-01T12:34:56Z", "type": if number % 4 == 2 {"warning"} else {"info"}, "payload": payload,
        })).unwrap()
    }

    #[test]
    #[ignore = "explicitly regenerate the bundled dictionary from synthetic data"]
    fn regenerate_preset_dictionary() {
        let samples: Vec<_> = (0..12000).map(synthetic).collect();
        let dictionary = zstd::dict::from_samples(&samples, DICTIONARY_BYTES).unwrap();
        std::fs::write(
            concat!(env!("CARGO_MANIFEST_DIR"), "/src/core/logs/preset.zdict"),
            dictionary,
        )
        .unwrap();
    }

    #[test]
    fn raw_preset_and_trained_round_trip_with_bounded_sampling() {
        for mode in [
            CoreLogCompression::None,
            CoreLogCompression::Preset,
            CoreLogCompression::Trained,
        ] {
            let mut encoder = LogEncoder::new(mode).unwrap();
            let mut raw_bytes = 0;
            let mut stored_bytes = 0;
            let mut trained = 0;
            for number in 0..14000 {
                let bytes = synthetic(number);
                let encoded = encoder.encode(&bytes).unwrap();
                let mut decoder = LogDecoder::new(encoder.dictionary()).unwrap();
                assert_eq!(decoder.decode(&encoded).unwrap(), bytes);
                raw_bytes += bytes.len();
                stored_bytes += encoded.len();
                assert!(
                    encoder
                        .samples
                        .as_ref()
                        .is_none_or(|samples| samples.bytes.len() <= SAMPLE_BYTES)
                );
                trained += usize::from(encoder.train().unwrap());
            }
            assert_eq!(trained, usize::from(mode == CoreLogCompression::Trained));
            if mode != CoreLogCompression::None {
                assert!(stored_bytes < raw_bytes / 2);
            }
            println!("{mode:?}: raw={raw_bytes}, encoded={stored_bytes}");
        }
    }

    #[test]
    fn raw_fallback_and_invalid_envelopes() {
        let mut encoder = LogEncoder::new(CoreLogCompression::Preset).unwrap();
        let encoded = encoder.encode(b"x").unwrap();
        assert_eq!(encoded[1], 0);
        let mut decoder = LogDecoder::new(encoder.dictionary()).unwrap();
        assert!(decoder.decode(&[]).is_err());
        let mut corrupt = encoded.clone();
        corrupt[0] = 2;
        assert!(decoder.decode(&corrupt).is_err());
        corrupt = encoded.clone();
        corrupt[2..HEADER].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decoder.decode(&corrupt).is_err());
        corrupt = encoded;
        corrupt[2] = 2;
        assert!(decoder.decode(&corrupt).is_err());
    }
}
