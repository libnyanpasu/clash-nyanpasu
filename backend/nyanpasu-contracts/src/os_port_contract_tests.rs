use std::{error::Error, io, path::PathBuf};

use nyanpasu_config::application::ClashCore;
use serde::Serialize;
use serde_json::json;
use snafu::ResultExt as _;

use crate::{
    core_install::InstallCoreBinaryError,
    service::{ResolveServiceDirsSnafu, ServiceCommand, ServiceCommandError},
    system_dns::SystemDnsError,
    system_proxy::{AutoLaunchError, OsProxyError, PacError, ReadOsProxySnafu},
};

fn assert_error(error: &dyn Error, display: &str, sources: &[&str]) {
    assert_eq!(error.to_string(), display);
    let mut actual = Vec::new();
    let mut source = error.source();
    while let Some(cause) = source {
        actual.push(cause.to_string());
        source = cause.source();
    }
    assert_eq!(actual, sources, "source chain for {display}");
}

fn assert_wire(error: &impl Serialize, expected: serde_json::Value) {
    assert_eq!(serde_json::to_value(error).unwrap(), expected);
}

#[test]
fn service_error_keeps_display_source_and_wire_shape() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing-service");
    let io_error = || std::process::Command::new(&missing).output().unwrap_err();
    let io_text = io_error().to_string();
    for (error, display, wire) in [
        (
            ServiceCommandError::RunElevated {
                command: ServiceCommand::Install,
                source: io_error(),
            },
            format!("could not run the service install command: {io_text}"),
            json!({"kind": "run_elevated", "command": "install"}),
        ),
        (
            ServiceCommandError::RunServiceStatus { source: io_error() },
            format!("could not query the service status: {io_text}"),
            json!({"kind": "run_service_status"}),
        ),
    ] {
        assert_error(&error, &display, &[&io_text]);
        assert!(error.source().unwrap().is::<io::Error>());
        assert_wire(&error, wire);
    }
    #[cfg(windows)]
    {
        let error = ServiceCommandError::ResolveServiceUser { source: io_error() };
        assert_error(
            &error,
            &format!("could not resolve the current user: {io_text}"),
            &[&io_text],
        );
        assert_wire(&error, json!({"kind": "resolve_service_user"}));
    }
    let error = nyanpasu_paths::installation_dir(&missing)
        .context(ResolveServiceDirsSnafu)
        .unwrap_err();
    let path_io = std::fs::canonicalize(&missing).unwrap_err().to_string();
    assert_error(
        &error,
        "could not resolve the application directories: failed to canonicalize the executable path",
        &["failed to canonicalize the executable path", &path_io],
    );
    assert_wire(&error, json!({"kind": "resolve_service_dirs"}));

    let error = ServiceCommandError::ServiceStatusNotUtf8 {
        source: String::from_utf8(vec![0xff]).unwrap_err(),
    };
    assert_error(
        &error,
        "the service status is not valid UTF-8",
        &["invalid utf-8 sequence of 1 bytes from index 0"],
    );
    assert_wire(&error, json!({"kind": "service_status_not_utf8"}));
    let error = ServiceCommandError::ParseServiceStatus {
        source: serde_json::from_str::<serde_json::Value>("{").unwrap_err(),
    };
    assert_error(
        &error,
        "could not parse the service status: EOF while parsing an object at line 1 column 1",
        &["EOF while parsing an object at line 1 column 1"],
    );
    assert_wire(&error, json!({"kind": "parse_service_status"}));

    for (error, display, wire) in [
        (
            ServiceCommandError::SessionEnding,
            "Windows is ending the session; service commands are unavailable",
            json!({"kind":"session_ending"}),
        ),
        (
            ServiceCommandError::ServiceCommandExit {
                command: ServiceCommand::Update,
                exit_code: Some(5),
                signal: Some(9),
            },
            "the service update command failed, exit code: Some(5), signal: Some(9)",
            json!({"kind":"service_command_exit","command":"update","exit_code":5,"signal":9}),
        ),
        (
            ServiceCommandError::ServiceStatusExit {
                exit_code: None,
                signal: Some(9),
            },
            "the service status query failed, exit code: None, signal: Some(9)",
            json!({"kind":"service_status_exit","exit_code":null,"signal":9}),
        ),
        (
            ServiceCommandError::TimedOut { limit_ms: 42 },
            "timed out after 42 ms",
            json!({"kind":"timed_out","limit_ms":42}),
        ),
        (
            ServiceCommandError::StillRunning { limit_ms: 42 },
            "timed out after 42 ms; the command is still running",
            json!({"kind":"still_running","limit_ms":42}),
        ),
        (
            ServiceCommandError::TaskCancelled,
            "the command's task was cancelled",
            json!({"kind":"task_cancelled"}),
        ),
    ] {
        assert_error(&error, display, &[]);
        assert_wire(&error, wire);
    }
    for (command, name) in [
        (ServiceCommand::Install, "install"),
        (ServiceCommand::Update, "update"),
        (ServiceCommand::Uninstall, "uninstall"),
        (ServiceCommand::Start, "start"),
        (ServiceCommand::Stop, "stop"),
        (ServiceCommand::Restart, "restart"),
    ] {
        assert_eq!(command.to_string(), name);
        assert_wire(&command, json!(name));
    }
}

