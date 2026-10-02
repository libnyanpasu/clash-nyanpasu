import assert from "node:assert/strict";
import { chromium } from "npm:playwright@1.63.0";

const entry = Deno.args[0];
assert.ok(
  entry,
  "Usage: deno task test:http-ui <running debug HTTP server URL>",
);
const url = new URL(entry).origin;
assert.ok(url, "Pass the running debug HTTP server URL");
const browser = await chromium.launch({ headless: true });
try {
  const context = await browser.newContext();
  const page = await context.newPage();
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  for (
    const path of [
      "/bridge/rpc",
      "/bridge/events",
      "/bridge/connection-details",
    ]
  ) {
    const response = await fetch(`${url}${path}`);
    assert.equal(response.status, 401);
    await response.body?.cancel();
  }
  await page.goto(entry, { waitUntil: "domcontentloaded" });
  await page.waitForURL(`${url}/main/dashboard`, { timeout: 30000 });
  assert.ok(!page.url().includes("access_token"));
  await page.goto(`${url}/main/settings/debug`, {
    waitUntil: "domcontentloaded",
  });
  const advanced = page.locator('[data-slot="allow-lan-switch-container"]')
    .filter({ hasText: "Advance Tools" }).getByRole("switch");
  await advanced.waitFor({ timeout: 30000 });
  if (!(await advanced.isChecked())) await advanced.click();
  const toggle = page.getByRole("switch", { name: "Axum HTTP server" });
  await toggle.waitFor({ timeout: 30000 });
  await toggle.and(page.locator('[aria-checked="true"]')).waitFor();
  assert.equal(await toggle.isDisabled(), true);
  assert.equal(await toggle.isChecked(), true);
  assert.equal(
    await page
      .getByRole("link", { name: entry, exact: true })
      .getAttribute("href"),
    entry,
  );
  const result = await page.evaluate(async () => {
    const call = async (
      method: string,
      params: Record<string, unknown> = {},
    ) => {
      const response = await fetch("/bridge/rpc", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ method, params }),
      });
      return { status: response.status, data: await response.json() };
    };
    await call("set_storage_item", { key: "http-ui-test", value: "browser" });
    const storage = await call("get_storage_item", { key: "http-ui-test" });
    const files = await call("list_log_files", { source: "app" });
    const error = await call("list_log_files", { source: "service" });
    const domainError = await call("read_profile_file", { uid: "missing" });
    const unsupported = await call("url_delay_test", {
      url: "http://127.0.0.1/",
    });
    await call("remove_storage_item", { key: "http-ui-test" });
    return { storage, files, error, domainError, unsupported };
  });
  assert.equal(result.unsupported.status, 501);
  assert.equal(result.unsupported.data.kind, "unsupported");
  assert.equal(result.storage.status, 200);
  assert.equal(result.storage.data, "browser");
  assert.equal(result.files.status, 200);
  assert.ok(Array.isArray(result.files.data));
  assert.equal(result.error.data.domain_error, "unsupported");
  assert.equal(result.domainError.data.domain_error.kind.domain, "profiles");
  assert.equal(typeof result.domainError.data.domain_error.detail, "string");
  if (Deno.env.get("NYANPASU_HTTP_CORE_LOG_FIXTURE") === "1") {
    const call = async (
      method: string,
      params: Record<string, unknown> = {},
    ) => {
      const response = await page.evaluate(async ({ method, params }) => {
        const response = await fetch("/bridge/rpc", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ method, params }),
        });
        return { status: response.status, data: await response.json() };
      }, { method, params });
      assert.equal(response.status, 200);
      return response.data;
    };
    const source = "core_local";
    const files = await call("list_log_files", { source });
    assert.equal(files.length, 1);
    const session = await call("open_log_session", {
      source,
      request: { request_id: crypto.randomUUID(), file: null },
    });
    const request = {
      session: session.id,
      direction: "latest",
      cursor: null,
      filter: {
        levels: ["debug"],
        text: "core-http-fixture",
        target: null,
        from_ms: null,
        to_ms: null,
      },
      limit: 200,
    };
    let preview = await call("query_logs", { source, request });
    const deadline = Date.now() + 10000;
    while (preview.building) {
      assert.ok(Date.now() < deadline, "Core archive indexing must finish");
      preview = await call("query_logs", { source, request });
    }
    assert.equal(preview.rows.length, 200);
    const last = preview.rows.at(-1);
    assert.equal(last.truncated, false);
    const frame = JSON.parse(last.raw);
    assert.equal(frame.t, "log");
    assert.equal(frame.stream, "stderr");
    assert.ok(frame.message.endsWith("complete-tail"));
    await page.goto(`${url}/main/logs`, { waitUntil: "domcontentloaded" });
    const viewer = page.locator('[data-slot="core-logs-viewer"]');
    await viewer.getByText(/core-http-fixture/).first().waitFor();
    await viewer.getByRole("button", { name: "View JSON", exact: true }).last()
      .click();
    await page.getByRole("region").filter({ hasText: "complete-tail" })
      .waitFor();
    const other = await context.newPage();
    await other.goto(`${url}/main/logs`, { waitUntil: "domcontentloaded" });
    await other.locator('[data-slot="core-logs-viewer"]').getByText(
      /core-http-fixture/,
    ).first().waitFor();
    await page.getByRole("button", {
      name: "Clear current display",
      exact: true,
    }).click();
    await page.getByText("No logs recorded", { exact: true }).waitFor();
    assert.ok(
      await other.locator('[data-slot="core-logs-viewer"]').getByText(
        /core-http-fixture/,
      ).count() > 0,
      "Clearing one viewer must preserve the other viewer",
    );
    const retained = await call("query_logs", { source, request });
    assert.equal(retained.rows.length, 200);
    assert.equal(retained.rows.at(-1).id, last.id);
    await call("close_log_session", { source, session: session.id });
    await other.close();
  }
  assert.deepEqual(
    errors,
    [],
    "Browser page must not call unavailable Tauri APIs",
  );
  console.log("Browser debug page and real HTTP RPC passed");
} finally {
  await browser.close();
}
