/** Stable snapshot serialization and exact residual gate comparison. */
import {
  type GateIssue,
  type GateResult,
  type LedgerReport,
  type MetricBuckets,
  type ReportMetric,
  type StableMetric,
  type StableSnapshot,
} from "./policy.ts";

export function sortedRecord(map: Map<string, number>): Record<string, number> {
  return Object.fromEntries(
    [...map.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])),
  );
}

/** Stable object-key order for exact snapshot JSON / compare. */
export function sortedObjectKeys(
  record: Record<string, number>,
): Record<string, number> {
  return Object.fromEntries(
    Object.entries(record).sort((a, b) =>
      b[1] - a[1] || a[0].localeCompare(b[0])
    ),
  );
}

export function metricsFromBuckets(
  buckets: MetricBuckets,
): Record<string, ReportMetric> {
  return {
    config_calls: {
      total: buckets.configCalls.total,
      byKey: sortedRecord(buckets.configCalls.byKey),
      samples: buckets.configCalls.hits,
    },
    service_globals: {
      total: buckets.serviceGlobals.total,
      byKey: sortedRecord(buckets.serviceGlobals.byKey),
      samples: buckets.serviceGlobals.hits,
    },
    migration_markers: {
      total: buckets.migrationMarkers.total,
      byKey: sortedRecord(buckets.migrationMarkers.byKey),
      samples: buckets.migrationMarkers.hits,
    },
    legacy_dto_refs: {
      total: buckets.legacyDtos.total,
      byKey: sortedRecord(buckets.legacyDtos.byKey),
      samples: buckets.legacyDtos.hits,
    },
    test_real_dirs: {
      total: buckets.testRealDirs.total,
      byKey: sortedRecord(buckets.testRealDirs.byKey),
      samples: buckets.testRealDirs.hits,
    },
    mutable_statics: {
      total: buckets.mutableStatics.total,
      byKey: sortedRecord(buckets.mutableStatics.byKey),
      samples: buckets.mutableStatics.hits,
    },
  };
}

export function toStableSnapshot(
  roots: string[],
  metrics: Record<string, ReportMetric | StableMetric>,
  bridgeFiles: string[],
): StableSnapshot {
  const stableMetrics: Record<string, StableMetric> = {};
  for (const [id, metric] of Object.entries(metrics)) {
    stableMetrics[id] = {
      total: metric.total,
      byKey: sortedObjectKeys({ ...metric.byKey }),
    };
  }
  return {
    roots: [...roots],
    metrics: stableMetrics,
    bridgeFiles: [...bridgeFiles].sort(),
  };
}

