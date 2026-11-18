//! Single-target resolution.
//!
//! The CLI selects exactly one target, then a transport for it. A failed connection
//! never falls back to another context, another instance or another transport.
//!
//! Target precedence: `--context`, else `NYANPASU_URL`, else the current context,
//! else the default local socket.

use std::path::PathBuf;

use clap::ValueEnum;
use nyanpasu_contracts::auth::TlsFingerprint;
use reqwest::Url;

use crate::{
    config::{CliConfig, ContextKind, TokenBackend, unknown_context},
    credentials::{TokenVault, clean_token},
    error::CliError,
};

/// The `--transport` choice. It narrows the resolved target; it never selects another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TransportChoice {
    /// Use the resolved target's own transport.
    Auto,
    /// Require a local socket target.
    Socket,
    /// Require an HTTP target.
    Http,
}

/// Process inputs the resolution depends on, injected so tests need no real environment.
#[derive(Debug, Clone, Default)]
pub struct Environment {
    /// `NYANPASU_URL`.
    pub url: Option<String>,
    /// `NYANPASU_TOKEN`.
    pub token: Option<String>,
    /// `NYANPASU_FINGERPRINT`: the certificate a `NYANPASU_URL` target pins.
    pub fingerprint: Option<String>,
}

impl Environment {
    /// Reads the process environment. Empty values count as unset.
    pub fn from_process() -> Self {
        let read = |name| std::env::var(name).ok().filter(|value| !value.is_empty());
        Self {
            url: read("NYANPASU_URL"),
            token: read("NYANPASU_TOKEN"),
            fingerprint: read("NYANPASU_FINGERPRINT"),
        }
    }
}

/// Which rule selected the target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetSource {
    /// `--context <name>`.
    ContextFlag(String),
    /// `NYANPASU_URL`.
    EnvironmentUrl,
    /// The `current_context` of `cli.toml`.
    CurrentContext(String),
    /// Nothing selected a target.
    DefaultSocket,
}

/// The concrete place to connect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetKind {
    /// The same-user local socket or named pipe; `endpoint` overrides discovery.
    LocalSocket { endpoint: Option<PathBuf> },
    /// An HTTP(S) base URL.
    Http { url: Url },
}

/// One resolved target and where its credential comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTarget {
    /// Which rule selected it.
    pub source: TargetSource,
    /// Where to connect.
    pub kind: TargetKind,
    /// The context that owns the token reference, when a context was selected.
    pub context: Option<String>,
    /// Where that context stored its token.
    pub token_backend: Option<TokenBackend>,
    /// The certificate an `https` target must present; without it the system trust store decides.
    pub fingerprint: Option<TlsFingerprint>,
}

impl ResolvedTarget {
    /// A short description for error messages.
    pub fn describe(&self) -> String {
        match &self.kind {
            TargetKind::LocalSocket { endpoint: None } => "the local socket".to_owned(),
            TargetKind::LocalSocket {
                endpoint: Some(path),
            } => format!("the local socket {}", path.display()),
            TargetKind::Http { url } => format!("{url}"),
        }
    }
}

