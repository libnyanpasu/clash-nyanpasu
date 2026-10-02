import { retry } from "jsr:@std/async@1/retry";
import { format as formatBytes } from "jsr:@std/fmt@1/bytes";
import * as path from "jsr:@std/path";
import { consola } from "../shared/logger.ts";

// --- constants ---

export const FILE_SERVER_UPLOAD_URL = "https://archive.nyanpasu.org/upload";
export const FILE_SERVER_BIN_URL = "https://archive.nyanpasu.org/bin";

export const UPLOAD_CONCURRENCY = 3;
export const CHUNK_RETRY_ATTEMPTS = 5;
export const CHUNK_MULTIPLIER = 32;

// --- types ---

export interface UploadResult {
  fileName: string;
  downloadUrl: string;
}

export interface UploadFailure {
  fileName: string;
  message: string;
  stack?: string;
}

export interface UploadBatchResult {
  results: UploadResult[];
  failures: UploadFailure[];
}

function getErrorDetails(
  error: unknown,
): Pick<UploadFailure, "message" | "stack"> {
  const messages: string[] = [];
  const stacks: string[] = [];
  const seen = new Set<unknown>();
  let current: unknown = error;

  while (current instanceof Error && !seen.has(current)) {
    seen.add(current);
    messages.push(`${current.name}: ${current.message}`);
    if (current.stack) stacks.push(current.stack);
    current = current.cause;
  }
  if (current !== undefined && !(current instanceof Error)) {
    messages.push(String(current));
  }

  return {
    message: messages.join("\nCaused by: "),
    ...(stacks.length > 0 ? { stack: stacks.join("\nCaused by:\n") } : {}),
  };
}

export interface InitResponse {
  uploadId: string;
  chunkSize: number;
}

export interface ChunkResponse {
  done: boolean;
  file?: { id: string };
}

export interface ChunkedUploadOptions<T> {
  filePath: string;
  fileSize: number;
  uploadId: string;
  chunkSize: number;
  label: string;
  uploadChunkFn: (
    chunk: Uint8Array,
    start: number,
    end: number,
    total: number,
  ) => Promise<T & { done: boolean }>;
}

// --- upload functions ---

