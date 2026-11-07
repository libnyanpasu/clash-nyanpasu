import * as path from 'node:path'
import { runCommand } from '../shared/command.ts'
import { WORKSPACE_ROOT } from '../shared/repo-paths.ts'
import { checkCoreModuleBoundaries } from './core-module-boundaries.ts'

type PackageMatcher = string | { prefix: string }

interface DependencyRule {
  package: string
  forbids: readonly PackageMatcher[]
  allowedWorkspace?: readonly string[]
}

const GUI: readonly PackageMatcher[] = [
  'clash-nyanpasu',
  'nyanpasu-egui',
  'nyanpasu-widget',
  'nyanpasu-crash-reporter',
  'slint',
  { prefix: 'slint-' },
  { prefix: 'i-slint-' },
  'winit',
  'eframe',
  'egui',
  'gtk',
  'webkit2gtk',
  'tauri',
  { prefix: 'tauri-' },
]
const FOUNDATION = [
  'nyanpasu-config',
  'nyanpasu-contracts',
  'nyanpasu-persistence',
  'nyanpasu-profile-transform',
  'nyanpasu-runtime-values',
]
const CORE_FAMILY = [
  'nyanpasu-core',
  'nyanpasu-state-engine',
  'nyanpasu-migration',
  'nyanpasu-owner-support',
  'nyanpasu-observers',
  'nyanpasu-updater',
]
const IMPLEMENTATION = [
  'nyanpasu-scripts',
  'nyanpasu-profile-io',
  'nyanpasu-net',
  'nyanpasu-os',
]

const DEPENDENCY_RULES: readonly DependencyRule[] = [
  { package: 'nyanpasu-rpc', forbids: [...GUI, ...IMPLEMENTATION] },
  {
    package: 'nyanpasu-cli',
    forbids: [
      ...GUI,
      ...CORE_FAMILY,
      ...IMPLEMENTATION,
      'nyanpasu-rpc',
      'nyanpasu-server',
    ],
  },
  ...[
    ...FOUNDATION,
    ...CORE_FAMILY,
    ...IMPLEMENTATION,
    'nyanpasu-rpc-protocol',
  ].map((name) => ({
    package: name,
    forbids: ['nyanpasu-rpc'],
  })),
  {
    package: 'nyanpasu-rpc-protocol',
    forbids: [...GUI, ...CORE_FAMILY, ...IMPLEMENTATION, 'nyanpasu-server'],
  },
  { package: 'boa_utils', forbids: [...CORE_FAMILY, ...IMPLEMENTATION] },
  ...[
    ...FOUNDATION,
    ...CORE_FAMILY,
    ...IMPLEMENTATION,
    'nyanpasu-diagnose',
    'boa_utils',
  ].map((name) => ({ package: name, forbids: GUI })),
  ...FOUNDATION.map((name) => ({
    package: name,
    forbids: [...CORE_FAMILY, ...IMPLEMENTATION, 'nyanpasu-rpc-protocol'],
  })),
  ...FOUNDATION.filter((name) => name !== 'nyanpasu-runtime-values').map(
    (name) => ({
      package: name,
      forbids: ['nyanpasu-runtime-values'],
    }),
  ),
  {
    package: 'nyanpasu-runtime-values',
    forbids: ['nyanpasu-core-manager'],
  },
  {
    package: 'nyanpasu-config',
    forbids: FOUNDATION.filter((name) => name !== 'nyanpasu-config'),
  },
  {
    package: 'nyanpasu-contracts',
    forbids: [
      'ractor',
      'reqwest',
      'redb',
      'nyanpasu-core-manager',
      'nyanpasu-local-channel',
      'nyanpasu-persistence',
      'nyanpasu-profile-transform',
    ],
  },
  {
    package: 'nyanpasu-persistence',
    forbids: [
      'redb',
      'nyanpasu-core-manager',
      'nyanpasu-contracts',
      'nyanpasu-profile-transform',
    ],
  },
  {
    package: 'nyanpasu-profile-transform',
    forbids: ['nyanpasu-contracts', 'nyanpasu-persistence'],
  },
  {
    package: 'nyanpasu-migration',
    forbids: ['redb', 'nyanpasu-core-manager'],
  },
  {
    package: 'nyanpasu-observers',
    forbids: [
      'redb',
      'clash-api',
      'nyanpasu-core-manager',
      'notify',
      'notify-debouncer-full',
    ],
  },
  {
    package: 'nyanpasu-owner-support',
    forbids: CORE_FAMILY.filter((name) => name !== 'nyanpasu-owner-support'),
  },
  {
    package: 'nyanpasu-state-engine',
    forbids: CORE_FAMILY.filter((name) => name !== 'nyanpasu-state-engine'),
    allowedWorkspace: ['nyanpasu-state-engine', 'nyanpasu-persistence'],
  },
  ...IMPLEMENTATION.map((name) => ({
    package: name,
    forbids: [
      ...CORE_FAMILY,
      ...IMPLEMENTATION.filter((other) => other !== name),
    ],
  })),
  ...CORE_FAMILY.map((name) => ({
    package: name,
    forbids: [...IMPLEMENTATION, 'sysproxy', 'runas'],
  })),
  {
    package: 'nyanpasu-os',
    forbids: ['nyanpasu-ipc', 'nyanpasu-core-manager'],
  },
  {
    package: 'nyanpasu-diagnose',
    forbids: [...CORE_FAMILY, ...IMPLEMENTATION],
  },
]

