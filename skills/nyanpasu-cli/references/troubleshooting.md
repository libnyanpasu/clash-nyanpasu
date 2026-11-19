# Targets, contexts and failures

## Contexts

A context names one target in `cli.toml`:

- `nyanpasu-cli context list` shows them, and `*` marks the current one.
- `nyanpasu-cli context use <name>` changes the default.
- `nyanpasu-cli --context <name> status` uses one context for a single invocation.

Only add or remove contexts when the user asks. Adding an HTTP context with a token
reads the token from standard input (`--token-stdin`) or from `NYANPASU_TOKEN`
(`--token-from-env`), never from an argument. An `https` context pins the app's
local certificate authority, so the short-lived certificates it renews stay trusted: the
user passes the authority fingerprint the app shows with `--fingerprint`, or
`--system-trust` for a certificate the system already trusts.
Never pin a fingerprint the user has not confirmed.

## Exit code 3: not reachable

The app is not running, the local socket switch is off, or HTTP access is off.
Ask the user to start the app or enable the switch in its settings. There is no
other instance to try: the CLI uses exactly one resolved target.

## Exit code 4: not authenticated or scope denied

The token is missing, expired or revoked, or its scopes do not include the command.
`nyanpasu-cli --json methods` lists the scopes this caller holds. Creating or
widening a token is a decision for the user; do not do it unprompted.

## Exit code 5: unsupported

The command is not exposed on this transport, or the app is older than the CLI.
The CLI and the app share a version; ask the user to update whichever is older.

## Exit code 6: applied with degradation

The change is committed. Part of its effect, such as restarting the core or applying
the system proxy, did not complete. Report the degradation lines printed on
standard error and do not repeat the command.

## Command not found

On Windows and in the Linux packages the installer puts `nyanpasu-cli` on `PATH`. For the
macOS app and the AppImage, the user installs it from the app's settings (Remote access,
Command-line tool), which runs `nyanpasu-cli install` for them. Do not link it yourself.
