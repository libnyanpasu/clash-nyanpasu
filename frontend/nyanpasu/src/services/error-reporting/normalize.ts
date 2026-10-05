import type {
  FrontendErrorCause,
  FrontendEventKind,
  FrontendEventLevel,
} from '@nyanpasu/rpc/types'

/** A captured event before the reporter adds identity and timing. */
export type EventDraft = {
  kind: FrontendEventKind
  level: FrontendEventLevel
  message: string
  error_name: string | null
  stack: string | null
  causes: FrontendErrorCause[]
  component_stack: string | null
}

const MAX_CAUSES = 3
const MAX_SERIALIZED_LENGTH = 4 * 1024
const MAX_DEPTH = 4

type ErrorDescription = Pick<
  EventDraft,
  'message' | 'error_name' | 'stack' | 'causes'
>

/** Drops the query and fragment, which may carry tokens. */
export function stripQuery(url: string): string {
  return url.replace(/[?#].*$/s, '')
}

function toJsonSafe(value: unknown, depth: number, seen: Set<object>): unknown {
  if (value === null || typeof value !== 'object') {
    switch (typeof value) {
      case 'bigint':
        return `${value}n`
      case 'function':
        return `[Function ${value.name || 'anonymous'}]`
      case 'symbol':
        return value.toString()
      case 'undefined':
        return '[undefined]'
      default:
        return value
    }
  }

  if (value instanceof Error) return `${value.name}: ${value.message}`
  if (seen.has(value)) return '[Circular]'
  if (depth >= MAX_DEPTH) return Array.isArray(value) ? '[Array]' : '[Object]'

  seen.add(value)
  const result = Array.isArray(value)
    ? value.map((item) => toJsonSafe(item, depth + 1, seen))
    : Object.fromEntries(
        Object.entries(value).map(([key, item]) => [
          key,
          toJsonSafe(item, depth + 1, seen),
        ]),
      )
  seen.delete(value)
  return result
}

/** Renders any value as bounded text without invoking its own logging. */
export function serializeValue(value: unknown): string {
  let text: string
  if (typeof value === 'string') {
    text = value
  } else if (value instanceof Error) {
    text = `${value.name}: ${value.message}`
  } else {
    try {
      const safe = toJsonSafe(value, 0, new Set())
      text = typeof safe === 'string' ? safe : JSON.stringify(safe)
    } catch {
      text = Object.prototype.toString.call(value)
    }
  }
  return text.length > MAX_SERIALIZED_LENGTH
    ? text.slice(0, MAX_SERIALIZED_LENGTH)
    : text
}

function causesOf(error: Error): FrontendErrorCause[] {
  const causes: FrontendErrorCause[] = []
  const seen = new Set<unknown>([error])
  let cause: unknown = error.cause

  while (
    cause !== undefined &&
    !seen.has(cause) &&
    causes.length < MAX_CAUSES
  ) {
    seen.add(cause)
    causes.push(
      cause instanceof Error
        ? {
            name: cause.name,
            message: cause.message,
            stack: cause.stack ?? null,
          }
        : { name: null, message: serializeValue(cause), stack: null },
    )
    cause = cause instanceof Error ? cause.cause : undefined
  }

  return causes
}

export function describeError(value: unknown): ErrorDescription {
  if (value instanceof Error) {
    return {
      message: value.message || value.name,
      error_name: value.name,
      stack: value.stack ?? null,
      causes: causesOf(value),
    }
  }

  return {
    message: serializeValue(value),
    error_name: null,
    stack: null,
    causes: [],
  }
}

export function fromConsole(
  level: FrontendEventLevel,
  args: unknown[],
): EventDraft {
  const error = args.find((arg): arg is Error => arg instanceof Error)
  const described = error ? describeError(error) : null

  return {
    kind: 'console',
    level,
    message: args.map(serializeValue).join(' '),
    error_name: described?.error_name ?? null,
    stack: described?.stack ?? null,
    causes: described?.causes ?? [],
    component_stack: null,
  }
}

/** A script error, or a resource that failed to load (`event.target`). */
export function fromWindowError(event: Event): EventDraft {
  if (event instanceof ErrorEvent) {
    const described =
      event.error != null
        ? describeError(event.error)
        : {
            message: event.message,
            error_name: null,
            stack: event.filename
              ? `at ${stripQuery(event.filename)}:${event.lineno}:${event.colno}`
              : null,
            causes: [],
          }
    return {
      kind: 'uncaught_error',
      level: 'error',
      component_stack: null,
      ...described,
    }
  }

  const target = event.target
  const tag =
    target instanceof Element ? target.tagName.toLowerCase() : 'unknown'
  const url =
    target instanceof HTMLScriptElement || target instanceof HTMLImageElement
      ? target.src
      : target instanceof HTMLLinkElement
        ? target.href
        : ''

  return {
    kind: 'uncaught_error',
    level: 'error',
    message: `failed to load <${tag}> ${stripQuery(url)}`.trimEnd(),
    error_name: null,
    stack: null,
    causes: [],
    component_stack: null,
  }
}

export function fromRejection(reason: unknown): EventDraft {
  return {
    kind: 'unhandled_rejection',
    level: 'error',
    component_stack: null,
    ...describeError(reason),
  }
}

export function fromReact(
  kind: Extract<
    FrontendEventKind,
    'react_uncaught' | 'react_caught' | 'react_recoverable'
  >,
  error: unknown,
  componentStack: string | undefined,
): EventDraft {
  return {
    kind,
    level: kind === 'react_recoverable' ? 'warning' : 'error',
    component_stack: componentStack ?? null,
    ...describeError(error),
  }
}
