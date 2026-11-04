'use strict'
'require view'
'require ui'
'require nyanpasu.api as api'

const SCRIPT_TEMPLATES = {
  javascript: 'function main(config) {\n  return config\n}\n',
  lua: 'return config\n',
}

const RUNTIME_LABELS = {
  javascript: 'JavaScript',
  lua: 'Lua',
}

function profileKind(item) {
  if (item.type === 'config') {
    return _('Config')
  }

  const transform = item.transform || {}

  if (transform.type === 'script') {
    return _('%s transform').format(
      RUNTIME_LABELS[transform.runtime] || transform.runtime,
    )
  }

  return _('Overlay')
}

function attachedTransforms(item) {
  return (item.config && item.config.transforms) || []
}

function textarea(rows, value) {
  const area = E('textarea', {
    class: 'cbi-input-textarea',
    rows,
    wrap: 'off',
    spellcheck: 'false',
    style: 'width: 100%; font-family: monospace',
  })

  area.value = value

  return area
}

function field(label, control) {
  return E('div', { class: 'cbi-value' }, [
    E('label', { class: 'cbi-value-title' }, label),
    E('div', { class: 'cbi-value-field' }, control),
  ])
}

return view.extend({
  load() {
    return Promise.all([api.canWrite(), api.settle(api.call('get_profiles'))])
  },

  render([writable, profiles]) {
    this.writable = writable
    this.profiles = profiles

    this.listBox = E('div', { class: 'cbi-section-node' })
    this.transformsBox = E('div', { class: 'cbi-section-node' })

    this.paint()

    const sections = [
      E('h2', {}, _('Nyanpasu profiles')),
      E('div', { class: 'cbi-section' }, [
        E('h3', {}, _('Profiles')),
        this.listBox,
      ]),
    ]

    if (writable) {
      sections.push(
        E('div', { class: 'cbi-section' }, [
          E('h3', {}, _('Transforms of the selected profile')),
          E(
            'div',
            { class: 'cbi-section-descr' },
            _('Transforms run in this order, after the profile is parsed.'),
          ),
          this.transformsBox,
        ]),
        this.renderImportForm(),
        this.renderTransformForm(),
      )
    } else {
      sections.push(
        E(
          'div',
          { class: 'alert-message notice' },
          _('This session is read-only: profiles cannot be changed.'),
        ),
      )
    }

    return E(sections)
  },

  /** The profile document, or null after rendering why it is unavailable. */
  profileDocument(box) {
    if (this.profiles.error != null) {
      box.replaceChildren(api.renderError(this.profiles.error))
      return null
    }

    return this.profiles.value
  },

  paint() {
    this.paintList()
    this.paintTransforms()
  },

  paintList() {
    const profileDocument = this.profileDocument(this.listBox)

    if (profileDocument == null) {
      return
    }

    if (profileDocument.items.length === 0) {
      this.listBox.replaceChildren(E('em', {}, _('There are no profiles yet.')))
      return
    }

    const names = new Map(
      profileDocument.items.map((item) => [item.uid, item.name]),
    )
    const rows = profileDocument.items.map((item) => {
      const selected = item.uid === profileDocument.current
      const transforms = attachedTransforms(item).map(
        (uid) => names.get(uid) || `${uid} (${_('missing')})`,
      )
      const actions = []

      if (this.writable) {
        if (item.type === 'config' && !selected) {
          actions.push(
            E(
              'button',
              {
                class: 'btn cbi-button cbi-button-action',
                click: ui.createHandlerFn(this, 'handleActivate', item.uid),
              },
              _('Activate'),
            ),
            ' ',
          )
        }

        actions.push(
          E(
            'button',
            {
              class: 'btn cbi-button cbi-button-negative',
              click: ui.createHandlerFn(this, 'handleDelete', item),
            },
            _('Delete'),
          ),
        )
      }

      return E('tr', { class: 'tr' }, [
        E('td', { class: 'td' }, [
          item.name,
          selected ? E('strong', {}, ` (${_('selected')})`) : '',
          E('div', {}, E('small', {}, api.text(item.uid))),
        ]),
        E('td', { class: 'td' }, api.text(profileKind(item))),
        E('td', { class: 'td' }, api.text(transforms.join(' → '))),
        E('td', { class: 'td cbi-section-actions' }, actions),
      ])
    })

    this.listBox.replaceChildren(
      E('table', { class: 'table' }, [
        E('tr', { class: 'tr table-titles' }, [
          E('th', { class: 'th' }, _('Name')),
          E('th', { class: 'th' }, _('Type')),
          E('th', { class: 'th' }, _('Transforms')),
          E('th', { class: 'th' }, ''),
        ]),
        ...rows,
      ]),
    )
  },

  /** The selected profile and the transforms that could still be attached. */
  selection(profileDocument) {
    const selected = profileDocument.items.find(
      (item) => item.uid === profileDocument.current,
    )

    return selected != null && selected.type === 'config' ? selected : null
  },

  paintTransforms() {
    if (!this.writable) {
      return
    }

    const profileDocument = this.profileDocument(this.transformsBox)

    if (profileDocument == null) {
      return
    }

    const selected = this.selection(profileDocument)

    if (selected == null) {
      this.transformsBox.replaceChildren(
        E('em', {}, _('Activate a config profile to arrange its transforms.')),
      )
      return
    }

    const attached = attachedTransforms(selected)
    const names = new Map(
      profileDocument.items.map((item) => [item.uid, item.name]),
    )
    const rows = attached.map((uid, index) => {
      const button = (label, enabled, handler, args) =>
        E(
          'button',
          {
            class: 'btn cbi-button',
            disabled: enabled ? null : '',
            click: ui.createHandlerFn(this, handler, ...args),
          },
          label,
        )

      return E('tr', { class: 'tr' }, [
        E('td', { class: 'td' }, String(index + 1)),
        E(
          'td',
          { class: 'td' },
          api.text(names.get(uid) || `${uid} (${_('missing')})`),
        ),
        E('td', { class: 'td cbi-section-actions' }, [
          button('↑', index > 0, 'handleMove', [selected, index, -1]),
          ' ',
          button('↓', index < attached.length - 1, 'handleMove', [
            selected,
            index,
            1,
          ]),
          ' ',
          button(_('Remove'), true, 'handleDetach', [selected, index]),
        ]),
      ])
    })

    const candidates = profileDocument.items.filter(
      (item) => item.type === 'transform' && !attached.includes(item.uid),
    )
    const picker = E(
      'select',
      { class: 'cbi-input-select' },
      candidates.map((item) =>
        E(
          'option',
          { value: item.uid },
          api.text(`${item.name} (${profileKind(item)})`),
        ),
      ),
    )

    this.transformsBox.replaceChildren(
      E('p', {}, [
        _('Selected profile: '),
        E('strong', {}, api.text(selected.name)),
      ]),
      attached.length === 0
        ? E('p', {}, E('em', {}, _('No transforms are attached.')))
        : E('table', { class: 'table' }, rows),
      candidates.length === 0
        ? E('p', {}, E('em', {}, _('There are no other transforms to attach.')))
        : E('div', { class: 'cbi-page-actions left' }, [
            picker,
            ' ',
            E(
              'button',
              {
                class: 'btn cbi-button cbi-button-add',
                click: ui.createHandlerFn(this, () =>
                  this.setTransforms(selected, attached.concat(picker.value)),
                ),
              },
              _('Attach at the end'),
            ),
          ]),
    )
  },

  renderImportForm() {
    this.importName = E('input', { type: 'text', class: 'cbi-input-text' })
    this.importYaml = textarea(12, '')

    const upload = E('input', {
      type: 'file',
      accept: '.yaml,.yml,text/yaml,text/plain',
      change: (ev) => {
        const file = ev.target.files[0]

        if (file == null) {
          return
        }

        if (this.importName.value === '') {
          this.importName.value = file.name.replace(/\.[^.]+$/, '')
        }

        return file.text().then((text) => {
          this.importYaml.value = text
        })
      },
    })

    return E('div', { class: 'cbi-section' }, [
      E('h3', {}, _('Import a YAML config')),
      field(_('Name'), this.importName),
      field(_('File'), upload),
      field(_('YAML'), this.importYaml),
      E('div', { class: 'cbi-page-actions left' }, [
        E(
          'button',
          {
            class: 'btn cbi-button cbi-button-add',
            click: ui.createHandlerFn(this, 'handleImport'),
          },
          _('Import'),
        ),
      ]),
    ])
  },

  renderTransformForm() {
    this.transformName = E('input', { type: 'text', class: 'cbi-input-text' })
    this.transformRuntime = E(
      'select',
      {
        class: 'cbi-input-select',
        change: () => this.applyTemplate(),
      },
      Object.keys(RUNTIME_LABELS).map((runtime) =>
        E('option', { value: runtime }, RUNTIME_LABELS[runtime]),
      ),
    )
    this.template = SCRIPT_TEMPLATES.javascript
    this.transformSource = textarea(10, this.template)

    return E('div', { class: 'cbi-section' }, [
      E('h3', {}, _('Create a script transform')),
      E(
        'div',
        { class: 'cbi-section-descr' },
        _(
          'The script receives the config and returns the changed config. Attach it to the selected profile above once created.',
        ),
      ),
      field(_('Name'), this.transformName),
      field(_('Language'), this.transformRuntime),
      field(_('Script'), this.transformSource),
      E('div', { class: 'cbi-page-actions left' }, [
        E(
          'button',
          {
            class: 'btn cbi-button cbi-button-add',
            click: ui.createHandlerFn(this, 'handleCreateTransform'),
          },
          _('Create'),
        ),
      ]),
    ])
  },

  /** Swaps the starter script for the other language unless it was edited. */
  applyTemplate() {
    if (this.transformSource.value === this.template) {
      this.template = SCRIPT_TEMPLATES[this.transformRuntime.value]
      this.transformSource.value = this.template
    }
  },

  reload() {
    return api.settle(api.call('get_profiles')).then((profiles) => {
      this.profiles = profiles
      this.paint()
    })
  },

  /** Reloads after a mutation, whether or not it committed. */
  afterMutation(committed) {
    return this.reload().then(() => committed)
  },

  handleImport() {
    const name = this.importName.value.trim()
    const text = this.importYaml.value

    if (name === '' || text.trim() === '') {
      ui.addNotification(
        null,
        E('p', {}, _('A name and the YAML are required.')),
        'warning',
      )
      return
    }

    return api
      .mutate(
        'create_profile',
        {
          request: api.newProfileRequest(name, api.configDefinition()),
          fileData: text,
        },
        _('Profile imported.'),
      )
      .then((committed) => {
        if (committed) {
          this.importName.value = ''
          this.importYaml.value = ''
        }

        return this.afterMutation(committed)
      })
  },

  handleCreateTransform() {
    const name = this.transformName.value.trim()
    const runtime = this.transformRuntime.value
    const text = this.transformSource.value

    if (name === '' || text.trim() === '') {
      ui.addNotification(
        null,
        E('p', {}, _('A name and the script are required.')),
        'warning',
      )
      return
    }

    return api
      .mutate(
        'create_profile',
        {
          request: api.newProfileRequest(name, api.scriptDefinition(runtime)),
          fileData: text,
        },
        _('Transform created.'),
      )
      .then((committed) => {
        if (committed) {
          this.transformName.value = ''
        }

        return this.afterMutation(committed)
      })
  },

  handleActivate(uid) {
    return api
      .mutate(
        'activate_profile',
        { uid },
        _('Profile selected. Use Start / apply on the Overview to run it.'),
      )
      .then((committed) => this.afterMutation(committed))
  },

  handleDelete(item) {
    return new Promise((resolve) => {
      ui.showModal(_('Delete profile'), [
        E(
          'p',
          {},
          api.text(_('Delete "%s"? This cannot be undone.').format(item.name)),
        ),
        E('div', { class: 'right' }, [
          E(
            'button',
            {
              class: 'btn',
              click: () => {
                ui.hideModal()
                resolve(false)
              },
            },
            _('Cancel'),
          ),
          ' ',
          E(
            'button',
            {
              class: 'btn cbi-button-negative important',
              click: () => {
                ui.hideModal()
                resolve(true)
              },
            },
            _('Delete'),
          ),
        ]),
      ])
    }).then((confirmed) => {
      if (!confirmed) {
        return false
      }

      return api
        .mutate('delete_profile', { uid: item.uid }, _('Profile deleted.'))
        .then((committed) => this.afterMutation(committed))
    })
  },

  /** Replaces the transform order of a config profile, keeping the rest of its definition. */
  setTransforms(item, transforms) {
    const definition = {
      type: 'config',
      config: Object.assign({}, item.config, { transforms }),
    }

    return api
      .mutate(
        'replace_profile_definition',
        { uid: item.uid, definition },
        _('Transforms updated.'),
      )
      .then((committed) => this.afterMutation(committed))
  },

  handleMove(item, index, offset) {
    const transforms = attachedTransforms(item).slice()
    const [moved] = transforms.splice(index, 1)

    transforms.splice(index + offset, 0, moved)

    return this.setTransforms(item, transforms)
  },

  handleDetach(item, index) {
    const transforms = attachedTransforms(item).slice()

    transforms.splice(index, 1)

    return this.setTransforms(item, transforms)
  },

  handleSave: null,
  handleSaveApply: null,
  handleReset: null,
})
