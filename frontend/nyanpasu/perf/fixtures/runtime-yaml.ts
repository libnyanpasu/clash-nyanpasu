import type { SnapshotDiffHunk } from '@nyanpasu/interface'

// A runtime config as a subscription produces it: proxies with their
// options, groups listing every proxy, and a long rule list.
export function createRuntimeYaml(proxies: number, rules: number) {
  const lines = [
    'mixed-port: 7890',
    'mode: rule',
    'log-level: info',
    'proxies:',
  ]

  for (let i = 0; i < proxies; i++) {
    lines.push(
      `  - name: "🇯🇵 Node ${i}"`,
      '    type: ss',
      `    server: node-${i}.example.com`,
      `    port: ${10000 + i}`,
      '    cipher: aes-128-gcm',
      `    password: "pass-${i}"`,
      '    udp: true',
    )
  }

  lines.push('proxy-groups:')
  for (let g = 0; g < 10; g++) {
    lines.push(`  - name: Group ${g}`, '    type: select', '    proxies:')
    for (let i = 0; i < proxies; i += 10)
      lines.push(`      - "🇯🇵 Node ${i + g}"`)
  }

  lines.push('rules:')
  for (let i = 0; i < rules; i++) {
    lines.push(`  - DOMAIN-SUFFIX,site-${i}.example.com,Group ${i % 10}`)
  }
  lines.push('  - MATCH,Group 0')

  return lines.join('\n')
}

// A transform that rewrites every proxy's port: one hunk per proxy.
export function createRuntimeDiff(yaml: string): SnapshotDiffHunk[] {
  const lines = yaml.split('\n')
  const hunks: SnapshotDiffHunk[] = []

  lines.forEach((line, index) => {
    if (!line.startsWith('    port: ')) return
    const start = Math.max(0, index - 3)
    const context = (from: number, to: number) =>
      lines.slice(from, to).map((text) => ` ${text}`)
    hunks.push({
      old_start: start + 1,
      old_lines: 7,
      new_start: start + 1,
      new_lines: 7,
      lines: [
        ...context(start, index),
        `-${line}`,
        `+    port: ${Number(line.slice(10)) + 1}`,
        ...context(index + 1, index + 4),
      ],
    })
  })

  return hunks
}