#[test]
fn os_port_errors_keep_display_source_and_wire_shape() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing");
    let io_error = || std::fs::read(&missing).unwrap_err();
    let io_text = io_error().to_string();
    let error = SystemDnsError::RunFlushCommand {
        command: "ipconfig.exe",
        source: io_error(),
    };
    assert_error(
        &error,
        "could not run ipconfig.exe to flush the system DNS cache",
        &[&io_text],
    );
    assert_wire(
        &error,
        json!({"kind":"run_flush_command","command":"ipconfig.exe"}),
    );
    let error = SystemDnsError::FlushRejected {
        command: "ipconfig.exe",
        code: Some(5),
    };
    assert_error(
        &error,
        "ipconfig.exe did not succeed (exit code Some(5))",
        &[],
    );
    assert_wire(
        &error,
        json!({"kind":"flush_rejected","command":"ipconfig.exe","code":5}),
    );
    assert_error(
        &SystemDnsError::Unsupported,
        "flushing the system DNS cache is not supported on this platform",
        &[],
    );
    assert_wire(&SystemDnsError::Unsupported, json!({"kind":"unsupported"}));

    let error = Err::<(), _>(io_error())
        .boxed()
        .context(ReadOsProxySnafu)
        .unwrap_err();
    assert_error(&error, "could not read the system proxy", &[&io_text]);
    assert!(error.source().unwrap().is::<io::Error>());
    assert_wire(&error, json!({"kind":"read_os_proxy"}));
    let error = OsProxyError::WriteOsProxy {
        enable: true,
        host: "localhost".into(),
        port: 7890,
        source: Box::new(io_error()),
    };
    assert_error(
        &error,
        "could not write the system proxy localhost:7890 (enabled: true)",
        &[&io_text],
    );
    assert_wire(
        &error,
        json!({"kind":"write_os_proxy","enable":true,"host":"localhost","port":7890}),
    );

    let error = InstallCoreBinaryError::StartElevatedCopy {
        core: ClashCore::Mihomo,
        destination: PathBuf::from("installed-core").into(),
        source: io_error(),
    };
    assert_error(
        &error,
        "could not start the elevated copy of the mihomo core to installed-core",
        &[&io_text],
    );
    assert_wire(
        &error,
        json!({"kind":"start_elevated_copy","core":"mihomo","destination":"installed-core"}),
    );
    let error = InstallCoreBinaryError::ElevatedCopyFailed {
        core: ClashCore::Mihomo,
        destination: PathBuf::from("installed-core").into(),
        exit_code: Some(5),
    };
    assert_error(
        &error,
        "the elevated copy of the mihomo core to installed-core failed (exit code Some(5))",
        &[],
    );
    assert_wire(
        &error,
        json!({"kind":"elevated_copy_failed","core":"mihomo","destination":"installed-core","exit_code":5}),
    );
    let error = InstallCoreBinaryError::PathNotUtf8 {
        path: PathBuf::from("core-path").into(),
    };
    assert_error(&error, "the path core-path is not valid UTF-8", &[]);
    assert_wire(&error, json!({"kind":"path_not_utf8","path":"core-path"}));

    // These errors intentionally have no serde implementation; their enclosing
    // application errors own the wire representation.
    for (error, display) in [
        (
            AutoLaunchError::LocateExecutable { source: io_error() },
            "could not locate the running executable",
        ),
        (
            AutoLaunchError::CanonicalizeExecutable {
                path: PathBuf::from("app"),
                source: io_error(),
            },
            "could not canonicalize the executable path app",
        ),
        (
            AutoLaunchError::BuildRegistration {
                app_path: "app".into(),
                source: Box::new(io_error()),
            },
            "could not build the auto-launch registration for app",
        ),
        (
            AutoLaunchError::ReadRegistration {
                source: Box::new(io_error()),
            },
            "could not read the auto-launch registration",
        ),
        (
            AutoLaunchError::Register {
                source: Box::new(io_error()),
            },
            "could not register the app for auto-launch",
        ),
    ] {
        assert_error(&error, display, &[&io_text]);
        assert!(error.source().unwrap().is::<io::Error>());
    }
    assert_error(
        &AutoLaunchError::ExecutableName {
            path: PathBuf::from("app"),
        },
        "the executable app has no file stem",
        &[],
    );
    assert_error(
        &AutoLaunchError::ExecutablePathNotUtf8 {
            path: PathBuf::from("app"),
        },
        "the executable path app is not UTF-8",
        &[],
    );
    for (error, display) in [
        (
            PacError::BuildHttpClient {
                source: Box::new(io_error()),
            },
            "could not build the PAC http client",
        ),
        (
            PacError::RequestScript {
                url: "https://pac.test".into(),
                source: Box::new(io_error()),
            },
            "could not request the PAC script at https://pac.test",
        ),
        (
            PacError::ReadScriptBody {
                url: "https://pac.test".into(),
                source: Box::new(io_error()),
            },
            "could not read the PAC script body from https://pac.test",
        ),
        (
            PacError::InstallPac {
                url: "https://pac.test".into(),
                source: Box::new(io_error()),
            },
            "could not install the PAC url https://pac.test",
        ),
        (
            PacError::ClearPac {
                source: Box::new(io_error()),
            },
            "could not clear the PAC url",
        ),
    ] {
        assert_error(&error, display, &[&io_text]);
        assert!(error.source().unwrap().is::<io::Error>());
    }
    let error = PacError::ScriptNotUtf8 {
        url: "https://pac.test".into(),
        source: String::from_utf8(vec![0xff]).unwrap_err(),
    };
    assert_error(
        &error,
        "the PAC script at https://pac.test is not valid UTF-8",
        &["invalid utf-8 sequence of 1 bytes from index 0"],
    );
    for (error, display) in [
        (
            PacError::ScriptStatus {
                url: "https://pac.test".into(),
                status: 404,
            },
            "the PAC script at https://pac.test answered with HTTP 404",
        ),
        (
            PacError::ScriptTooLarge {
                url: "https://pac.test".into(),
                limit: 42,
            },
            "the PAC script at https://pac.test is over the 42 bytes allowed",
        ),
        (
            PacError::MissingEntryPoint {
                url: "https://pac.test".into(),
            },
            "the PAC script at https://pac.test has no FindProxyForURL function",
        ),
        (
            PacError::DownloadCancelled,
            "the PAC download was cancelled by the shutdown",
        ),
    ] {
        assert_error(&error, display, &[]);
    }
}

