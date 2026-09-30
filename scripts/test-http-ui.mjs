import assert from 'node:assert/strict'
import { chromium } from 'playwright'

const url = process.argv[2]
assert.ok(url, 'Pass the running debug HTTP server URL')
const browser = await chromium.launch({ headless: true })
try {
  const page = await browser.newPage()
  const errors = []
  page.on('pageerror', (error) => errors.push(error.message))
  await page.goto(url, { waitUntil: 'domcontentloaded' })
  await page.waitForURL(`${url}/main/dashboard`, { timeout: 30000 })
  await page.goto(`${url}/main/settings/debug`, {
    waitUntil: 'domcontentloaded',
  })
  const advanced = page.locator('[data-slot="allow-lan-switch-container"]').filter({ hasText: 'Advance Tools' }).getByRole('switch')
  await advanced.waitFor({ timeout: 30000 })
  if (!(await advanced.isChecked())) await advanced.click()
  const toggle = page.getByRole('switch', { name: 'Axum HTTP server' })
  await toggle.waitFor({ timeout: 30000 })
  assert.equal(await toggle.isDisabled(), true)
  assert.equal(await toggle.isChecked(), true)
  assert.equal(
    await page
      .getByRole('link', { name: url, exact: true })
      .getAttribute('href'),
    url,
  )
  const result = await page.evaluate(async () => {
    const call = async (method, params = {}) => {
      const response = await fetch('/bridge/rpc', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ method, params }),
      })
      return { status: response.status, data: await response.json() }
    }
    await call('set_storage_item', { key: 'http-ui-test', value: 'browser' })
    const storage = await call('get_storage_item', { key: 'http-ui-test' })
    const files = await call('list_log_files', { source: 'app' })
    const error = await call('list_log_files', { source: 'service' })
    const domainError = await call('read_profile_file', { uid: 'missing' })
    await call('remove_storage_item', { key: 'http-ui-test' })
    return { storage, files, error, domainError }
  })
  assert.equal(result.storage.status, 200)
  assert.equal(result.storage.data, 'browser')
  assert.equal(result.files.status, 200)
  assert.ok(Array.isArray(result.files.data))
  assert.equal(result.error.data.domain_error, 'unsupported')
  assert.equal(result.domainError.data.domain_error.kind.domain, 'profiles')
  assert.equal(typeof result.domainError.data.domain_error.detail, 'string')
  assert.deepEqual(
    errors,
    [],
    'Browser page must not call unavailable Tauri APIs',
  )
  console.log('Browser debug page and real HTTP RPC passed')
} finally {
  await browser.close()
}
