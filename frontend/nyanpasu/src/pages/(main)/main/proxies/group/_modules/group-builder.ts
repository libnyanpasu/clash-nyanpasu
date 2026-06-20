// Helpers for generating a profile-scoped "merge" chain that appends a new
// proxy-group. The generated YAML relies on the backend merge engine's
// `append__proxy-groups` primitive (see backend/tauri/src/enhance/merge.rs).

export const PROXY_GROUP_TYPES = [
  'select',
  'url-test',
  'fallback',
  'load-balance',
] as const

export type ProxyGroupType = (typeof PROXY_GROUP_TYPES)[number]

export const LOAD_BALANCE_STRATEGIES = [
  'consistent-hashing',
  'round-robin',
  'sticky-sessions',
] as const

export type LoadBalanceStrategy = (typeof LOAD_BALANCE_STRATEGIES)[number]

/** Group types that perform health checks and therefore accept url/interval. */
export const HEALTH_CHECK_GROUP_TYPES: ProxyGroupType[] = [
  'url-test',
  'fallback',
  'load-balance',
]

export const DEFAULT_HEALTH_CHECK_URL = 'http://www.gstatic.com/generate_204'

export const DEFAULT_HEALTH_CHECK_INTERVAL = 300

export const RULE_TYPES = [
  'DOMAIN-SUFFIX',
  'DOMAIN',
  'DOMAIN-KEYWORD',
  'IP-CIDR',
  'IP-CIDR6',
  'PROCESS-NAME',
  'GEOIP',
] as const

export type RuleType = (typeof RULE_TYPES)[number]

/** Rule types matching on IP need `no-resolve` to avoid a DNS lookup. */
const NO_RESOLVE_RULE_TYPES: RuleType[] = ['IP-CIDR', 'IP-CIDR6', 'GEOIP']

export interface GroupRule {
  type: RuleType
  value: string
}

export interface NewGroupOptions {
  name: string
  type: ProxyGroupType
  proxies: string[]
  url?: string
  interval?: number
  strategy?: LoadBalanceStrategy
  /** Existing group names that should reference (include) the new group. */
  injectInto?: string[]
  /** Rules routed to this new group (target defaults to the group name). */
  rules?: GroupRule[]
}

/**
 * Build a single clash rule line `TYPE,VALUE,TARGET`, appending `no-resolve`
 * for IP-based rules.
 */
export const buildRuleLine = (
  type: RuleType,
  value: string,
  target: string,
): string => {
  const base = `${type},${value},${target}`

  return NO_RESOLVE_RULE_TYPES.includes(type) ? `${base},no-resolve` : base
}

/** Always double-quote scalar strings so arbitrary node names stay valid YAML. */
const quote = (value: string): string =>
  `"${value.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`

/** Escape a value for embedding inside a single-quoted Lua string literal. */
const luaQuote = (value: string): string =>
  `'${value
    .replace(/\\/g, '\\\\')
    .replace(/'/g, "\\'")
    .replace(/\n/g, '\\n')
    .replace(/\r/g, '\\r')}'`

/**
 * Build a merge-profile YAML that appends a single proxy-group built from the
 * selected nodes. The output is intentionally human-readable so it can still be
 * tweaked in the profile editor afterwards.
 */