/** Inspect resolved production dependencies, including transitive GUI leaks. */
export function dependencyViolations(
  owner: string,
  cargoTree: string,
  rules: readonly DependencyRule[] = DEPENDENCY_RULES,
  workspaceMembers: readonly string[] = [],
): string[] {
  const ownerRules = rules.filter((rule) => rule.package === owner)
  const forbidden = ownerRules.flatMap((rule) => rule.forbids)
  const names = new Set(
    cargoTree
      .split('\n')
      .map((line) => line.trim().split(/\s+/)[0])
      .filter(Boolean),
  )

  return [...names]
    .filter(
      (name) =>
        name !== owner &&
        (forbidden.some((matcher) =>
          typeof matcher === 'string'
            ? name === matcher
            : name.startsWith(matcher.prefix),
        ) ||
          (workspaceMembers.includes(name) &&
            ownerRules.some(
              (rule) =>
                rule.allowedWorkspace && !rule.allowedWorkspace.includes(name),
            ))),
    )
    .map((name) => `${owner} must not depend on ${name}`)
}

export async function main(): Promise<number> {
  const metadataResult = await runCommand('cargo', {
    cwd: WORKSPACE_ROOT,
    args: [
      'metadata',
      '--locked',
      '--no-deps',
      '--format-version',
      '1',
      '--manifest-path',
      'backend/Cargo.toml',
    ],
  })
  if (!metadataResult.success) {
    console.error(metadataResult.stderr.toString('utf8'))
    return metadataResult.code || 1
  }
  const metadata: {
    packages: { id: string; name: string }[]
    workspace_members: string[]
  } = JSON.parse(metadataResult.stdout.toString('utf8'))
  const workspaceMembers = metadata.packages
    .filter((pkg) => metadata.workspace_members.includes(pkg.id))
    .map((pkg) => pkg.name)
  const problems = checkCoreModuleBoundaries(
    path.join(WORKSPACE_ROOT, 'backend/nyanpasu-core/src'),
  )
  const packages = new Set(DEPENDENCY_RULES.map((rule) => rule.package))
  for (const owner of packages) {
    // The CLI is composed in E4; its dependency policy already applies to fixtures.
    if (owner === 'nyanpasu-cli' && !workspaceMembers.includes(owner)) continue
    const result = await runCommand('cargo', {
      cwd: WORKSPACE_ROOT,
      args: [
        'tree',
        '--locked',
        '--all-features',
        '--manifest-path',
        'backend/Cargo.toml',
        '--package',
        owner,
        '--target',
        'all',
        '--edges',
        'normal,build',
        '--prefix',
        'none',
        '--format',
        '{p}',
      ],
    })
    if (!result.success) {
      console.error(result.stderr.toString('utf8'))
      return result.code || 1
    }
    problems.push(
      ...dependencyViolations(
        owner,
        result.stdout.toString('utf8'),
        DEPENDENCY_RULES,
        workspaceMembers,
      ),
    )
  }
  if (problems.length > 0) {
    console.error(problems.join('\n'))
    return 1
  }
  console.log('Backend package boundaries passed.')
  return 0
}

if (import.meta.main) {
  main()
    .then((code) => process.exit(code))
    .catch((error) => {
      console.error(error)
      process.exit(1)
    })
}
