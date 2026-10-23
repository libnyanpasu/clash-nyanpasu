import {
  sftpQuote,
  validateSourceforgeProject,
  validateSourceforgeUsername,
} from "./sourceforge.ts";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runCommand } from "../shared/command.ts";

export function storageConfiguration(
  get: (name: string) => string | undefined,
) {
  const required = (name: string) => {
    const value = get(name)?.trim();
    if (!value) throw new Error(`${name} is required for storage publication`);
    return value;
  };
  const sourceforge = get("SOURCEFORGE_PROJECT")?.trim() || null;
  const archive = get("IA_ITEM_PREFIX")?.trim() || null;
  const telegram = get("TELEGRAM_ARCHIVE_CHANNEL")?.trim() || null;
  if (sourceforge) {
    validateSourceforgeProject(sourceforge);
    validateSourceforgeUsername(required("SOURCEFORGE_USERNAME"));
    required("SOURCEFORGE_SSH_KEY");
    required("SOURCEFORGE_KNOWN_HOSTS");
  }
  const token = get("ARCHIVE_UPLOAD_TOKEN")?.trim() ||
    get("FILE_SERVER_TOKEN")?.trim() || get("UPLOAD_TOKEN")?.trim();
  if (archive) {
    if (!/^[A-Za-z0-9_-]+$/.test(archive)) {
      throw new Error("Invalid IA_ITEM_PREFIX");
    }
    required("IA_ACCESS_KEY");
    required("IA_SECRET_KEY");
    required("IA_UPLOADER");
    if (!token) throw new Error("An archive upload token is required");
  }
  if (telegram) {
    const id = Number(required("TELEGRAM_API_ID"));
    if (!Number.isSafeInteger(id) || id <= 0) {
      throw new Error("Invalid TELEGRAM_API_ID");
    }
    required("TELEGRAM_API_HASH");
    required("TELEGRAM_TOKEN");
    if (!token) throw new Error("An archive upload token is required");
  }
  return { sourceforge, archive, telegram, token };
}

export async function probeArchiveApi(token: string, fetcher = fetch) {
  // An invalid manifest checks authentication without creating a build.
  const probes = [
    { path: "/archive/builds", method: "POST", status: 400, body: "{}" },
    // The impossible identity exercises the database without modifying it.
    { path: "/archive/builds/storage-preflight", method: "GET", status: 404 },
  ];
  for (const probe of probes) {
    const response = await fetcher(
      `https://archive.nyanpasu.org${probe.path}`,
      {
        method: probe.method,
        headers: {
          authorization: `Bearer ${token}`,
          "content-type": "application/json",
        },
        body: probe.body,
        redirect: "error",
        signal: AbortSignal.timeout(30_000),
      },
    );
    const detail = await response.text();
    if (
      response.status !== probe.status ||
      !response.headers.get("content-type")?.includes("application/json")
    ) {
      throw new Error(
        `Archive preflight ${probe.method} ${probe.path}: HTTP ${response.status}: ${
          detail.slice(0, 2000)
        }`,
      );
    }
    const result = JSON.parse(detail);
    if (
      result.error !==
        (probe.method === "POST" ? "Invalid build manifest" : "Build not found")
    ) {
      throw new Error(
        `Unexpected Archive API preflight response for ${probe.path}`,
      );
    }
  }
}

async function probeSourceforge(project: string) {
  const temp = await mkdtemp(join(tmpdir(), "storage-preflight-"));
  try {
    const key = `${temp}/key`;
    const hosts = `${temp}/known_hosts`;
    await writeFile(key, process.env["SOURCEFORGE_SSH_KEY"]!, {
      mode: 0o600,
    });
    await writeFile(hosts, process.env["SOURCEFORGE_KNOWN_HOSTS"]!);
    const result = await runCommand("sftp", {
      args: [
        "-i",
        key,
        "-o",
        "BatchMode=yes",
        "-o",
        "IdentitiesOnly=yes",
        "-o",
        `UserKnownHostsFile=${hosts}`,
        "-o",
        "StrictHostKeyChecking=yes",
        "-o",
        "ConnectTimeout=15",
        "-o",
        "ServerAliveInterval=15",
        "-o",
        "ServerAliveCountMax=2",
        "-b",
        "-",
        `${process.env["SOURCEFORGE_USERNAME"]!.trim()}@frs.sourceforge.net`,
      ],
      input: `ls ${sftpQuote(`/home/frs/project/${project}`)}\n`,
    });
    if (!result.success) {
      throw new Error(
        `SourceForge preflight: ${result.stderr.toString("utf8")}`,
      );
    }
  } finally {
    await rm(temp, { recursive: true });
  }
}

if (import.meta.main) {
  const configuration = storageConfiguration((name) => process.env[name]);
  if (configuration.archive || configuration.telegram) {
    await probeArchiveApi(configuration.token!);
  }
  if (configuration.sourceforge) {
    await probeSourceforge(configuration.sourceforge);
  }
  console.log("Storage configuration and read-only connectivity checks passed");
}
