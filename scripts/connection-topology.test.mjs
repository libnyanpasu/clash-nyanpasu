import assert from 'node:assert/strict'
import test from 'node:test'
import { buildTopology } from '../frontend/nyanpasu/src/pages/(main)/main/topology/_modules/topology.ts'

const connection = (id, overrides = {}) => ({
  id, metadata: { process: 'Browser', sourceIP: '127.0.0.1' },
  upload: 20, download: 80, chains: ['Exit', 'Nested', 'Select'],
  rule: 'Domain', rulePayload: 'example.com', ...overrides,
})

test('preserves logical chain order and isolates identical names across layers', () => {
  const graph = buildTopology([connection('1', {
    metadata: { process: 'Exit' }, chains: ['Exit', 'Nested', 'Select'],
  })], 'connections')
  assert.equal(graph.layers[2][0].label, 'Select → Nested')
  assert.notEqual(graph.layers[0][0].id, graph.layers[3][0].id)
  assert.equal(graph.links.length, 3)
  assert.equal(new Set(graph.layers.flat().map(node => node.id)).size, 4)
})

test('other buckets preserve every connection and byte at every layer', () => {
  const connections = Array.from({ length: 80 }, (_, i) => connection(String(i), {
    metadata: { process: `app-${i}` }, chains: [`exit-${i}`, `group-${i}`],
    rulePayload: `domain-${i}`, upload: i, download: i * 2,
  }))
  for (const metric of ['bytes', 'connections']) {
    const graph = buildTopology(connections, metric)
    const expectedBytes = connections.reduce((sum, c) => sum + c.upload + c.download, 0)
    for (const layer of graph.layers) {
      assert.equal(layer.length, 7)
      assert.equal(layer.reduce((sum, n) => sum + n.count, 0), 80)
      assert.equal(layer.reduce((sum, n) => sum + n.bytes, 0), expectedBytes)
      assert.equal(new Set(layer.flatMap(n => [...n.connectionIds])).size, 80)
    }
    for (let i = 0; i < 3; i++) {
      const ids = new Set(graph.layers[i].map(n => n.id))
      const links = graph.links.filter(link => ids.has(link.source))
      assert.equal(links.reduce((sum, link) => sum + link.bytes, 0), expectedBytes)
      assert.equal(links.reduce((sum, link) => sum + link.count, 0), 80)
    }
  }
})

test('traffic ranking differs from count ranking without changing totals', () => {
  const connections = [connection('a'), connection('b'), connection('c', {
    metadata: { process: 'Download' }, download: 10000,
  })]
  assert.equal(buildTopology(connections, 'connections').layers[0][0].label, 'Browser')
  assert.equal(buildTopology(connections, 'bytes').layers[0][0].label, 'Download')
})

test('missing metadata, direct routes, and zero traffic remain representable', () => {
  const graph = buildTopology([connection('a', {
    metadata: null, chains: ['DIRECT'], rule: '', upload: -5, download: 0,
  })], 'bytes')
  assert.equal(graph.layers[0][0].label, null)
  assert.equal(graph.layers[2][0].label, null)
  assert.equal(graph.layers[3][0].label, 'DIRECT')
  assert.equal(graph.links[0].bytes, 0)
  assert.equal(graph.links[0].count, 1)
  assert.deepEqual(buildTopology([], 'bytes'), { layers: [[], [], [], []], links: [] })
})

test('a selected aggregate retains exact membership and input order does not affect ranking', () => {
  const connections = [connection('a'), connection('b', { metadata: { process: 'Other' } })]
  const graph = buildTopology(connections, 'connections')
  assert.deepEqual([...graph.layers[0].find(n => n.label === 'Browser').connectionIds], ['a'])
  assert.deepEqual(graph.layers.map(layer => layer.map(n => n.id)),
    buildTopology([...connections].reverse(), 'connections').layers.map(layer => layer.map(n => n.id)))
})

// Geography consumes only core metadata, never proxy names or host guesses.
const { buildGeography, connectionRegion } = await import('../frontend/nyanpasu/src/pages/(main)/main/topology/_modules/geography.ts')
const knownRegions = new Set(['US', 'JP', 'SG', 'HK'])

test('geography conserves totals and leaves missing or ambiguous regions unknown', () => {
  const connections = [
    connection('a', { metadata: { destinationGeoIP: ['us'] } }),
    connection('b', { metadata: { destinationGeoIP: ['US', 'US'] } }),
    connection('c', { metadata: { destinationGeoIP: ['US', 'JP'] } }),
    connection('d', { metadata: { destinationGeoIP: ['LAN'] } }),
    connection('e', { metadata: null }),
  ]
  const result = buildGeography(connections, knownRegions)
  assert.deepEqual(result.regions.find(region => region.code === 'US'), { code: 'US', count: 2, bytes: 200 })
  assert.deepEqual(result.regions.find(region => region.code === 'unknown'), { code: 'unknown', count: 3, bytes: 300 })
  assert.equal(result.regions.reduce((total, region) => total + region.bytes, 0), 500)
  assert.equal(result.routes.length, 0)
})

test('geographic routes require both known endpoints and never guess a proxy exit', () => {
  const connections = [
    connection('a', { metadata: { sourceGeoIP: ['HK'], destinationGeoIP: ['JP'] } }),
    connection('b', { metadata: { sourceGeoIP: ['HK'], destinationGeoIP: ['JP'] } }),
    connection('c', { metadata: { sourceIP: '127.0.0.1', destinationGeoIP: ['SG'] }, chains: ['🇺🇸 US server'] }),
    connection('d', { metadata: { sourceGeoIP: ['JP'], destinationGeoIP: ['JP'] } }),
  ]
  const result = buildGeography(connections, knownRegions)
  assert.deepEqual(result.routes, [{ code: 'HK:JP', source: 'HK', destination: 'JP', count: 2, bytes: 200 }])
  assert.equal(connectionRegion(connections[2], 'source', knownRegions), 'unknown')
  assert.equal(connectionRegion(connection('missing'), 'destination', knownRegions), 'unknown')
  assert.deepEqual(buildGeography([], knownRegions), { regions: [], routes: [] })
})

test('bundled map provides finite positions including common small regions', async () => {
  const { readFile } = await import('node:fs/promises')
  const map = JSON.parse(await readFile(new URL('../frontend/nyanpasu/src/assets/maps/world-map.json', import.meta.url)))
  for (const code of ['US', 'JP', 'SG', 'HK', 'FR']) assert.ok(map.centers[code])
  for (const [x, y] of Object.values(map.centers)) {
    assert.ok(Number.isFinite(x) && x >= 0 && x <= 960)
    assert.ok(Number.isFinite(y) && y >= 0 && y <= 460)
  }
  assert.ok(map.outline.startsWith('M'))
})
