export function normalizeVersion(version?: string): string | undefined {
  if (!version || version === "Not Found") return undefined;
  return version;
}

export function isValidVersion(version?: string): version is string {
  return Boolean(version && /^[A-Za-z0-9._+-]+$/.test(version));
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

export function extractVersionFromAssetList(
  page: string,
  assetTemplate: string,
): string | undefined {
  const [prefix, suffix] = assetTemplate.split("{}");
  if (prefix === undefined || suffix === undefined) return undefined;

  const matcher = new RegExp(
    `${escapeRegExp(prefix)}([A-Za-z0-9._+-]+)${escapeRegExp(suffix)}`,
  );
  const matched = page.match(matcher)?.[1];
  return isValidVersion(matched) ? matched : undefined;
}

export async function fetchText(
  url: string,
  options?: { headers?: HeadersInit },
): Promise<string | undefined> {
  const response = await fetch(url, {
    method: "GET",
    headers: {
      "User-Agent":
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:131.0) Gecko/20100101 Firefox/131.0",
      ...options?.headers,
    },
  });

  if (!response.ok) return undefined;
  return await response.text();
}
