# OpenWrt packaging (POC)

Native LuCI integration for `nyanpasu-server`. LuCI never talks to the server
directly: the rpcd plugin runs `nyanpasu-cli call METHOD --params-file FILE`,
and the CLI talks to the server over `/var/run/nyanpasu/rpc.sock`. The design
and the method table are in
[the roadmap](../docs/design/openwrt-roadmap.md#poc-plan-2026-11-05).

## Layout

| Path                                                   | Installs to                        | Purpose                                                        |
| ------------------------------------------------------ | ---------------------------------- | -------------------------------------------------------------- |
| `nyanpasu/Makefile`, `nyanpasu/files/nyanpasu.init`    | `/usr/bin`, `/etc/init.d/nyanpasu` | Package for the server, the CLI and the procd init             |
| `luci-app-nyanpasu/root/usr/share/rpcd/ucode/nyanpasu` | `/usr/share/rpcd/ucode/nyanpasu`   | ubus object `nyanpasu`, one method per allowed command         |
| `luci-app-nyanpasu/root/usr/share/rpcd/acl.d/*.json`   | `/usr/share/rpcd/acl.d/`           | `read` and `write` ACL groups, explicit methods only           |
| `luci-app-nyanpasu/root/usr/share/luci/menu.d/*.json`  | `/usr/share/luci/menu.d/`          | Services → Nyanpasu                                            |
| `luci-app-nyanpasu/htdocs/luci-static/resources/`      | `/www/luci-static/resources/`      | `view/nyanpasu/{overview,profiles,logs}.js`, `nyanpasu/api.js` |

The two `Makefile`s follow the OpenWrt package and LuCI feed conventions but
have not been built.

## Installing by hand into a container

With `nyanpasu-server` and `nyanpasu-cli` already at `/usr/bin` (or the stub
CLI in place of the CLI), from the repository root and with `C` as the
container name:

```sh
cp() { wslc exec -i "$C" sh -c "mkdir -p \$(dirname $2) && cat > $2 && chmod $3 $2" < "$1"; }

L=openwrt/luci-app-nyanpasu
cp openwrt/nyanpasu/files/nyanpasu.init /etc/init.d/nyanpasu 755
cp $L/root/usr/share/rpcd/ucode/nyanpasu /usr/share/rpcd/ucode/nyanpasu 644
cp $L/root/usr/share/rpcd/acl.d/luci-app-nyanpasu.json /usr/share/rpcd/acl.d/luci-app-nyanpasu.json 644
cp $L/root/usr/share/luci/menu.d/luci-app-nyanpasu.json /usr/share/luci/menu.d/luci-app-nyanpasu.json 644
for f in nyanpasu/api.js view/nyanpasu/overview.js view/nyanpasu/profiles.js view/nyanpasu/logs.js; do
  cp $L/htdocs/luci-static/resources/$f /www/luci-static/resources/$f 644
done

wslc exec "$C" sh -c '/etc/init.d/rpcd restart && /etc/init.d/nyanpasu enable && /etc/init.d/nyanpasu start'
```

Then open Services → Nyanpasu in LuCI. Log out and in again if the menu entry
is missing. A login that should be read-only needs `list read luci-app-nyanpasu`
and no `write` entry for it in `/etc/config/rpcd`.
