'use strict'
'require view'
'require ui'
'require nyanpasu.api as api'

const LOG_LIMIT = 200

const LEVELS = ['', 'debug', 'info', 'warning', 'error']

function formatTime(record) {
  if (record.time != null) {
    return record.time
  }

  return record.received_at == null
    ? '-'
    : new Date(record.received_at).toLocaleString()
}

function levelClass(type) {
  switch (type) {
    case 'error':
      return 'label-danger'
    case 'warning':
      return 'label-warning'
    default:
      return ''
  }
}

return view.extend({
  load() {
    return this.fetch('')
  },

  /** The newest log lines at or above `level`, with the status of the log store. */
  fetch(level) {
    return Promise.all([
      api.settle(
        api.call('query_core_logs', {
          query: {
            direction: 'latest',
            cursor: null,
            level: level === '' ? null : level,
            keyword: '',
            limit: LOG_LIMIT,
          },
        }),
      ),
      api.settle(api.call('get_core_log_status')),
    ])
  },

  render([page, status]) {
    this.level = E(
      'select',
      { class: 'cbi-input-select' },
      LEVELS.map((level) =>
        E('option', { value: level }, level === '' ? _('All levels') : level),
      ),
    )
    this.statusLine = E('div', { class: 'cbi-section-descr' })
    this.output = E('div', { class: 'cbi-section-node' })

    this.paint(page, status)

    return E([
      E('h2', {}, _('Nyanpasu logs')),
      E('div', { class: 'cbi-section' }, [
        E('div', { class: 'cbi-page-actions left' }, [
          this.level,
          ' ',
          E(
            'button',
            {
              class: 'btn cbi-button cbi-button-action',
              click: ui.createHandlerFn(this, 'handleRefresh'),
            },
            _('Refresh'),
          ),
        ]),
        this.statusLine,
        this.output,
      ]),
    ])
  },

  paint(page, status) {
    if (status.error != null) {
      this.statusLine.replaceChildren(api.renderError(status.error))
    } else {
      const log = status.value

      this.statusLine.replaceChildren(
        _('Stored: %d bytes, discarded: %d lines.').format(
          log.bytes,
          log.discarded,
        ),
        log.error != null
          ? E('div', { class: 'alert-message warning' }, api.text(log.error))
          : '',
      )
    }

    if (page.error != null) {
      this.output.replaceChildren(api.renderError(page.error))
      return
    }

    if (page.value.rows.length === 0) {
      this.output.replaceChildren(E('em', {}, _('There are no log lines.')))
      return
    }

    this.output.replaceChildren(
      E('table', { class: 'table' }, [
        E('tr', { class: 'tr table-titles' }, [
          E('th', { class: 'th' }, _('Time')),
          E('th', { class: 'th' }, _('Level')),
          E('th', { class: 'th' }, _('Message')),
        ]),
        ...page.value.rows.map((row) =>
          E('tr', { class: 'tr' }, [
            E('td', { class: 'td nowrap' }, api.text(formatTime(row.record))),
            E(
              'td',
              { class: 'td' },
              E(
                'span',
                { class: `label ${levelClass(row.record.type)}` },
                api.text(row.record.type),
              ),
            ),
            E('td', { class: 'td' }, [
              E(
                'span',
                { style: 'white-space: pre-wrap; font-family: monospace' },
                api.text(row.record.payload),
              ),
              row.truncated ? E('em', {}, ` ${_('(truncated)')}`) : '',
            ]),
          ]),
        ),
      ]),
    )
  },

  handleRefresh() {
    return this.fetch(this.level.value).then(([page, status]) =>
      this.paint(page, status),
    )
  },

  handleSave: null,
  handleSaveApply: null,
  handleReset: null,
})