#[test]
fn elevation_refusal_keeps_display_source_and_wire_shape() {
    use crate::elevation::{ElevationConsentError, ElevationOperation};

    for (operation, display, wire) in [
        (
            ElevationOperation::Service {
                command: ServiceCommand::Restart,
            },
            "administrator access cannot be requested for the service restart command",
            json!({"kind":"service", "command":"restart"}),
        ),
        (
            ElevationOperation::DnsFlush,
            "administrator access cannot be requested for the DNS cache flush",
            json!({"kind":"dns_flush"}),
        ),
        (
            ElevationOperation::CoreBinaryCopy,
            "administrator access cannot be requested for the core binary copy",
            json!({"kind":"core_binary_copy"}),
        ),
    ] {
        let error = ElevationConsentError::AuthorizeElevation { operation };
        assert_error(&error, display, &[]);
        assert_wire(
            &error,
            json!({"kind":"authorize_elevation", "operation":wire}),
        );
    }
}

#[test]
fn consent_consumer_errors_keep_typed_sources_and_wire_shape() {
    use crate::elevation::{ElevationConsentError, ElevationOperation};

    let refusal = |operation| ElevationConsentError::AuthorizeElevation { operation };
    let service_operation = ElevationOperation::Service {
        command: ServiceCommand::Install,
    };
    let service = ServiceCommandError::AuthorizeServiceCommand {
        command: ServiceCommand::Install,
        source: refusal(service_operation),
    };
    let dns = SystemDnsError::AuthorizeDnsFlush {
        source: refusal(ElevationOperation::DnsFlush),
    };
    let copy = InstallCoreBinaryError::AuthorizeCoreCopy {
        core: ClashCore::Mihomo,
        destination: PathBuf::from("installed-core").into(),
        source: refusal(ElevationOperation::CoreBinaryCopy),
    };
    let service_refusal = refusal(service_operation).to_string();
    for (error, operation, display) in [
        (
            &service as &dyn Error,
            service_operation,
            format!("could not authorize the service install command: {service_refusal}"),
        ),
        (
            &dns as &dyn Error,
            ElevationOperation::DnsFlush,
            "could not authorize flushing the system DNS cache".to_owned(),
        ),
        (
            &copy as &dyn Error,
            ElevationOperation::CoreBinaryCopy,
            format!(
                "could not authorize copying the {} core to installed-core",
                ClashCore::Mihomo
            ),
        ),
    ] {
        let source_text = refusal(operation).to_string();
        assert_error(error, &display, &[&source_text]);
        let source = error
            .source()
            .unwrap()
            .downcast_ref::<ElevationConsentError>()
            .unwrap();
        assert!(
            matches!(source, ElevationConsentError::AuthorizeElevation { operation: actual } if *actual == operation)
        );
    }
    assert_wire(
        &service,
        json!({"kind":"authorize_service_command", "command":"install",
        "source":{"kind":"authorize_elevation", "operation":{"kind":"service", "command":"install"}}}),
    );
    assert_wire(
        &dns,
        json!({"kind":"authorize_dns_flush",
        "source":{"kind":"authorize_elevation", "operation":{"kind":"dns_flush"}}}),
    );
    assert_wire(
        &copy,
        json!({"kind":"authorize_core_copy", "core":"mihomo", "destination":"installed-core",
        "source":{"kind":"authorize_elevation", "operation":{"kind":"core_binary_copy"}}}),
    );
}
