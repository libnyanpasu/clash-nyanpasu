'use strict'
'require view'
'require rpc'
'require uci'
'require ui'
'require poll'

/**
 * Services -> Nyanpasu: the nyanpasu service and a way into its WebUI. LuCI
 * implements no management UI of its own. The service is controlled through
 * LuCI's standard `rc` ubus object (procd), and "Open WebUI" opens the WebUI,
 * which nyanpasu-server serves on its own port, in a new window.
 *
 * The WebUI needs a one-time ticket to sign the browser in. The rpcd method
 * `webui_ticket` (write ACL) returns it:
 *
 *   { ok: true,  result: { path, port } }
 *   { ok: false, error }                    an RpcError from the server
 *   { ok: false, transport: true, error }   the CLI could not reach the server
 *
 * A login without write access sees the status only.
 */

const SERVICE = 'nyanpasu'

// How "Open WebUI" opens the window; the UCI option nyanpasu.main.window_mode.
const WINDOW_MODES = {
  tab: _('New tab'),
  popup: _('Popup window'),
}
const POPUP_FEATURES = 'popup,width=1280,height=800'
const STATUS_POLL_SECONDS = 3

const callRcList = rpc.declare({
  object: 'rc',
  method: 'list',
  params: ['name'],
  expect: { '': {} },
})

const callRcInit = rpc.declare({
  object: 'rc',
  method: 'init',
  params: ['name', 'action'],
  reject: true,
})

const callUciCommit = rpc.declare({
  object: 'uci',
  method: 'commit',
  params: ['config'],
  reject: true,
})

const callWebuiTicket = rpc.declare({
  object: 'nyanpasu',
  method: 'webui_ticket',
})

function readStatus() {
  return L.resolveDefault(callRcList(SERVICE), {}).then(
    (list) => list[SERVICE] ?? null,
  )
}

function windowMode() {
  const mode = uci.get('nyanpasu', 'main', 'window_mode')

  return mode in WINDOW_MODES ? mode : 'tab'
}

function notify(text) {
  ui.addNotification(null, E('p', text), 'danger')
}

/**
 * Opens the WebUI. The window is opened inside the click handler, before the
 * ticket is known, so the popup blocker accepts it; its location is set when
 * the ticket arrives. It gets no `opener`, so the WebUI cannot reach LuCI.
 */
function openWebui(mode) {
  const target = window.open(
    'about:blank',
    '_blank',
    mode === 'popup' ? POPUP_FEATURES : undefined,
  )

  if (target == null) {
    notify(
      _('The browser blocked the new window. Allow pop-ups for this page.'),
    )

    return Promise.resolve()
  }

  target.opener = null

  const fail = (reason) => {
    target.close()
    notify(`${_('The Nyanpasu WebUI is not available:')} ${reason}`)
  }

  return callWebuiTicket().then(
    (reply) => {
      if (reply?.ok === true) {
        const { path, port } = reply.result

        target.location.href = `${location.protocol}//${location.hostname}:${port}${path}`
      } else {
        fail(
          reply?.error?.message ??
            _('The server did not answer. Check that the service is running.'),
        )
      }
    },
    (error) => fail(error?.message ?? error),
  )
}

return view.extend({
  load() {
    return Promise.all([readStatus(), uci.load('nyanpasu')]).then(
      ([status]) => status,
    )
  },

  render(status) {
    // Without write access the page shows the status and nothing to press.
    const writable = L.hasViewPermission() !== false

    const state = E('strong')
    const bootToggle = E('input', { type: 'checkbox' })
    const bootText = E('span')

    const control = (label, action, className) =>
      E(
        'button',
        {
          class: `cbi-button ${className}`,
          click: ui.createHandlerFn(this, () => this.control(action)),
        },
        label,
      )
    const start = control(_('Start'), 'start', 'cbi-button-positive')
    const stop = control(_('Stop'), 'stop', 'cbi-button-negative')
    const restart = control(_('Restart'), 'restart', 'cbi-button-action')

    const open = E(
      'button',
      {
        class: 'cbi-button cbi-button-action important',
        click: () => openWebui(windowMode()),
      },
      _('Open WebUI'),
    )

    const modeSelect = E(
      'select',
      {
        class: 'cbi-input-select',
        change: ui.createHandlerFn(this, (event) =>
          this.saveWindowMode(event.target.value),
        ),
      },
      Object.entries(WINDOW_MODES).map(([value, label]) =>
        E(
          'option',
          { value, selected: value === windowMode() ? '' : null },
          label,
        ),
      ),
    )

    this.show = (info) => {
      const running = info?.running === true

      state.textContent =
        info == null
          ? _('Not installed')
          : running
            ? _('Running')
            : _('Stopped')
      state.style.color = running ? 'green' : 'red'
      bootText.textContent = info?.enabled === true ? _('Yes') : _('No')
      bootToggle.checked = info?.enabled === true
      bootToggle.disabled = info == null
      start.disabled = running || info == null
      stop.disabled = !running
      restart.disabled = info == null
    }
    this.show(status)

    bootToggle.addEventListener(
      'change',
      ui.createHandlerFn(this, () =>
        this.control(bootToggle.checked ? 'enable' : 'disable'),
      ),
    )

    poll.add(
      () => readStatus().then((info) => this.show(info)),
      STATUS_POLL_SECONDS,
    )

    const row = (title, content) =>
      E('div', { class: 'cbi-value' }, [
        E('label', { class: 'cbi-value-title' }, title),
        E('div', { class: 'cbi-value-field' }, content),
      ])

    return E('div', { class: 'cbi-map' }, [
      E('h2', _('Nyanpasu')),
      E(
        'div',
        { class: 'cbi-section' },
        writable
          ? [
              row(_('Status'), state),
              row(_('Start at boot'), bootToggle),
              row(_('Service'), [start, ' ', stop, ' ', restart]),
              row(_('Open WebUI in'), modeSelect),
              row(_('WebUI'), open),
            ]
          : [row(_('Status'), state), row(_('Start at boot'), bootText)],
      ),
    ])
  },

  control(action) {
    // rc init answers with no data; a failed init script is a rejection.
    return callRcInit(SERVICE, action)
      .catch((error) =>
        notify(
          `${_('The service did not accept this action:')} ${action} (${error?.message ?? error})`,
        ),
      )
      .then(() => readStatus())
      .then((info) => this.show(info))
  },

  // Only the UCI file changes; the service is not restarted for it.
  saveWindowMode(mode) {
    uci.set('nyanpasu', 'main', 'window_mode', mode)

    return uci
      .save()
      .then(() => callUciCommit('nyanpasu'))
      .catch((error) =>
        notify(
          `${_('Could not save the setting:')} ${error?.message ?? error}`,
        ),
      )
  },

  handleSave: null,
  handleSaveApply: null,
  handleReset: null,
})
