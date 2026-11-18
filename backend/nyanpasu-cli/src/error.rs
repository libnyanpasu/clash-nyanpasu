//! CLI failures and their process exit codes.
//!
//! Every failure maps to exactly one documented exit status, so scripts and agents
//! can branch on the code without parsing messages.

use nyanpasu_rpc_wire::{RpcError, RpcErrorKind};
use snafu::Snafu;

/// The process exit statuses documented for `nyanpasu-cli`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitStatus {
    /// The command succeeded.
    Success = 0,
    /// The shell reported an application error.
    Application = 1,
    /// The invocation was malformed.
    Usage = 2,
    /// The app is not running, the socket is disabled or HTTP is off.
    Unreachable = 3,
    /// The caller is not authenticated, or the scope is denied.
    Unauthenticated = 4,
    /// The command is unsupported on this target or shell.
    Unsupported = 5,
    /// The change was committed but part of its effects were degraded.
    Degraded = 6,
}

impl ExitStatus {
    /// The numeric process exit code.
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// Failure of one CLI invocation. Variants carry what the exit mapping needs.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum CliError {
    /// The invocation or its configuration is invalid.
    #[snafu(display("{message}"))]
    Usage { message: String },
    /// The selected target cannot be reached. There is no fallback to another target.
    #[snafu(display("cannot reach {target}: {detail}"))]
    Unreachable { target: String, detail: String },
    /// The credential was rejected, or the principal lacks the command's scope.
    #[snafu(display("not authenticated or scope denied: {message}"))]
    Unauthenticated { message: String },
    /// The server presented a certificate that the authority the context pinned did not issue.
    /// The connection was refused before anything, including the token, was sent.
    #[snafu(display(
        "the certificate authority of {target} is {presented}, not the pinned {pinned}; compare \
         it with the app's remote access settings, then pin it again with `context add`"
    ))]
    CertificateMismatch {
        target: String,
        pinned: String,
        presented: String,
    },
    /// The target or shell does not offer the command.
    #[snafu(display("unsupported on this target: {message}"))]
    Unsupported { message: String },
    /// The shell answered with an `RpcError` envelope.
    #[snafu(display("{}", error.message))]
    Remote { error: Box<RpcError> },
    /// Any other local or protocol failure, including an unknown-outcome exchange.
    #[snafu(display("{message}"))]
    Failed { message: String },
}

impl CliError {
    /// A usage error.
    pub fn usage(message: impl Into<String>) -> Self {
        Self::Usage {
            message: message.into(),
        }
    }

    /// A local or protocol failure that is neither usage nor reachability.
    pub fn failed(message: impl Into<String>) -> Self {
        Self::Failed {
            message: message.into(),
        }
    }

    /// Wraps a shell error envelope; its `kind` selects the exit status.
    pub fn from_remote(error: RpcError) -> Self {
        Self::Remote {
            error: Box::new(error),
        }
    }

    /// The exit status this failure ends the process with.
    pub fn exit_status(&self) -> ExitStatus {
        match self {
            Self::Usage { .. } => ExitStatus::Usage,
            Self::Unreachable { .. } => ExitStatus::Unreachable,
            Self::Unauthenticated { .. } | Self::CertificateMismatch { .. } => {
                ExitStatus::Unauthenticated
            }
            Self::Unsupported { .. } => ExitStatus::Unsupported,
            Self::Failed { .. } => ExitStatus::Application,
            Self::Remote { error } => remote_exit_status(error.error_kind()),
        }
    }
}

impl From<std::io::Error> for CliError {
    fn from(error: std::io::Error) -> Self {
        Self::failed(format!("i/o error: {error}"))
    }
}

/// Maps an `RpcError.kind` to an exit status. Unclassified kinds are application errors.
fn remote_exit_status(kind: RpcErrorKind) -> ExitStatus {
    match kind {
        RpcErrorKind::Unauthorized | RpcErrorKind::Forbidden => ExitStatus::Unauthenticated,
        RpcErrorKind::Unsupported | RpcErrorKind::UnknownMethod => ExitStatus::Unsupported,
        RpcErrorKind::InvalidParams => ExitStatus::Usage,
        RpcErrorKind::ApplicationError | RpcErrorKind::Other => ExitStatus::Application,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(kind: &str) -> CliError {
        CliError::from_remote(RpcError {
            kind: kind.to_owned(),
            message: "boom".to_owned(),
            code: None,
            retryable: None,
            operation_id: None,
            domain_error: None,
        })
    }

    #[test]
    fn exit_codes_are_the_documented_values() {
        assert_eq!(ExitStatus::Success.code(), 0);
        assert_eq!(ExitStatus::Application.code(), 1);
        assert_eq!(ExitStatus::Usage.code(), 2);
        assert_eq!(ExitStatus::Unreachable.code(), 3);
        assert_eq!(ExitStatus::Unauthenticated.code(), 4);
        assert_eq!(ExitStatus::Unsupported.code(), 5);
        assert_eq!(ExitStatus::Degraded.code(), 6);
    }

    #[test]
    fn local_failures_map_to_their_exit_status() {
        assert_eq!(CliError::usage("x").exit_status(), ExitStatus::Usage);
        assert_eq!(
            CliError::Unreachable {
                target: "socket".into(),
                detail: "refused".into()
            }
            .exit_status(),
            ExitStatus::Unreachable
        );
        assert_eq!(
            CliError::Unauthenticated {
                message: "401".into()
            }
            .exit_status(),
            ExitStatus::Unauthenticated
        );
        assert_eq!(
            CliError::CertificateMismatch {
                target: "https://h/".into(),
                pinned: "AA".into(),
                presented: "BB".into()
            }
            .exit_status(),
            ExitStatus::Unauthenticated
        );
        assert_eq!(
            CliError::Unsupported {
                message: "x".into()
            }
            .exit_status(),
            ExitStatus::Unsupported
        );
        assert_eq!(
            CliError::failed("io").exit_status(),
            ExitStatus::Application
        );
    }

    #[test]
    fn remote_kinds_map_to_their_exit_status() {
        for (kind, expected) in [
            ("application_error", ExitStatus::Application),
            ("something_new", ExitStatus::Application),
            ("unsupported", ExitStatus::Unsupported),
            ("unknown_method", ExitStatus::Unsupported),
            ("unauthorized", ExitStatus::Unauthenticated),
            ("forbidden", ExitStatus::Unauthenticated),
            ("invalid_params", ExitStatus::Usage),
            // Spellings no shell emits are not guessed at.
            ("unauthenticated", ExitStatus::Application),
            ("scope_denied", ExitStatus::Application),
        ] {
            assert_eq!(remote(kind).exit_status(), expected, "{kind}");
        }
    }
}
