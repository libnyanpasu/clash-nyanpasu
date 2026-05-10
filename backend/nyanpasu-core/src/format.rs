use serde::{Serialize, de::DeserializeOwned};
use std::io::{Read, Write};

pub trait Format {
    fn serialize<W: Write, T: Serialize>(
        &self,
        writer: W,
        value: &T,
        prefix: Option<&str>,
    ) -> anyhow::Result<()>;
    fn deserialize<R: Read, T: DeserializeOwned>(&self, reader: R) -> anyhow::Result<T>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct YamlFormat;

impl Format for YamlFormat {
    fn serialize<W: Write, T: Serialize>(
        &self,
        mut writer: W,
        value: &T,
        prefix: Option<&str>,
    ) -> anyhow::Result<()> {
        if let Some(prefix) = prefix {
            writeln!(writer, "{}", prefix)?;
        }
        serde_yaml_ng::to_writer(writer, value)?;
        Ok(())
    }

    fn deserialize<R: Read, T: DeserializeOwned>(&self, reader: R) -> anyhow::Result<T> {
        let value = serde_yaml_ng::from_reader(reader)?;
        Ok(value)
    }
}
