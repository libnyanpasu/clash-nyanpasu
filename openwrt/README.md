# OpenWrt packaging (POC)

The Nyanpasu WebUI on a router. `nyanpasu-server` serves the React WebUI and
its HTTP API on its own port (`/etc/config/nyanpasu`); `luci-app-nyanpasu` is
a minimal page (Services → Nyanpasu): the service status from procd, Start,
Stop, Restart, Start at boot (the standard `rc` ubus object, no API of our own)
and Open WebUI. Open WebUI opens the WebUI in a new window: LuCI asks the rpcd
method `webui_ticket` (write ACL) for a one-time ticket, through `nyanpasu-cli`
and the local socket `/var/run/nyanpasu/rpc.sock`, and sends the new window to
the WebUI with it. A login without write access sees the status only. The
design and the security notes are in
[the roadmap](../docs/design/openwrt-roadmap.md#webui-link-2026-11-05).

## Layout

| Path                                                   | Installs to                        | Purpose                                                                             |
| ------------------------------------------------------ | ---------------------------------- | ----------------------------------------------------------------------------------- |
| `nyanpasu/Makefile`, `nyanpasu/files/nyanpasu.init`    | `/usr/bin`, `/etc/init.d/nyanpasu` | Package for the server, the CLI and the procd init                                  |
| `nyanpasu/files/nyanpasu.config`                       | `/etc/config/nyanpasu`             | UCI: `webui_listen` (empty turns it off), `webui_dir`, `window_mode`                |
| `luci-app-nyanpasu/root/usr/share/rpcd/ucode/nyanpasu` | `/usr/share/rpcd/ucode/nyanpasu`   | ubus object `nyanpasu` with the one method `webui_ticket`                           |
| `luci-app-nyanpasu/root/usr/share/rpcd/acl.d/*.json`   | `/usr/share/rpcd/acl.d/`           | `rc` list/init, `uci` commit and `webui_ticket` for `write`; `read` sees the status |
| `luci-app-nyanpasu/root/usr/share/luci/menu.d/*.json`  | `/usr/share/luci/menu.d/`          | Services → Nyanpasu, one view                                                       |
| `luci-app-nyanpasu/htdocs/luci-static/resources/`      | `/www/luci-static/resources/`      | `view/nyanpasu/service.js`, the service page                                        |

The built WebUI (`pnpm web:build`, output `backend/tauri/tmp/dist`) is installed
at `webui_dir`, by default `/usr/share/nyanpasu/webui`; no package carries it
yet. The two `Makefile`s follow the OpenWrt package and LuCI feed conventions
but have not been built.

The WebUI port listens on all interfaces, in plain HTTP, and is open on the
LAN; the default firewall's WAN input reject keeps it off the WAN. Open WebUI
works from an HTTPS LuCI too, since the WebUI is a window of its own. The UCI
option `window_mode` (`tab`, the default, or `popup`) chooses whether it opens
as a new tab or as a simplified popup window; the page sets it without
restarting the service.

## Installing by hand into a container

With `nyanpasu-server` and `nyanpasu-cli` already at `/usr/bin`, the built
WebUI at `/usr/share/nyanpasu/webui`, from the repository root and with `C` as
the container name:

```sh
cp() { wslc exec -i "$C" sh -c "mkdir -p \$(dirname $2) && cat > $2 && chmod $3 $2" < "$1"; }

L=openwrt/luci-app-nyanpasu
cp openwrt/nyanpasu/files/nyanpasu.init /etc/init.d/nyanpasu 755
cp openwrt/nyanpasu/files/nyanpasu.config /etc/config/nyanpasu 644
cp $L/root/usr/share/rpcd/ucode/nyanpasu /usr/share/rpcd/ucode/nyanpasu 644
cp $L/root/usr/share/rpcd/acl.d/luci-app-nyanpasu.json /usr/share/rpcd/acl.d/luci-app-nyanpasu.json 644
cp $L/root/usr/share/luci/menu.d/luci-app-nyanpasu.json /usr/share/luci/menu.d/luci-app-nyanpasu.json 644
cp $L/htdocs/luci-static/resources/view/nyanpasu/service.js /www/luci-static/resources/view/nyanpasu/service.js 644

wslc exec "$C" sh -c '/etc/init.d/rpcd restart && rm -rf /tmp/luci-* && /etc/init.d/nyanpasu enable && /etc/init.d/nyanpasu restart'
```

Publish the WebUI port when the container is created (`-p 7899:7899` next to
the LuCI port). Then open Services → Nyanpasu in LuCI. Log out and in again if
the menu entry is missing. A login that should only see the status needs
`list read luci-app-nyanpasu` and no `write` entry for it in `/etc/config/rpcd`.
