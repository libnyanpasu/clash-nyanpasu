import * as path from "jsr:@std/path";
import { CENTRAL_PUBLICATION_TARGETS } from "./central-publication.ts";
import { hashFile } from "./sourceforge.ts";

export interface TelegramArtifact {
  fileName: string;
  fileSize: number;
  sha256: string;
  md5: string;
  path: string;
}

export interface TelegramReceipt {
  messageId: number;
  documentId: string;
  messageUrl: string;
}

export interface TelegramTarget {
  buildId: string;
  folderPath: string;
  channel: "nightly" | "release";
  commit: string;
  tag: string | null;
  target: string;
  publishedAt: string;
  artifacts: TelegramArtifact[];
}

export interface TelegramPublicationRecord extends TelegramArtifact {
  buildId: string;
  target: string;
  status: "uploaded" | "failed";
  receipt?: TelegramReceipt;
  error?: string;
}

/** Validate the complete inventory before posting any file to the channel. */
export async function readTelegramInventory(
  root: string,
): Promise<TelegramTarget[]> {
  const targets: TelegramTarget[] = [];
  const names = new Set<string>();
  for (const target of CENTRAL_PUBLICATION_TARGETS) {
    const manifest = JSON.parse(
      await Deno.readTextFile(path.join(root, "manifests", `${target}.json`)),
    ) as TelegramTarget;
    if (manifest.target !== target || !manifest.artifacts.length) {
      throw new Error(`Incomplete Telegram inventory: ${target}`);
    }
    for (const artifact of manifest.artifacts) {
      if (names.has(artifact.fileName)) {
        throw new Error(`Duplicate Telegram filename: ${artifact.fileName}`);
      }
      names.add(artifact.fileName);
      const digest = await hashFile(artifact.path);
      if (
        digest.fileSize !== artifact.fileSize ||
        digest.sha256 !== artifact.sha256 || digest.md5 !== artifact.md5
      ) {
        throw new Error(`Telegram artifact changed: ${artifact.fileName}`);
      }
      if (artifact.fileSize > 2_000_000_000) {
        throw new Error(`Telegram artifact exceeds 2 GB: ${artifact.fileName}`);
      }
    }
    targets.push(manifest);
  }
  return targets;
}

export async function publishTelegramInventory(
  targets: TelegramTarget[],
  previous: TelegramPublicationRecord[],
  send: (
    artifact: TelegramArtifact,
    target: TelegramTarget,
  ) => Promise<TelegramReceipt>,
  checkpoint: (records: TelegramPublicationRecord[]) => Promise<void>,
): Promise<TelegramPublicationRecord[]> {
  const records: TelegramPublicationRecord[] = [];
  for (const target of targets) {
    for (const artifact of target.artifacts) {
      const prior = previous.find((record) =>
        record.buildId === target.buildId &&
        record.fileName === artifact.fileName
      );
      if (
        prior &&
        (prior.sha256 !== artifact.sha256 ||
          prior.fileSize !== artifact.fileSize || prior.md5 !== artifact.md5)
      ) {
        throw new Error(`Conflicting Telegram receipt: ${artifact.fileName}`);
      }
    }
  }
  // One document at a time avoids channel flood limits; the adapter uploads parts concurrently.
  for (const target of targets) {
    for (const artifact of target.artifacts) {
      const prior = previous.find((record) =>
        record.buildId === target.buildId &&
        record.fileName === artifact.fileName
      );
      let record: TelegramPublicationRecord;
      if (prior?.status === "uploaded" && prior.receipt) {
        record = { ...prior, path: artifact.path };
      } else {
        try {
          const receipt = await send(artifact, target);
          record = {
            ...artifact,
            buildId: target.buildId,
            target: target.target,
            status: "uploaded",
            receipt,
          };
        } catch (error) {
          record = {
            ...artifact,
            buildId: target.buildId,
            target: target.target,
            status: "failed",
            error: error instanceof Error ? error.message : String(error),
          };
        }
      }
      records.push(record);
      // Retain successful message ids immediately, even if a later upload fails.
      await checkpoint([
        ...previous.filter((prior) =>
          !records.some((record) =>
            record.buildId === prior.buildId &&
            record.fileName === prior.fileName
          )
        ),
        ...records,
      ]);
    }
  }
  return records;
}

export async function registerTelegramInventory(
  targets: TelegramTarget[],
  records: TelegramPublicationRecord[],
  token: string,
  fetcher = fetch,
): Promise<void> {
  const errors: string[] = [];
  for (const target of targets) {
    const artifacts = target.artifacts.map((artifact) => {
      const record = records.find((entry) =>
        entry.buildId === target.buildId && entry.fileName === artifact.fileName
      );
      if (record?.status !== "uploaded" || !record.receipt) return null;
      return {
        fileName: artifact.fileName,
        fileSize: artifact.fileSize,
        sha256: artifact.sha256,
        md5: artifact.md5,
        messageId: record.receipt.messageId,
        documentId: record.receipt.documentId,
      };
    });
    if (artifacts.some((artifact) => artifact === null)) continue;
    try {
      const response = await fetcher(
        "https://archive.nyanpasu.org/archive/builds",
        {
          method: "POST",
          headers: {
            authorization: `Bearer ${token}`,
            "content-type": "application/json",
          },
          body: JSON.stringify({
            ...target,
            schemaVersion: 2,
            storageProvider: "telegram",
            buildId: `tg-${target.buildId}`,
            itemIdentifier: "ClashNyanpasu",
            artifacts,
          }),
          redirect: "error",
          signal: AbortSignal.timeout(30_000),
        },
      );
      const detail = await response.text();
      if (!response.ok) {
        throw new Error(
          `Archive registration HTTP ${response.status}: ${
            detail.slice(0, 2000)
          }`,
        );
      }
      const result = JSON.parse(detail);
      if (
        result.buildId !== `tg-${target.buildId}` ||
        result.status !== "ready" ||
        result.artifacts?.length !== artifacts.length
      ) {
        throw new Error("Archive did not confirm the complete Telegram index");
      }
    } catch (error) {
      errors.push(
        `${target.target}: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
    }
  }
  if (errors.length) throw new Error(errors.join("\n"));
}
