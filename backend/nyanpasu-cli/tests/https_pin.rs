//! The CLI against a real HTTPS listener: certificate pinning, trust on first use, and the
//! refusal to send a token to a server whose certificate changed.

use std::sync::Arc;

use clap::Parser as _;
use nyanpasu_cli::{
    app::{Io, RealConnector, Services, execute},
    cli::Cli,
    config::{CliConfig, CliPaths},
    credentials::TokenVault,
    error::ExitStatus,
    target::Environment,
};
use nyanpasu_config::application::{HttpAccessConfig, HttpAccessStrategy, HttpTls, Scope};
use nyanpasu_contracts::auth::{CreateAccessTokenRequest, ScopeSet, TlsFingerprint, TokenName};
use nyanpasu_rpc::{
    http::{
        HttpServerClient,
        auth::NyanpasuClientHttpAuthenticator,
        routes::RpcHttpRoutes,
        test_support::{StaticTlsIdentity, self_signed_identity},
        tls::HttpTlsIdentitySource,
    },
    test_support::Fixture,
};

struct Lan {
    fixture: Fixture,
    server: HttpServerClient,
    url: String,
    token: String,
    certificate: TlsFingerprint,
    directory: tempfile::TempDir,
}

impl Lan {
    async fn start() -> Self {
        let fixture = Fixture::new(false).await;
        let routes = Arc::new(RpcHttpRoutes::default());
        routes.install(&fixture.rpc).unwrap();
        let identity = self_signed_identity(&[]);
        let certificate = identity.fingerprint();
        let server = HttpServerClient::spawn_with_tls(
            None,
            routes,
            Arc::new(NyanpasuClientHttpAuthenticator::new(
                &fixture.rpc.dependencies().client,
            )),
            Some(Arc::new(StaticTlsIdentity::new(identity)) as Arc<dyn HttpTlsIdentitySource>),
        )
        .await
        .unwrap();
        let status = server
            .configure(&HttpAccessConfig {
                enabled: true,
                strategy: HttpAccessStrategy::LanAuthenticated,
                tls: HttpTls::SelfSigned,
                ..Default::default()
            })
            .await
            .unwrap();
        let token = fixture
            .rpc
            .dependencies()
            .client
            .create_remote_access_token(CreateAccessTokenRequest {
                name: TokenName::parse("pin-test").unwrap(),
                scopes: ScopeSet::from([Scope::Read]),
                expires_at_ms: None,
            })
            .await
            .unwrap()
            .token
            .expose_secret()
            .to_owned();
        Self {
            fixture,
            server,
            url: status.url.unwrap(),
            token,
            certificate,
            directory: tempfile::tempdir().unwrap(),
        }
    }

    fn services(&self) -> Services<RealConnector> {
        Services {
            paths: CliPaths::discover(Some(self.directory.path().join("cli.toml"))).unwrap(),
            vault: TokenVault::new(self.directory.path().join("tokens"), false),
            connector: RealConnector,
            home: None,
            executable: None,
            skill_dir: None,
            appimage: None,
            search_path: None,
        }
    }

    /// Runs the CLI, as `NYANPASU_URL`, `NYANPASU_TOKEN` and `NYANPASU_FINGERPRINT` would.
    async fn run(
        &self,
        fingerprint: Option<TlsFingerprint>,
        arguments: &[&str],
        stdin: &str,
    ) -> (ExitStatus, String, String) {
        let environment = Environment {
            url: Some(self.url.clone()),
            token: Some(self.token.clone()),
            fingerprint: fingerprint.map(|fingerprint| fingerprint.to_string()),
        };
        self.execute(&environment, arguments, stdin).await
    }

    async fn execute(
        &self,
        environment: &Environment,
        arguments: &[&str],
        stdin: &str,
    ) -> (ExitStatus, String, String) {
        let cli =
            Cli::try_parse_from(std::iter::once("nyanpasu-cli").chain(arguments.iter().copied()))
                .unwrap();
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut stdin = stdin.as_bytes();
        let status = execute(
            cli,
            environment,
            &self.services(),
            &mut Io {
                stdin: &mut stdin,
                stdout: &mut stdout,
                stderr: &mut stderr,
            },
        )
        .await;
        (
            status,
            String::from_utf8(stdout).unwrap(),
            String::from_utf8(stderr).unwrap(),
        )
    }

    async fn token_was_used(&self) -> bool {
        self.fixture
            .rpc
            .dependencies()
            .client
            .list_remote_access_tokens()
            .await
            .unwrap()
            .iter()
            .any(|token| token.last_used_at_ms.is_some())
    }

    async fn stop(self) {
        self.server.shutdown().await.unwrap();
        self.fixture.shutdown().await;
    }
}

#[tokio::test]
async fn the_pinned_certificate_is_accepted() {
    let lan = Lan::start().await;
    let (status, stdout, stderr) = lan.run(Some(lan.certificate), &["methods"], "").await;
    assert_eq!(status, ExitStatus::Success, "{stderr}");
    assert!(!stdout.is_empty());
    assert!(lan.token_was_used().await);
    lan.stop().await;
}

#[tokio::test]
async fn another_certificate_is_refused_before_the_token_is_sent() {
    let lan = Lan::start().await;
    let other = self_signed_identity(&[]).fingerprint();
    let (status, _, stderr) = lan.run(Some(other), &["methods"], "").await;

    assert_eq!(status, ExitStatus::Unauthenticated, "{stderr}");
    assert!(stderr.contains(&other.to_string()), "{stderr}");
    assert!(stderr.contains(&lan.certificate.to_string()), "{stderr}");
    assert!(
        !lan.token_was_used().await,
        "the token must not reach a server that does not match"
    );
    lan.stop().await;
}

#[tokio::test]
async fn an_unpinned_self_signed_server_is_not_trusted_by_the_system_store() {
    let lan = Lan::start().await;
    let (status, _, stderr) = lan.run(None, &["methods"], "").await;
    assert_eq!(status, ExitStatus::Unreachable, "{stderr}");
    assert!(!lan.token_was_used().await);
    lan.stop().await;
}

#[tokio::test]
async fn trust_on_first_use_shows_the_real_fingerprint_and_pins_it_for_later_commands() {
    let lan = Lan::start().await;
    let environment = Environment::default();
    let add = ["context", "add", "lan", "--url", lan.url.as_str()];

    let (status, _, stderr) = lan.execute(&environment, &add, "n\n").await;
    assert_eq!(status, ExitStatus::Usage, "{stderr}");
    assert!(stderr.contains(&lan.certificate.to_string()), "{stderr}");
    assert!(
        !lan.services().paths.config_file.exists(),
        "a declined pin saves nothing"
    );

    let (status, _, stderr) = lan.execute(&environment, &add, "y\n").await;
    assert_eq!(status, ExitStatus::Success, "{stderr}");
    let saved = CliConfig::load(&lan.services().paths.config_file).unwrap();
    assert_eq!(saved.contexts["lan"].fingerprint, Some(lan.certificate));

    // The saved pin now verifies later commands; the token comes from the environment.
    let environment = Environment {
        token: Some(lan.token.clone()),
        ..Default::default()
    };
    let (status, _, stderr) = lan.execute(&environment, &["methods"], "").await;
    assert_eq!(status, ExitStatus::Success, "{stderr}");
    lan.stop().await;
}