export const buildGroupMergeYaml = (options: NewGroupOptions): string => {
  const lines: string[] = [
    '# Clash Nyanpasu - GUI generated proxy group',
    '# Scoped to the current profile via its chain.',
    '# Documentation on https://nyanpasu.org/',
    'append__proxy-groups:',
    `  - name: ${quote(options.name)}`,
    `    type: ${options.type}`,
  ]

  if (options.type === 'load-balance' && options.strategy) {
    lines.push(`    strategy: ${options.strategy}`)
  }

  if (HEALTH_CHECK_GROUP_TYPES.includes(options.type)) {
    if (options.url) {
      lines.push(`    url: ${quote(options.url)}`)
    }

    if (options.interval != null) {
      lines.push(`    interval: ${options.interval}`)
    }
  }

  lines.push('    proxies:')

  for (const proxy of options.proxies) {
    lines.push(`      - ${quote(proxy)}`)
  }

  // Inject the new group into existing groups by name. The merge engine runs
  // the Lua `expr` against each proxy-group, so we prepend the new group to the
  // target's proxies list (prepend keeps it visible at the top of the picker).
  const injectInto = options.injectInto?.filter(
    (target) => target !== options.name,
  )

  if (injectInto && injectInto.length > 0) {
    lines.push('filter__proxy-groups:')

    for (const target of injectInto) {
      lines.push(`  - when: |`)
      lines.push(`      item.name == ${luaQuote(target)}`)
      lines.push(`    expr: |`)
      lines.push(`      if item.proxies == nil then item.proxies = {} end`)
      lines.push(
        `      table.insert(item.proxies, 1, ${luaQuote(options.name)})`,
      )
      lines.push(`      return item`)
    }
  }

  // Rules routed to the new group. `prepend__rules` keeps them ahead of the
  // catch-all MATCH rule so they actually take effect.
  if (options.rules && options.rules.length > 0) {
    lines.push(`${RULES_MERGE_KEY}:`)

    for (const rule of options.rules) {
      lines.push(
        `  - ${quote(buildRuleLine(rule.type, rule.value, options.name))}`,
      )
    }
  }

  return `${lines.join('\n')}\n`
}

/** Top-level merge key holding profile-scoped rules. */
export const RULES_MERGE_KEY = 'prepend__rules'

/**
 * Insert a rule line as the first entry of the `prepend__rules` block in an
 * existing generated merge file, creating the block if absent. The file format
 * is the one produced by {@link buildGroupMergeYaml}, so a targeted text edit
 * is safe and avoids a YAML round-trip.
 */
export const insertRuleIntoMergeYaml = (
  yamlText: string,
  ruleLine: string,
): string => {
  const entry = `  - ${quote(ruleLine)}`
  const lines = yamlText.replace(/\r\n/g, '\n').split('\n')
  const keyIndex = lines.findIndex(
    (line) => line.trimEnd() === `${RULES_MERGE_KEY}:`,
  )

  if (keyIndex === -1) {
    const trimmed = yamlText.replace(/\s+$/, '')
    return `${trimmed}\n${RULES_MERGE_KEY}:\n${entry}\n`
  }

  lines.splice(keyIndex + 1, 0, entry)
  return lines.join('\n')
}

/**
 * Extract the proxy-group name a generated merge file defines, used to map a
 * GUI-created group back to its merge profile. Returns null for files that
 * don't follow the {@link buildGroupMergeYaml} layout.
 */
export const extractGroupNameFromMergeYaml = (
  yamlText: string,
): string | null => {
  const lines = yamlText.replace(/\r\n/g, '\n').split('\n')
  const startIndex = lines.findIndex(
    (line) => line.trimEnd() === 'append__proxy-groups:',
  )

  if (startIndex === -1) {
    return null
  }

  for (let i = startIndex + 1; i < lines.length; i++) {
    const line = lines[i]
    const match = line.match(/^\s*-?\s*name:\s*"((?:[^"\\]|\\.)*)"\s*$/)

    if (match) {
      return match[1].replace(/\\"/g, '"').replace(/\\\\/g, '\\')
    }

    // Stop once a new top-level key starts (the group block has ended).
    if (line.trim() !== '' && /^\S/.test(line)) {
      break
    }
  }

  return null
}

// nanoid alphabet mirrors the backend (`get_uid`) so generated uids look native.
const UID_ALPHABET =
  '0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ'

/**
 * Generate a merge-profile uid client-side. The backend honours a uid supplied
 * in the builder (see ProfileSharedBuilder::build), letting us reference the
 * freshly created profile without an extra round-trip.
 */
export const generateMergeUid = (): string => {
  const bytes = crypto.getRandomValues(new Uint8Array(11))
  let id = ''

  for (const byte of bytes) {
    id += UID_ALPHABET[byte % UID_ALPHABET.length]
  }

  return `m${id}`
}
