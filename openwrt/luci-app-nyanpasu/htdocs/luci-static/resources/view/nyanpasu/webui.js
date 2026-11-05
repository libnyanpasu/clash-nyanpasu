'use strict'
'require view'
'require rpc'

/**
 * Services -> Nyanpasu: the Nyanpasu WebUI, framed. LuCI is the menu entry,
 * the login gate and the frame; the WebUI itself is served by nyanpasu-server
 * on its own port and talks to the server's HTTP API directly.
 *
 * Each load asks the rpcd method `webui_ticket` (write ACL) for a one-time
 * ticket and points the frame at `/?ticket=...` on the WebUI's port, on the
 * hostname LuCI was reached by. The server turns the ticket into a session
 * cookie for the frame. The reply is
 *
 *   { ok: true,  result: { path, port } }
 *   { ok: false, error }                    an RpcError from the server
 *   { ok: false, transport: true, error }   the CLI could not reach the server
 */

const callWebuiTicket = rpc.declare({
  object: 'nyanpasu',
  method: 'webui_ticket',
})

// The space left below the frame: the page's bottom padding.
const BOTTOM_GAP_PX = 16

function message(text, hint) {
  return E('div', { class: 'cbi-section' }, [
    E('p', {}, text),
    hint != null ? E('p', { class: 'cbi-section-descr' }, hint) : '',
  ])
}

function fit(frame) {
  const top = frame.getBoundingClientRect().top

  frame.style.height = `${Math.max(window.innerHeight - top - BOTTOM_GAP_PX, 320)}px`
}

function frameFor(ticket) {
  const frame = E('iframe', {
    title: _('Nyanpasu WebUI'),
    src: `${location.protocol}//${location.hostname}:${ticket.port}${ticket.path}`,
    style: 'display: block; width: 100%; border: 0',
  })
  const refit = () => fit(frame)

  window.addEventListener('resize', refit)
  // The frame is laid out only after the view is attached to the page.
  requestAnimationFrame(refit)

  return frame
}

return view.extend({
  load() {
    // A refused or failed call is rendered, not thrown, so the page keeps
    // its menu and says what happened.
    return callWebuiTicket().then(
      (reply) => ({ reply }),
      (error) => ({ failed: error }),
    )
  },

  render({ reply, failed }) {
    if (failed != null) {
      // ubus answers a call outside the login's ACL with "Access denied".
      if (/denied/i.test(String(failed.message ?? failed))) {
        return message(
          _('Opening the Nyanpasu WebUI needs write access to Nyanpasu.'),
          _('Your login is read-only. Sign in as a user with write access.'),
        )
      }

      return message(
        `${_('The Nyanpasu WebUI is not available:')} ${failed.message ?? failed}`,
      )
    }

    if (reply?.ok !== true) {
      const detail = reply?.error?.message ?? _('The server did not answer.')

      return message(
        `${_('The Nyanpasu WebUI is not available:')} ${detail}`,
        _(
          'Check that the nyanpasu service is running and that webui_listen is set in /etc/config/nyanpasu.',
        ),
      )
    }

    const frame = frameFor(reply.result)

    return location.protocol === 'https:'
      ? E('div', {}, [
          E(
            'p',
            { class: 'cbi-section-descr' },
            _(
              'The WebUI is served over plain HTTP; a browser blocks it inside an HTTPS page. Open LuCI over HTTP to use it.',
            ),
          ),
          frame,
        ])
      : frame
  },

  handleSave: null,
  handleSaveApply: null,
  handleReset: null,
})
