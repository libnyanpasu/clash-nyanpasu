'use strict'
'require view'
'require poll'
'require ui'
'require nyanpasu.api as api'

const STATUS_POLL_SECONDS = 5

function healthLabel(healthy) {
  if (healthy === true) {
    return _('Healthy')
  }

  return healthy === false ? _('Unhealthy') : _('Unknown')
}

function profileLabel(profiles) {
  if (profiles.current == null) {
    return _('None selected')
  }

  const item = profiles.items.find(
    (profile) => profile.uid === profiles.current,
  )

  return item != null
    ? `${item.name} (${item.uid})`
    : `${profiles.current} (${_('missing')})`
}

return view.extend({
  load() {
    return Promise.all([
      api.canWrite(),
      api.settle(api.call('get_core_status')),
      api.settle(api.call('get_profiles')),
      api.settle(api.call('get_runtime_yaml_redacted')),
    ])
  },

  render([writable, status, profiles, yaml]) {
    this.writable = writable
    this.fullYaml = false

    this.statusBox = E('div', { class: 'cbi-section-node' })
    this.profileBox = E('div', { class: 'cbi-section-node' })
    this.yamlBox = E('div', { class: 'cbi-section-node' })
    this.controls = E('div', { class: 'cbi-section-node' })

    this.paintStatus(status)
    this.paintProfile(profiles)
    this.paintYaml(yaml)
    this.paintControls()

    poll.add(() => {
      return api
        .settle(api.call('get_core_status'))
        .then((next) => this.paintStatus(next))
    }, STATUS_POLL_SECONDS)

    return E([
      E('h2', {}, _('Nyanpasu')),
      E('div', { class: 'cbi-map-descr' }, [
        _('Status of the Nyanpasu core, reported by nyanpasu-server.'),
        writable
          ? ''
          : ` ${_('This session is read-only: starting, stopping and the full runtime YAML are not available.')}`,
      ]),
      E('div', { class: 'cbi-section' }, [
        E('h3', {}, _('Core')),
        this.statusBox,
        this.controls,
      ]),
      E('div', { class: 'cbi-section' }, [
        E('h3', {}, _('Selected profile')),
        this.profileBox,
      ]),
      E('div', { class: 'cbi-section' }, [
        E('h3', {}, _('Runtime configuration')),
        this.yamlBox,
      ]),
    ])
  },

  paintStatus(result) {
    if (result.error != null) {
      this.statusBox.replaceChildren(api.renderError(result.error))
      return
    }

    const info = result.value
    const { kind, pid } = api.describeState(info.state)

    this.statusBox.replaceChildren(
      E('table', { class: 'table' }, [
        this.row(_('State'), kind),
        this.row(_('PID'), pid == null ? '-' : String(pid)),
        this.row(_('Health'), healthLabel(info.healthy)),
        this.row(_('Host'), info.host == null ? '-' : String(info.host)),
        this.row(
          _('Connectivity'),
          info.connectivity != null ? String(info.connectivity.kind) : '-',
        ),
      ]),
    )
  },

  row(label, value) {
    return E('tr', { class: 'tr' }, [
      E('td', { class: 'td left', width: '33%' }, E('strong', {}, label)),
      E('td', { class: 'td left' }, api.text(value)),
    ])
  },

  paintProfile(result) {
    if (result.error != null) {
      this.profileBox.replaceChildren(api.renderError(result.error))
      return
    }

    this.profileBox.replaceChildren(
      E('p', {}, [
        profileLabel(result.value),
        ' ',
        E(
          'a',
          { href: L.url('admin/services/nyanpasu/profiles') },
          _('Manage profiles'),
        ),
      ]),
    )
  },

  paintControls() {
    if (!this.writable) {
      this.controls.replaceChildren()
      return
    }

    this.startButton = E(
      'button',
      {
        class: 'btn cbi-button cbi-button-action',
        click: ui.createHandlerFn(this, 'handleStart'),
      },
      _('Start / apply'),
    )
    this.stopButton = E(
      'button',
      {
        class: 'btn cbi-button cbi-button-negative',
        click: ui.createHandlerFn(this, 'handleStop'),
      },
      _('Stop'),
    )

    this.controls.replaceChildren(
      E('div', { class: 'cbi-page-actions left' }, [
        this.startButton,
        ' ',
        this.stopButton,
      ]),
    )
  },

  paintYaml(result) {
    const children = []

    if (this.writable) {
      const toggle = E('input', {
        type: 'checkbox',
        change: ui.createHandlerFn(this, 'handleFullYamlToggle'),
      })

      toggle.checked = this.fullYaml
      children.push(
        E('p', {}, [
          E('label', {}, [
            toggle,
            ` ${_('Show the full YAML, including secrets')}`,
          ]),
        ]),
      )
    }

    if (result.error != null) {
      children.push(api.renderError(result.error))
    } else {
      children.push(
        this.fullYaml
          ? E(
              'div',
              { class: 'alert-message warning' },
              _('This YAML contains secrets. Do not share it.'),
            )
          : E(
              'div',
              { class: 'cbi-section-descr' },
              _('Secrets are redacted.'),
            ),
        E(
          'pre',
          { style: 'max-height: 30em; overflow: auto; white-space: pre' },
          api.text(result.value),
        ),
      )
    }

    this.yamlBox.replaceChildren(...children)
  },

  /** Starts the core, or applies the selected profile to a running one. */
  handleStart() {
    return this.runLifecycle('reconcile_core', (report) => {
      const { kind } = api.describeState(
        report && report.status && report.status.state,
      )

      return _('Core reconciled; it is now: %s.').format(kind)
    })
  },

  handleStop() {
    return this.runLifecycle('stop_core', (report) => {
      const { kind } = api.describeState(
        report && report.status && report.status.state,
      )

      return _('Core stop requested; it is now: %s.').format(kind)
    })
  },

  runLifecycle(method, describe) {
    return api.call(method).then(
      (report) => {
        ui.addNotification(null, E('p', {}, api.text(describe(report))), 'info')
        return this.refresh()
      },
      (error) => {
        api.notifyError(error)
        return this.refresh()
      },
    )
  },

  handleFullYamlToggle(ev) {
    this.fullYaml = ev.target.checked
    return this.refreshYaml()
  },

  refreshYaml() {
    const method = this.fullYaml
      ? 'get_runtime_yaml'
      : 'get_runtime_yaml_redacted'

    return api.settle(api.call(method)).then((yaml) => this.paintYaml(yaml))
  },

  refresh() {
    return Promise.all([
      api.settle(api.call('get_core_status')),
      api.settle(api.call('get_profiles')),
    ]).then(([status, profiles]) => {
      this.paintStatus(status)
      this.paintProfile(profiles)
      return this.refreshYaml()
    })
  },

  handleSave: null,
  handleSaveApply: null,
  handleReset: null,
})
