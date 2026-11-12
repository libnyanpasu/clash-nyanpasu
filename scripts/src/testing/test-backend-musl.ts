import { runCommand } from '../shared/command.ts'
import { WORKSPACE_ROOT } from '../shared/repo-paths.ts'

/** Build neutral-crate test binaries with musl, then run them in a router rootfs. */
export async function main(args: string[]): Promise<void> {
  const architecture = args[0] ?? 'arm64'
  if (!['arm64', 'amd64'].includes(architecture) || args.length > 2) {
    throw new Error(
      'Usage: pnpm run test:backend-musl [arm64|amd64] [router-rootfs-image]',
    )
  }
  const router =
    args[1] ??
    (architecture === 'arm64'
      ? 'immortalwrt/rootfs:armsr-armv8-openwrt-24.10.4'
      : 'immortalwrt/rootfs:x86-64-openwrt-24.10.4')
  const volume = `nyanpasu-musl-tests-${architecture}`
  const platform = `linux/${architecture}`
  // Some rootfs tags publish amd64 image metadata even when their files are ARM.
  // Check the rootfs loader instead of asking Docker to enforce that metadata.
  const loader = architecture === 'arm64' ? 'aarch64' : 'x86_64'
  const rootfs = await runCommand('docker', {
    args: [
      'run',
      '--rm',
      '--entrypoint',
      '/bin/sh',
      router,
      '-ec',
      `test -e /lib/ld-musl-${loader}.so.1`,
    ],
    stdout: 'inherit',
    stderr: 'inherit',
  })
  if (!rootfs.success) {
    throw new Error(`rootfs lacks the ${loader} musl loader`)
  }
  // Separate container-owned caches from the current checkout's macOS target.
  // The source mount is read-only; there are no host credentials or network bridges.
  const build = await runCommand('docker', {
    args: [
      'run',
      '--rm',
      '--platform',
      platform,
      '--mount',
      `type=bind,src=${WORKSPACE_ROOT},dst=/workspace,readonly`,
      '--mount',
      `type=volume,src=${volume},dst=/cache`,
      '--mount',
      `type=volume,src=${volume}-cargo,dst=/usr/local/cargo`,
      '--workdir',
      '/workspace',
      '--env',
      'CARGO_HOME=/usr/local/cargo',
      '--env',
      'RUSTUP_HOME=/cache/rustup',
      '--env',
      'CARGO_TARGET_DIR=/cache/target',
      'rust:alpine',
      'sh',
      '-ec',
      'apk add --no-cache build-base cmake perl pkgconf git openssl-dev >&2\n' +
        'rustup toolchain install nightly --profile minimal >&2\n' +
        'toolchain_cargo=$(rustup which --toolchain nightly cargo)\n' +
        'export RUSTC=$(rustup which --toolchain nightly rustc)\n' +
        // Resolve the client library separately: test/dev and RPC features must not mask omissions.
        '"$toolchain_cargo" build --locked --manifest-path backend/Cargo.toml ' +
        '-p nyanpasu-rpc-protocol --lib --no-default-features --features local-client --jobs 12 >&2\n' +
        '"$toolchain_cargo" build --locked --manifest-path backend/Cargo.toml -p nyanpasu-server --lib --bin nyanpasu-server --jobs 12 >&2\n' +
        // The independent production closure is checked by lint:backend-boundaries.
        '"$toolchain_cargo" test --locked --manifest-path backend/Cargo.toml ' +
        '-p nyanpasu-rpc --features nyanpasu-rpc/http --features nyanpasu-rpc/local-server -p nyanpasu-rpc-protocol --features nyanpasu-rpc-protocol/frame-io --features nyanpasu-rpc-protocol/local-client -p nyanpasu-contracts -p nyanpasu-core -p nyanpasu-state-engine -p nyanpasu-runtime-values -p nyanpasu-migration -p nyanpasu-profile-transform -p nyanpasu-scripts -p boa_utils ' +
        '-p nyanpasu-persistence -p nyanpasu-profile-io -p nyanpasu-net -p nyanpasu-owner-support -p nyanpasu-observers -p nyanpasu-updater -p nyanpasu-os -p nyanpasu-server --features nyanpasu-os/test-support --lib --tests ' +
        '--no-run --message-format=json --jobs 4',
    ],
    stdout: 'piped',
    stderr: 'inherit',
  })
  if (!build.success) throw new Error(`musl build failed (${build.code})`)
  const executables: string[] = []
  const expectedTargets = new Set([
    'lib:nyanpasu_rpc_protocol',
    'lib:nyanpasu_rpc',
    'lib:nyanpasu_server',
    'bin:nyanpasu-server',
    'test:panic_process',
    'lib:nyanpasu_contracts',
    'lib:nyanpasu_core',
    'lib:nyanpasu_state_engine',
    'lib:nyanpasu_runtime_values',
    'lib:nyanpasu_migration',
    'lib:nyanpasu_profile_transform',
    'lib:nyanpasu_scripts',
    'lib:boa_utils',
    'lib:nyanpasu_persistence',
    'lib:nyanpasu_profile_io',
    'lib:nyanpasu_net',
    'lib:nyanpasu_owner_support',
    'lib:nyanpasu_observers',
    'lib:nyanpasu_updater',
    'lib:nyanpasu_os',
  ])
  const expectedCount = expectedTargets.size
  for (const line of build.stdout.toString('utf8').split('\n')) {
    if (!line.startsWith('{')) continue
    const artifact: {
      reason?: string
      executable?: string
      profile?: { test?: boolean }
      target?: { name: string; kind: string[] }
    } = JSON.parse(line)
    if (
      artifact.reason === 'compiler-artifact' &&
      artifact.profile?.test &&
      artifact.executable
    ) {
      const target = artifact.target
      const identity = `${target?.kind.join(',')}:${target?.name}`
      if (!expectedTargets.delete(identity)) {
        throw new Error(`unexpected test target: ${identity}`)
      }
      executables.push(artifact.executable)
    }
  }
  if (expectedTargets.size > 0) {
    throw new Error(
      `expected ${expectedCount} neutral crate test binaries, got ${executables.length}`,
    )
  }
  for (const executable of executables) {
    const result = await runCommand('docker', {
      args: [
        'run',
        '--rm',
        '--mount',
        `type=volume,src=${volume},dst=/cache,readonly`,
        '--entrypoint',
        executable,
        router,
        '--test-threads=2',
      ],
      stdout: 'inherit',
      stderr: 'inherit',
    })
    if (!result.success) {
      throw new Error(`router tests failed (${result.code}): ${executable}`)
    }
  }
  console.log(`Neutral backend tests passed in ${router} (${platform}).`)
}

if (import.meta.main) {
  main(process.argv.slice(2)).catch((error) => {
    console.error(error)
    process.exit(1)
  })
}
