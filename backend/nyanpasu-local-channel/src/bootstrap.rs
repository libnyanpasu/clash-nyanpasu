use std::{env, fmt, str::FromStr};

use snafu::{OptionExt as _, ensure};

use crate::{
    error::{BootstrapError, bootstrap_error},
    wire::{TOKEN_LEN, Token},
};

const VERSION: &str = "v1";

/// What a child needs to connect to its parent's server: the socket name and
/// the one-time token, carried as one opaque string.
///
/// The string form is `v1:<64 hex digits>:<name>`, made by [`Display`] and read
/// by [`FromStr`]. Treat it as a secret for the lifetime of the server; `Debug`
/// leaves the token out.
#[derive(Clone, PartialEq, Eq)]
pub struct Bootstrap {
    name: String,
    token: Token,
}

impl Bootstrap {
    /// The environment variable the parent conventionally passes the bootstrap in.
    pub const ENV: &'static str = "NYANPASU_LOCAL_CHANNEL";

    #[cfg(any(feature = "tokio", test))]
    pub(crate) fn new(name: String, token: Token) -> Self {
        Self { name, token }
    }

    /// Reads the bootstrap from [`Bootstrap::ENV`].
    pub fn from_env() -> Result<Self, BootstrapError> {
        Self::from_env_var(Self::ENV)
    }

    /// Reads the bootstrap from the environment variable `var`.
    pub fn from_env_var(var: &str) -> Result<Self, BootstrapError> {
        match env::var(var) {
            Ok(value) => value.parse(),
            Err(env::VarError::NotPresent) => Err(BootstrapError::Missing {
                var: var.to_owned(),
            }),
            Err(env::VarError::NotUnicode(_)) => Err(BootstrapError::NotUnicode {
                var: var.to_owned(),
            }),
        }
    }

    /// The socket name: a pipe name on Windows, a socket path elsewhere.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn token(&self) -> &Token {
        &self.token
    }
}

impl fmt::Display for Bootstrap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{VERSION}:")?;
        for byte in self.token {
            write!(f, "{byte:02x}")?;
        }
        write!(f, ":{}", self.name)
    }
}

impl fmt::Debug for Bootstrap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bootstrap")
            .field("name", &self.name)
            .field("token", &"<redacted>")
            .finish()
    }
}

impl FromStr for Bootstrap {
    type Err = BootstrapError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        // The name comes last and is taken whole, so it may contain ':'.
        let mut parts = value.splitn(3, ':');
        let (Some(version), Some(token), Some(name)) = (parts.next(), parts.next(), parts.next())
        else {
            return Err(BootstrapError::Malformed);
        };
        ensure!(!name.is_empty(), bootstrap_error::MalformedSnafu);
        ensure!(
            version == VERSION,
            bootstrap_error::UnsupportedVersionSnafu { version }
        );
        Ok(Self {
            name: name.to_owned(),
            token: parse_token(token).context(bootstrap_error::InvalidTokenSnafu)?,
        })
    }
}

fn parse_token(hex: &str) -> Option<Token> {
    if hex.len() != TOKEN_LEN * 2 || !hex.is_ascii() {
        return None;
    }
    let mut token = [0; TOKEN_LEN];
    let (pairs, _) = hex.as_bytes().as_chunks::<2>();
    for (byte, pair) in token.iter_mut().zip(pairs) {
        let pair = std::str::from_utf8(pair).ok()?;
        *byte = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Bootstrap {
        let mut token = [0; TOKEN_LEN];
        for (index, byte) in token.iter_mut().enumerate() {
            *byte = index as u8 * 7;
        }
        Bootstrap::new(r"C:\odd:name\with:colons".to_owned(), token)
    }

    #[test]
    fn the_string_form_round_trips_a_name_with_colons() {
        let bootstrap = sample();
        let text = bootstrap.to_string();
        assert!(text.starts_with("v1:00070e15"), "{text}");
        assert_eq!(text.parse::<Bootstrap>().unwrap(), bootstrap);
    }

    #[test]
    fn debug_leaves_the_token_out() {
        let debug = format!("{:?}", sample());
        assert!(debug.contains("<redacted>"), "{debug}");
        assert!(!debug.contains("0e15"), "{debug}");
    }

    #[test]
    fn malformed_strings_are_refused() {
        let token = "ab".repeat(TOKEN_LEN);
        for (text, expect) in [
            ("", "malformed"),
            ("v1", "malformed"),
            (&format!("v1:{token}") as &str, "malformed"),
            (&format!("v1:{token}:"), "malformed"),
            (&format!("v2:{token}:name"), "version"),
            ("v1:abcd:name", "token"),
            (&format!("v1:{}:name", "zz".repeat(TOKEN_LEN)), "token"),
            (&format!("v1:{}:name", "é".repeat(TOKEN_LEN)), "token"),
        ] {
            let error = text.parse::<Bootstrap>().unwrap_err();
            let matched = match expect {
                "malformed" => matches!(error, BootstrapError::Malformed),
                "version" => matches!(error, BootstrapError::UnsupportedVersion { .. }),
                _ => matches!(error, BootstrapError::InvalidToken),
            };
            assert!(matched, "{text:?}: {error:?}");
        }
    }

    #[test]
    fn a_missing_variable_is_named_in_the_error() {
        let error = Bootstrap::from_env_var("NYANPASU_LOCAL_CHANNEL_TEST_NEVER_SET").unwrap_err();
        assert!(
            matches!(&error, BootstrapError::Missing { var } if var == "NYANPASU_LOCAL_CHANNEL_TEST_NEVER_SET"),
            "{error:?}"
        );
    }
}
