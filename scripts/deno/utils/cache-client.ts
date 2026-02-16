import { format as formatBytes } from 'jsr:@std/fmt@1/bytes'
import { CHUNK_MULTIPLIER, performChunkedUpload } from './file-server.ts'
import { consola } from './logger.ts'

const CACHE_BASE_URL = 'https://file-server.elaina.moe/cache'

// --- cache chunked upload types ---

interface CacheInitResponse {
  uploadId: string
  key: string
  fileSize: number
  chunkSize: number
  expiresAt: number
}

interface CacheChunkResponse {
  done: boolean
  nextExpectedRanges?: string[]
  key?: string
  size?: number
}

// --- cache chunked upload functions ---

async function initCacheUploadSession(
  key: string,
  fileSize: number,
  token: string,
): Promise<CacheInitResponse> {
  const resp = await fetch(`${CACHE_BASE_URL}/init`, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${token}`,
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({
      key,
      fileSize,
      chunkMultiplier: CHUNK_MULTIPLIER,
    }),
  })

  if (!resp.ok) {
    const body = await resp.text()
    throw new Error(
      `cache upload init failed: ${resp.status} ${resp.statusText} - ${body}`,
    )
  }

  return (await resp.json()) as CacheInitResponse
}

async function uploadCacheChunk(
  uploadId: string,
  chunk: Uint8Array,
  start: number,
  end: number,
  total: number,
  token: string,
): Promise<CacheChunkResponse> {
  const resp = await fetch(`${CACHE_BASE_URL}/chunk`, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${token}`,
      'x-upload-id': uploadId,
      'Content-Range': `bytes ${start}-${end}/${total}`,
      'Content-Type': 'application/octet-stream',
    },
    body: chunk,
  })

  if (!resp.ok) {
    const body = await resp.text()
    throw new Error(
      `cache chunk upload failed: ${resp.status} ${resp.statusText} - ${body}`,
    )
  }

  return (await resp.json()) as CacheChunkResponse
}

export async function uploadCache(
  key: string,
  filePath: string,
  token: string,
): Promise<void> {
  const stat = await Deno.stat(filePath)
  const fileSize = stat.size

  consola.info(
    `uploading cache "${key}" (${formatBytes(fileSize)}) via chunked upload...`,
  )

  const { uploadId, chunkSize } = await initCacheUploadSession(
    key,
    fileSize,
    token,
  )

  await performChunkedUpload({
    filePath,
    fileSize,
    uploadId,
    chunkSize,
    label: `cache "${key}"`,
    uploadChunkFn: (chunk, start, end, total) =>
      uploadCacheChunk(uploadId, chunk, start, end, total, token),
  })

  consola.success(`cache "${key}" uploaded successfully`)
}

export async function downloadCache(
  key: string,
  destPath: string,
  token: string,
): Promise<boolean> {
  consola.info(`downloading cache "${key}"...`)

  const resp = await fetch(`${CACHE_BASE_URL}/${encodeURIComponent(key)}`, {
    method: 'GET',
    headers: {
      'x-authorization': token,
    },
  })

  if (resp.status === 404) {
    consola.info(`cache miss for "${key}"`)
    return false
  }

  if (!resp.ok) {
    const body = await resp.text()
    throw new Error(
      `cache download failed: ${resp.status} ${resp.statusText} - ${body}`,
    )
  }

  const dest = await Deno.open(destPath, {
    write: true,
    create: true,
    truncate: true,
  })

  try {
    await resp.body!.pipeTo(dest.writable)
  } catch {
    try {
      dest.close()
    } catch {
      // already closed
    }
    throw new Error(`failed to write cache to "${destPath}"`)
  }

  consola.success(`cache "${key}" downloaded to "${destPath}"`)
  return true
}

export async function listCacheKeys(
  prefix: string,
  token: string,
): Promise<string[]> {
  consola.debug(`listing cache keys with prefix "${prefix}"...`)

  const resp = await fetch(
    `${CACHE_BASE_URL}?prefix=${encodeURIComponent(prefix)}`,
    {
      method: 'GET',
      headers: {
        'x-authorization': token,
      },
    },
  )

  if (!resp.ok) {
    const body = await resp.text()
    throw new Error(
      `cache list failed: ${resp.status} ${resp.statusText} - ${body}`,
    )
  }

  const keys = (await resp.json()) as string[]
  consola.debug(`found ${keys.length} cache keys matching prefix "${prefix}"`)
  return keys
}
