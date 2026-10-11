import { isAlias, isMap, isSeq, parseDocument } from 'yaml'

/** Counts only static entries in the source file, before transforms or providers. */
export function profileContentSummary(content: string) {
  const document = parseDocument(content)
  if (document.errors.length > 0 || !isMap(document.contents)) return null

  const counts: number[] = []
  for (const key of ['proxies', 'proxy-groups', 'rules']) {
    const value = document.get(key, true)
    const sequence = isAlias(value) ? value.resolve(document) : value
    if (sequence == null) counts.push(0)
    else if (isSeq(sequence)) counts.push(sequence.items.length)
    else return null
  }

  return { nodes: counts[0], groups: counts[1], rules: counts[2] }
}
