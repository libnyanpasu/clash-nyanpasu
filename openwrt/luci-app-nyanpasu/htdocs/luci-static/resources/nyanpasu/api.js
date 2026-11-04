'use strict'
'require baseclass'
'require rpc'
'require ui'

/**
 * The ubus object `nyanpasu` published by the rpcd plugin, one method per
 * nyanpasu-server command. Every method takes `{ params: {...} }`, where the
 * inner object is the command's camelCase argument object, and replies with
 *
 *   { ok: true,  result }                          the command's result
 *   { ok: false, error }                           an RpcError
 *   { ok: false, transport: true, error }          the CLI could not reach the server
 */

const OBJECT = 'nyanpasu'

// Commands every login with the `read` ACL may call, and the commands that
// need `write`. rpcd enforces this; the views use the write probe only to hide
// controls that would be refused.
const READ_METHODS = [
  'get_core_status',
  'get_profiles',
  'get_runtime_yaml_redacted',
  'inspect_runtime',
  'query_core_logs',
  'get_core_log_status',
]

const WRITE_METHODS = [
  'create_profile',
  'delete_profile',
  'replace_profile_definition',
  'set_global_transforms',
  'activate_profile',
  'read_profile_file',
  'save_profile_file',
  'reconcile_core',
  'stop_core',
  'get_runtime_yaml',
]

// The write probe: any method of the write group answers the same question.
const WRITE_PROBE = 'stop_core'

// The placeholder the client rewrites to `<uid>.<ext>` when it adds a profile.
const PENDING_FILE = {
  yaml: 'pending.yaml',
  javascript: 'pending.js',
  lua: 'pending.lua',
}

const sessionAccess = rpc.declare({
  object: 'session',
  method: 'access',
  params: ['scope', 'object', 'function'],
  expect: { access: false },
})

const calls = {}

for (const method of READ_METHODS.concat(WRITE_METHODS)) {
  calls[method] = rpc.declare({
    object: OBJECT,
    method,
    params: ['params'],
  })
}

/**
 * Untrusted text as a DOM node. LuCI's `E()` parses a lone string child as HTML,
 * so every value that came from the server or the user goes through this.
 */
function text(value) {
  return document.createTextNode(String(value))
}

/** A failed command, carrying the RpcError it was reported with. */
class CommandError extends Error {
  constructor(method, error, transport) {
    const message =
      error != null && typeof error.message === 'string'
        ? error.message
        : _('Unknown error')

    super(message)
    this.name = 'CommandError'
    this.method = method
    this.rpcError = error
    this.transport = transport === true
  }
}

function errorKindLabel(kind) {
  if (typeof kind === 'string') {
    return kind
  }

  if (kind != null && typeof kind === 'object') {
    const inner =
      kind.error != null && typeof kind.error === 'object'
        ? kind.error.kind
        : null

    return [kind.domain, inner].filter((part) => part != null).join(' / ')
  }

  return ''
}

/** Whether the session may call the write methods. */
function canWrite() {
  return sessionAccess('ubus', OBJECT, WRITE_PROBE).catch(() => false)
}

/**
 * Runs one command. Resolves to its result; rejects with a CommandError for an
 * RpcError or an unreachable server, and with the ubus error (for example
 * "Permission denied") when rpcd refuses the call.
 */
function call(method, params) {
  // rpc.declare names the single positional argument `params`.
  return calls[method](params == null ? {} : params).then((reply) => {
    if (reply == null || reply.ok !== true) {
      throw new CommandError(
        method,
        reply == null ? null : reply.error,
        reply != null && reply.transport === true,
      )
    }

    return reply.result == null ? null : reply.result
  })
}

/** Resolves `{ value }` or `{ error }` so one failing source does not hide the rest. */
function settle(promise) {
  return promise.then(
    (value) => ({ value }),
    (error) => ({ error }),
  )
}

/** A visible description of any rejection from `call`. */
function renderError(error) {
  if (!(error instanceof CommandError)) {
    return E('div', { class: 'alert-message danger' }, [
      E('strong', {}, _('Request failed')),
      E('div', {}, text((error && error.message) || error)),
    ])
  }

  const rpcError = error.rpcError || {}
  const kind = errorKindLabel(rpcError.kind)
  const title = error.transport
    ? _('Cannot reach nyanpasu-server')
    : _('Command failed')
  const detail =
    typeof rpcError.detail === 'string' && rpcError.detail !== ''
      ? rpcError.detail
      : null

  return E('div', { class: 'alert-message danger' }, [
    E('strong', {}, text(title + (kind ? ` (${kind})` : ''))),
    E('div', {}, text(error.message)),
    detail
      ? E('details', {}, [
          E('summary', {}, _('Details')),
          E('pre', { style: 'white-space: pre-wrap' }, text(detail)),
        ])
      : '',
  ])
}

function renderDegradation(degradation) {
  const reason =
    degradation.reason != null && typeof degradation.reason === 'object'
      ? degradation.reason.code
      : degradation.reason

  return E('li', {}, [
    E('code', {}, text(`${degradation.phase} / ${reason}`)),
    ': ',
    degradation.message,
    degradation.retryable ? ` (${_('retryable')})` : '',
  ])
}

/**
 * Tells the user how a mutation ended. A committed mutation with degradations
 * is a warning that lists each one, never a plain success.
 */
function notifyOutcome(outcome, successText) {
  if (
    outcome != null &&
    outcome.status === 'committed_degraded' &&
    Array.isArray(outcome.degradations)
  ) {
    ui.addNotification(
      null,
      E('div', {}, [
        E(
          'p',
          {},
          _('%s It was committed, but some effects are degraded:').format(
            successText,
          ),
        ),
        E('ul', {}, outcome.degradations.map(renderDegradation)),
      ]),
      'warning',
    )
    return
  }

  ui.addNotification(null, E('p', {}, successText), 'info')
}

function notifyError(error) {
  ui.addNotification(null, renderError(error), 'danger')
}

/** Runs a mutation, reporting how it ended; resolves to whether it committed. */
function mutate(method, params, successText) {
  return call(method, params).then(
    (outcome) => {
      notifyOutcome(outcome, successText)
      return true
    },
    (error) => {
      notifyError(error)
      return false
    },
  )
}

/** `{ kind, pid }` from the externally tagged core state. */
function describeState(state) {
  if (state == null || typeof state !== 'object') {
    return { kind: 'Unknown', pid: null }
  }

  const kind = Object.keys(state)[0]

  if (kind == null) {
    return { kind: 'Unknown', pid: null }
  }

  const body = state[kind]

  return {
    kind,
    pid: body != null && typeof body.pid === 'number' ? body.pid : null,
  }
}

/** The managed local source of a profile file, as the client expects a new one. */
function pendingSource(fileKind) {
  return {
    type: 'local',
    binding: { type: 'managed', file: PENDING_FILE[fileKind] },
  }
}

function configDefinition() {
  return {
    type: 'config',
    config: { type: 'file', source: pendingSource('yaml'), transforms: [] },
  }
}

function scriptDefinition(runtime) {
  return {
    type: 'transform',
    transform: {
      type: 'script',
      source: pendingSource(runtime),
      runtime,
    },
  }
}

function newProfileRequest(name, definition) {
  return {
    metadata: { name, desc: null, custom_name: true },
    definition,
    folder: null,
  }
}

return baseclass.extend({
  call,
  canWrite,
  settle,
  mutate,
  text,
  renderError,
  notifyError,
  notifyOutcome,
  describeState,
  configDefinition,
  scriptDefinition,
  newProfileRequest,
})
