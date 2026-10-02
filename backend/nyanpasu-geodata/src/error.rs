#[derive(Debug, thiserror::Error)]
pub enum GeoError {
    #[error("invalid MaxMind DB: {0}")]
    Mmdb(#[from] maxminddb::MaxMindDbError),
    #[error("unsupported ASN database type: {0}")]
    UnsupportedAsnDatabase(String),
    #[error("malformed geodata: {0}")]
    Malformed(&'static str),
    #[error("too many distinct {0}")]
    TooMany(&'static str),
    #[error("cannot map build memory: {0}")]
    Memory(std::io::Error),
}

pub type GeoResult<T> = Result<T, GeoError>;