export function reportToStableSnapshot(report: LedgerReport): StableSnapshot {
  return toStableSnapshot(report.roots, report.metrics, report.bridgeFiles);
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * Validate and normalize a JSON-decoded snapshot into the stable contract.
 * Throws with an actionable message on structural invalidity.
 */
export function parseStableSnapshot(raw: unknown): StableSnapshot {
  if (!isPlainObject(raw)) {
    throw new Error("snapshot must be a JSON object");
  }
  if (
    !Array.isArray(raw.roots) || !raw.roots.every((r) => typeof r === "string")
  ) {
    throw new Error("snapshot.roots must be string[]");
  }
  if (
    !Array.isArray(raw.bridgeFiles) ||
    !raw.bridgeFiles.every((r) => typeof r === "string")
  ) {
    throw new Error("snapshot.bridgeFiles must be string[]");
  }
  if (!isPlainObject(raw.metrics)) {
    throw new Error("snapshot.metrics must be an object");
  }

  const metrics: Record<string, StableMetric> = {};
  for (const [id, metric] of Object.entries(raw.metrics)) {
    if (!isPlainObject(metric)) {
      throw new Error(`snapshot.metrics.${id} must be an object`);
    }
    if (typeof metric.total !== "number" || !Number.isFinite(metric.total)) {
      throw new Error(`snapshot.metrics.${id}.total must be a finite number`);
    }
    if (!isPlainObject(metric.byKey)) {
      throw new Error(`snapshot.metrics.${id}.byKey must be an object`);
    }
    const byKey: Record<string, number> = {};
    for (const [key, count] of Object.entries(metric.byKey)) {
      if (typeof count !== "number" || !Number.isFinite(count)) {
        throw new Error(
          `snapshot.metrics.${id}.byKey[${
            JSON.stringify(key)
          }] must be a finite number`,
        );
      }
      byKey[key] = count;
    }
    metrics[id] = {
      total: metric.total,
      byKey: sortedObjectKeys(byKey),
    };
  }

  return {
    roots: raw.roots.map(String),
    metrics,
    bridgeFiles: [...raw.bridgeFiles.map(String)].sort(),
  };
}

function formatListDiff(
  label: string,
  expected: string[],
  actual: string[],
): string[] {
  const messages: string[] = [];
  const expSet = new Set(expected);
  const actSet = new Set(actual);
  for (const item of expected) {
    if (!actSet.has(item)) {
      messages.push(`${label}: missing ${JSON.stringify(item)}`);
    }
  }
  for (const item of actual) {
    if (!expSet.has(item)) {
      messages.push(`${label}: unexpected ${JSON.stringify(item)}`);
    }
  }
  if (
    messages.length === 0 &&
    (expected.length !== actual.length ||
      expected.some((v, i) => v !== actual[i]))
  ) {
    messages.push(
      `${label}: order/content mismatch expected=${
        JSON.stringify(expected)
      } actual=${JSON.stringify(actual)}`,
    );
  }
  return messages;
}

/**
 * Exact stable-snapshot compare + hard denylist on test_real_dirs.
 * Intentional residual shrink/growth requires an audited snapshot update.
 * `staticAllowlist` lists the static allowlist entries that do not name
 * exactly one static (see `staticAllowlistIssues`).
 */
export function evaluateGate(
  current: StableSnapshot,
  expected: StableSnapshot,
  staticAllowlist: string[] = [],
): GateResult {
  const issues: GateIssue[] = [];

  const denylistTotal = current.metrics.test_real_dirs?.total ?? 0;
  if (denylistTotal !== 0) {
    issues.push({
      kind: "hard_denylist",
      message:
        `hard denylist: test_real_dirs.total must be 0, got ${denylistTotal}`,
    });
  }

  for (const issue of staticAllowlist) {
    issues.push({
      kind: "static_allowlist",
      message: `static allowlist: ${issue}`,
    });
  }

  for (
    const msg of formatListDiff("roots", expected.roots, current.roots)
  ) {
    issues.push({ kind: "roots", message: msg });
  }

  for (
    const msg of formatListDiff(
      "bridgeFiles",
      expected.bridgeFiles,
      current.bridgeFiles,
    )
  ) {
    issues.push({ kind: "bridge_files", message: msg });
  }

  const expectedIds = new Set(Object.keys(expected.metrics));
  const currentIds = new Set(Object.keys(current.metrics));

  for (const id of expectedIds) {
    if (!currentIds.has(id)) {
      issues.push({
        kind: "metric_key",
        message: `metrics: missing metric id ${JSON.stringify(id)}`,
      });
    }
  }
  for (const id of currentIds) {
    if (!expectedIds.has(id)) {
      issues.push({
        kind: "metric_key",
        message: `metrics: unexpected metric id ${JSON.stringify(id)}`,
      });
    }
  }

  for (const id of expectedIds) {
    if (!currentIds.has(id)) continue;
    const exp = expected.metrics[id];
    const act = current.metrics[id];
    if (exp.total !== act.total) {
      issues.push({
        kind: "metric_total",
        message:
          `metrics.${id}.total: expected ${exp.total}, actual ${act.total}`,
      });
    }

    const expKeys = new Set(Object.keys(exp.byKey));
    const actKeys = new Set(Object.keys(act.byKey));
    for (const key of expKeys) {
      if (!actKeys.has(key)) {
        issues.push({
          kind: "metric_key",
          message: `metrics.${id}.byKey: missing key ${
            JSON.stringify(key)
          } (expected ${exp.byKey[key]})`,
        });
        continue;
      }
      if (exp.byKey[key] !== act.byKey[key]) {
        issues.push({
          kind: "metric_key",
          message: `metrics.${id}.byKey[${JSON.stringify(key)}]: expected ${
            exp.byKey[key]
          }, actual ${act.byKey[key]}`,
        });
      }
    }
    for (const key of actKeys) {
      if (!expKeys.has(key)) {
        issues.push({
          kind: "metric_key",
          message: `metrics.${id}.byKey: unexpected key ${
            JSON.stringify(key)
          } (actual ${act.byKey[key]})`,
        });
      }
    }
  }

  return { ok: issues.length === 0, issues };
}