export async function initUploadSession(
  fileName: string,
  fileSize: number,
  mimeType: string | null,
  token: string,
  folderPath?: string,
): Promise<InitResponse> {
  const body: Record<string, unknown> = {
    filename: fileName,
    fileSize,
    mimeType,
    chunkMultiplier: CHUNK_MULTIPLIER,
  };
  if (folderPath) {
    body.folderPath = folderPath;
  }

  const resp = await fetch(`${FILE_SERVER_UPLOAD_URL}/init`, {
    method: "POST",
    headers: {
      "x-authorization": token,
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });

  if (!resp.ok) {
    const body = await resp.text();
    throw new Error(
      `upload init failed: ${resp.status} ${resp.statusText} - ${body}`,
    );
  }

  const data = await resp.json() as Partial<InitResponse>;
  if (
    typeof data.uploadId !== "string" || !data.uploadId ||
    typeof data.chunkSize !== "number" ||
    !Number.isSafeInteger(data.chunkSize) ||
    data.chunkSize <= 0
  ) {
    throw new Error(
      `upload init returned an invalid response: ${JSON.stringify(data)}`,
    );
  }

  return data as InitResponse;
}

export async function uploadChunk(
  uploadId: string,
  chunk: Uint8Array,
  start: number,
  end: number,
  total: number,
  token: string,
): Promise<ChunkResponse> {
  const resp = await fetch(`${FILE_SERVER_UPLOAD_URL}/chunk`, {
    method: "POST",
    headers: {
      "x-authorization": token,
      "x-upload-id": uploadId,
      "Content-Range": `bytes ${start}-${end}/${total}`,
      "Content-Type": "application/octet-stream",
    },
    body: chunk as unknown as BodyInit,
  });

  if (!resp.ok) {
    const body = await resp.text();
    throw new Error(
      `chunk upload failed: ${resp.status} ${resp.statusText} - ${body}`,
    );
  }

  const data = await resp.json() as Partial<ChunkResponse>;
  if (typeof data.done !== "boolean") {
    throw new Error(
      `chunk upload returned an invalid response: ${JSON.stringify(data)}`,
    );
  }
  if (data.done && (typeof data.file?.id !== "string" || !data.file.id)) {
    throw new Error(
      `completed chunk upload did not return a file id: ${
        JSON.stringify(data)
      }`,
    );
  }

  return data as ChunkResponse;
}

export async function performChunkedUpload<T>(
  options: ChunkedUploadOptions<T>,
): Promise<T & { done: boolean }> {
  const { filePath, fileSize, uploadId, chunkSize, label, uploadChunkFn } =
    options;

  consola.debug(
    `upload session created: uploadId=${uploadId}, chunkSize=${
      formatBytes(chunkSize)
    }`,
  );

  const file = await Deno.open(filePath, { read: true });
  try {
    let start = 0;
    let chunkIndex = 0;
    const totalChunks = Math.ceil(fileSize / chunkSize);
    let lastLogTime = Date.now();
    let lastLogUploaded = 0;

    while (start < fileSize) {
      const endExclusive = Math.min(start + chunkSize, fileSize);
      const size = endExclusive - start;
      const buf = new Uint8Array(size);
      await file.seek(start, Deno.SeekMode.Start);
      let bytesRead = 0;
      while (bytesRead < size) {
        const n = await file.read(buf.subarray(bytesRead));
        if (n === null) break;
        bytesRead += n;
      }
      if (bytesRead !== size) {
        throw new Error(
          `Unexpected end of file while reading ${label}: expected ${size} bytes, got ${bytesRead}`,
        );
      }

      const end = endExclusive - 1;
      chunkIndex++;

      const data = await retry(
        () => uploadChunkFn(buf.subarray(0, bytesRead), start, end, fileSize),
        { maxAttempts: CHUNK_RETRY_ATTEMPTS },
      );

      const now = Date.now();
      const elapsed = now - lastLogTime;
      if (elapsed >= 1000 || data.done) {
        const speed = ((endExclusive - lastLogUploaded) / elapsed) * 1000;
        lastLogTime = now;
        lastLogUploaded = endExclusive;
        const pct = Math.floor((endExclusive / fileSize) * 100);
        consola.info(
          `  ${label} ${chunkIndex}/${totalChunks}: ${
            formatBytes(endExclusive)
          }/${formatBytes(fileSize)} (${pct}%) ${formatBytes(speed)}/s`,
        );
      }

      if (data.done) {
        return data;
      }

      start = endExclusive;
    }
  } finally {
    file.close();
  }

  throw new Error(`Upload of ${label} ended unexpectedly without done=true`);
}

export async function uploadToFileServer(
  filePath: string,
  token: string,
  folderPath?: string,
): Promise<UploadResult> {
  const fileName = path.basename(filePath);
  const stat = await Deno.stat(filePath);
  const fileSize = stat.size;

  consola.info(
    `uploading ${fileName} (${formatBytes(fileSize)}) to file server${
      folderPath ? ` [folder: ${folderPath}]` : ""
    }...`,
  );

  const { uploadId, chunkSize } = await initUploadSession(
    fileName,
    fileSize,
    null,
    token,
    folderPath,
  );

  const data = await performChunkedUpload({
    filePath,
    fileSize,
    uploadId,
    chunkSize,
    label: fileName,
    uploadChunkFn: (chunk, start, end, total) =>
      uploadChunk(uploadId, chunk, start, end, total, token),
  });

  const fileId = data.file?.id;
  if (!fileId) {
    throw new Error(`Upload of ${fileName} completed without a file id`);
  }

  const downloadUrl = `${FILE_SERVER_BIN_URL}/${fileId}`;
  consola.success(`uploaded ${fileName} -> ${downloadUrl}`);
  return { fileName, downloadUrl };
}

export async function uploadAllFilesWithResults(
  filePaths: string[],
  token: string,
  folderPath?: string,
  uploadFile: typeof uploadToFileServer = uploadToFileServer,
): Promise<UploadBatchResult> {
  const results: UploadResult[] = [];
  const failures: UploadFailure[] = [];
  const queue = [...filePaths];
  const inFlight: Promise<void>[] = [];

  async function processNext(): Promise<void> {
    while (queue.length > 0) {
      const filePath = queue.shift()!;
      try {
        const result = await retry(
          () => uploadFile(filePath, token, folderPath),
          {
            maxAttempts: CHUNK_RETRY_ATTEMPTS,
          },
        );
        results.push(result);
      } catch (error) {
        failures.push({
          fileName: path.basename(filePath),
          ...getErrorDetails(error),
        });
      }
    }
  }

  const workers = Math.min(UPLOAD_CONCURRENCY, filePaths.length);
  for (let i = 0; i < workers; i++) {
    inFlight.push(processNext());
  }
  await Promise.all(inFlight);

  return { results, failures };
}

export async function uploadAllFiles(
  filePaths: string[],
  token: string,
  folderPath?: string,
): Promise<UploadResult[]> {
  const { results, failures } = await uploadAllFilesWithResults(
    filePaths,
    token,
    folderPath,
  );
  if (failures.length > 0) {
    throw new AggregateError(
      failures.map(({ fileName, message }) =>
        new Error(`${fileName}: ${message}`)
      ),
      `Failed to upload ${failures.length} of ${filePaths.length} files`,
    );
  }
  return results;
}
