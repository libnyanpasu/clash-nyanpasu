/** Whether an error is a Node.js file system error for a missing path. */
export function isNotFound(error: unknown): boolean {
  return error instanceof Error && 'code' in error && error.code === 'ENOENT'
}