/// Parses an HTTP base URL: an `http`/`https` URL with a host and no credentials.
/// A trailing slash is added so request paths join below it.
///
/// # Errors
/// Anything else is a usage error.
pub fn parse_http_url(raw: &str) -> Result<Url, CliError> {
    let mut url = Url::parse(raw)
        .map_err(|error| CliError::usage(format!("invalid URL `{raw}`: {error}")))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(CliError::usage(format!(
            "`{raw}` is not an http(s) URL with a host"
        )));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(CliError::usage(
            "put no credentials in the URL; tokens come from NYANPASU_TOKEN or the context",
        ));
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn from_context(
    config: &CliConfig,
    name: &str,
    source: TargetSource,
) -> Result<ResolvedTarget, CliError> {
    let context = config
        .contexts
        .get(name)
        .ok_or_else(|| unknown_context(name))?;
    let kind = match context.kind {
        ContextKind::LocalSocket => TargetKind::LocalSocket {
            endpoint: context.endpoint.clone(),
        },
        ContextKind::Http => {
            let url = context
                .url
                .as_deref()
                .ok_or_else(|| CliError::usage(format!("context `{name}` has no URL")))?;
            TargetKind::Http {
                url: parse_http_url(url)?,
            }
        }
    };
    Ok(ResolvedTarget {
        source,
        kind,
        context: Some(name.to_owned()),
        token_backend: context.token,
        fingerprint: context.fingerprint,
    })
}

/// Resolves exactly one target, then checks the transport choice against it.
///
/// # Errors
/// An unknown context, an invalid URL or a transport that does not fit the target is
/// a usage error. No branch retries with a different target.
pub fn resolve(
    config: &CliConfig,
    context_flag: Option<&str>,
    transport: TransportChoice,
    environment: &Environment,
) -> Result<ResolvedTarget, CliError> {
    let target = if let Some(name) = context_flag {
        from_context(config, name, TargetSource::ContextFlag(name.to_owned()))?
    } else if let Some(url) = &environment.url {
        ResolvedTarget {
            source: TargetSource::EnvironmentUrl,
            kind: TargetKind::Http {
                url: parse_http_url(url)?,
            },
            context: None,
            token_backend: None,
            fingerprint: environment
                .fingerprint
                .as_deref()
                .map(|text| {
                    text.parse().map_err(|error| {
                        CliError::usage(format!("NYANPASU_FINGERPRINT is invalid: {error}"))
                    })
                })
                .transpose()?,
        }
    } else if let Some(name) = &config.current_context {
        from_context(config, name, TargetSource::CurrentContext(name.clone()))?
    } else {
        ResolvedTarget {
            source: TargetSource::DefaultSocket,
            kind: TargetKind::LocalSocket { endpoint: None },
            context: None,
            token_backend: None,
            fingerprint: None,
        }
    };
    match (transport, &target.kind) {
        (TransportChoice::Socket, TargetKind::Http { .. }) => Err(CliError::usage(format!(
            "--transport socket does not fit {}, which is an HTTP target",
            target.describe()
        ))),
        (TransportChoice::Http, TargetKind::LocalSocket { .. }) => Err(CliError::usage(format!(
            "--transport http does not fit {}, which is a local socket target",
            target.describe()
        ))),
        _ => Ok(target),
    }
}

/// The bearer token for an HTTP target. `NYANPASU_TOKEN` overrides the context's
/// stored token; a local socket target never sends one.
///
/// # Errors
/// A context that recorded a token which is no longer stored is an authentication
/// failure, so the user re-adds it instead of silently calling without credentials.
pub fn resolve_token(
    target: &ResolvedTarget,
    environment: &Environment,
    vault: &TokenVault,
) -> Result<Option<String>, CliError> {
    if !matches!(target.kind, TargetKind::Http { .. }) {
        return Ok(None);
    }
    if let Some(token) = &environment.token {
        return clean_token(token).map(Some);
    }
    let (Some(name), Some(backend)) = (&target.context, target.token_backend) else {
        return Ok(None);
    };
    vault
        .load(name, backend)?
        .map(Some)
        .ok_or_else(|| CliError::Unauthenticated {
            message: format!(
                "the token of context `{name}` is missing from its {backend:?} store; \
                 add the context again"
            ),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::ContextConfig, credentials::fakes::MemoryStore};

    fn config() -> CliConfig {
        let mut config = CliConfig::default();
        config
            .add(
                "work",
                ContextConfig {
                    kind: ContextKind::Http,
                    endpoint: None,
                    url: Some("https://work.example:9443".to_owned()),
                    token: Some(TokenBackend::File),
                    fingerprint: None,
                },
            )
            .unwrap();
        config
            .add(
                "box",
                ContextConfig {
                    kind: ContextKind::LocalSocket,
                    endpoint: Some(PathBuf::from("/run/user/1000/box.sock")),
                    url: None,
                    token: None,
                    fingerprint: None,
                },
            )
            .unwrap();
        config
    }

    fn env(url: Option<&str>) -> Environment {
        Environment {
            url: url.map(str::to_owned),
            token: None,
            fingerprint: None,
        }
    }

    #[test]
    fn the_context_flag_beats_the_environment_and_the_current_context() {
        let config = config();
        let target = resolve(
            &config,
            Some("box"),
            TransportChoice::Auto,
            &env(Some("http://127.0.0.1:1")),
        )
        .unwrap();
        assert_eq!(target.source, TargetSource::ContextFlag("box".into()));
        assert!(matches!(target.kind, TargetKind::LocalSocket { .. }));
    }

    #[test]
    fn the_environment_url_beats_the_current_context() {
        let config = config();
        assert_eq!(config.current_context.as_deref(), Some("work"));
        let target = resolve(
            &config,
            None,
            TransportChoice::Auto,
            &env(Some("http://127.0.0.1:7777")),
        )
        .unwrap();
        assert_eq!(target.source, TargetSource::EnvironmentUrl);
        assert_eq!(target.context, None);
        assert_eq!(
            target.kind,
            TargetKind::Http {
                url: Url::parse("http://127.0.0.1:7777/").unwrap()
            }
        );
    }

    #[test]
    fn the_current_context_beats_the_default_socket() {
        let target = resolve(&config(), None, TransportChoice::Auto, &env(None)).unwrap();
        assert_eq!(target.source, TargetSource::CurrentContext("work".into()));
        assert_eq!(target.token_backend, Some(TokenBackend::File));
    }

    #[test]
    fn with_nothing_configured_the_default_socket_is_used() {
        let target = resolve(
            &CliConfig::default(),
            None,
            TransportChoice::Auto,
            &env(None),
        )
        .unwrap();
        assert_eq!(target.source, TargetSource::DefaultSocket);
        assert_eq!(target.kind, TargetKind::LocalSocket { endpoint: None });
    }

    #[test]
    fn an_unknown_context_never_falls_back() {
        let config = config();
        let error = resolve(
            &config,
            Some("missing"),
            TransportChoice::Auto,
            &env(Some("http://127.0.0.1:7777")),
        )
        .unwrap_err();
        assert!(matches!(error, CliError::Usage { .. }), "{error}");

        let mut dangling = config;
        dangling.current_context = Some("gone".to_owned());
        let error = resolve(&dangling, None, TransportChoice::Auto, &env(None)).unwrap_err();
        assert!(
            matches!(error, CliError::Usage { .. }),
            "a dangling current context must not select the default socket: {error}"
        );
    }

    #[test]
    fn an_incompatible_transport_is_a_usage_error_not_another_target() {
        let config = config();
        let error =
            resolve(&config, Some("work"), TransportChoice::Socket, &env(None)).unwrap_err();
        assert!(matches!(error, CliError::Usage { .. }), "{error}");
        let error = resolve(&config, Some("box"), TransportChoice::Http, &env(None)).unwrap_err();
        assert!(matches!(error, CliError::Usage { .. }), "{error}");

        for (context, transport) in [
            ("work", TransportChoice::Http),
            ("work", TransportChoice::Auto),
            ("box", TransportChoice::Socket),
        ] {
            resolve(&config, Some(context), transport, &env(None)).unwrap();
        }
    }

    #[test]
    fn urls_are_validated_and_normalized() {
        assert_eq!(
            parse_http_url("http://127.0.0.1:9090/base?x=1#f")
                .unwrap()
                .as_str(),
            "http://127.0.0.1:9090/base/"
        );
        for bad in [
            "ftp://h/",
            "not a url",
            "http://user:pw@h/",
            "http://tok@h/",
            "file:///x",
        ] {
            assert!(parse_http_url(bad).is_err(), "{bad}");
        }
    }

    fn vault_with(secret: Option<&str>) -> TokenVault {
        let file = MemoryStore::new(true);
        if let Some(secret) = secret {
            crate::credentials::SecretStore::set(&file, "work", secret).unwrap();
        }
        TokenVault::with_stores(Box::new(MemoryStore::new(false)), Box::new(file))
    }

    #[test]
    fn nyanpasu_token_overrides_the_stored_token_for_http_targets_only() {
        let config = config();
        let http = resolve(&config, Some("work"), TransportChoice::Auto, &env(None)).unwrap();
        let socket = resolve(&config, Some("box"), TransportChoice::Auto, &env(None)).unwrap();
        let vault = vault_with(Some("stored"));
        let mut environment = env(None);

        assert_eq!(
            resolve_token(&http, &environment, &vault)
                .unwrap()
                .as_deref(),
            Some("stored")
        );
        environment.token = Some("from-env\n".to_owned());
        assert_eq!(
            resolve_token(&http, &environment, &vault)
                .unwrap()
                .as_deref(),
            Some("from-env")
        );
        assert_eq!(resolve_token(&socket, &environment, &vault).unwrap(), None);
    }

    #[test]
    fn a_recorded_but_missing_token_is_an_authentication_failure() {
        let config = config();
        let http = resolve(&config, Some("work"), TransportChoice::Auto, &env(None)).unwrap();
        let error = resolve_token(&http, &env(None), &vault_with(None)).unwrap_err();
        assert!(matches!(error, CliError::Unauthenticated { .. }), "{error}");
    }

    #[test]
    fn an_environment_url_target_uses_only_the_environment_token() {
        let config = CliConfig::default();
        let target = resolve(
            &config,
            None,
            TransportChoice::Auto,
            &env(Some("http://127.0.0.1:1")),
        )
        .unwrap();
        let vault = vault_with(Some("stored"));
        assert_eq!(resolve_token(&target, &env(None), &vault).unwrap(), None);
        let environment = Environment {
            url: None,
            token: Some("t".to_owned()),
            fingerprint: None,
        };
        assert_eq!(
            resolve_token(&target, &environment, &vault)
                .unwrap()
                .as_deref(),
            Some("t")
        );
    }
}
