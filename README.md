<h1 align="center">
  <img src="https://nyanpasu.elaina.moe/images/banner/nyanpasu_banner.png" alt="Clash Nyanpasu Banner" />
</h1>

<h3>Clash Nyanpasu</h3>

<h3>
  A <a href="https://github.com/Dreamacro/clash">Clash</a> GUI based on <a href="https://github.com/tauri-apps/tauri">Tauri</a>.
</h3>

<p>
  <a href="https://github.com/libnyanpasu/clash-nyanpasu/releases/latest"><img src="https://img.shields.io/github/v/release/libnyanpasu/clash-nyanpasu?style=flat-square" alt="Nyanpasu Release" /></a>
  <a href="https://github.com/libnyanpasu/clash-nyanpasu/releases/pre-release"><img src="https://img.shields.io/github/actions/workflow/status/libnyanpasu/clash-nyanpasu/target-dev-build.yaml?style=flat-square" alt="Dev Build Status" /></a>
  <a href="https://github.com/libnyanpasu/clash-nyanpasu/stargazers"><img src="https://img.shields.io/github/stars/libnyanpasu/clash-nyanpasu?style=flat-square" alt="Nyanpasu Stars" /></a>
  <a href="https://github.com/libnyanpasu/clash-nyanpasu/releases/latest"><img src="https://img.shields.io/github/downloads/libnyanpasu/clash-nyanpasu/total?style=flat-square" alt="GitHub Downloads (all assets, all releases)" /></a>
  <a href="https://github.com/libnyanpasu/clash-nyanpasu/blob/main/LICENSE"><img src="https://img.shields.io/github/license/libnyanpasu/clash-nyanpasu?style=flat-square" alt="Nyanpasu License" /></a>
  <a href="https://twitter.com/ClashNyanpasu"><img src="https://img.shields.io/twitter/follow/ClashNyanpasu?style=flat-square" alt="Nyanpasu Twitter" /></a>
  <a href="https://deepwiki.com/libnyanpasu/clash-nyanpasu"><img src="https://deepwiki.com/badge.svg" alt="Ask DeepWiki"></a>
</p>

## Features

- Built-in support [Clash Premium](https://github.com/Dreamacro/clash), [Mihomo](https://github.com/MetaCubeX/mihomo) & [Clash Rust](https://github.com/Watfaq/clash-rs).
- Profiles management and enhancement (by YAML, JavaScript & Lua). [Doc](https://nyanpasu.elaina.moe/tutorial/proxy-chain)
- Provider management support.
- Google Material You Design UI and animation support.

## Preview

![preview-light](https://nyanpasu.elaina.moe/images/screenshot/app-dashboard-light.png)

![preview-dark](https://nyanpasu.elaina.moe/images/screenshot/app-dashboard-dark.png)

## Links

- [Install](https://nyanpasu.elaina.moe/tutorial/install)
- [FAQ](https://nyanpasu.elaina.moe/others/faq)
- [Q&A Convention](https://nyanpasu.elaina.moe/others/issues)
- [How To Ask Questions](https://nyanpasu.elaina.moe/others/how-to-ask)

## Development

### Configure your development environment

You should install Rust and Node.js, see [here](https://v2.tauri.app/start/prerequisites/) for more details.

Clash Nyanpasu uses the pnpm package manager. See [here](https://pnpm.io/installation) for installation instructions. Then, install Node.js packages.

```shell
pnpm i
```

### Download the Clash binary & other dependencies

```shell
# force update to latest version
# pnpm check --force

pnpm check
```

### Run dev

```shell
pnpm dev

# run it in another way if app instance exists
pnpm dev:diff
```

### Build application

```shell
pnpm build
```

## Contributions

Issue and PR welcome!

## Acknowledgement

Clash Nyanpasu was based on or inspired by these projects and so on:

- [zzzgydi/clash-verge](https://github.com/zzzgydi/clash-verge): A Clash GUI based on Tauri. Supports Windows, macOS and Linux.
- [clash-verge-rev/clash-verge-rev](https://github.com/clash-verge-rev/clash-verge-rev): Another fork of Clash Verge. Some patches are included for bug fixes.
- [tauri-apps/tauri](https://github.com/tauri-apps/tauri): Build smaller, faster, and more secure desktop applications with a web frontend.
- [Dreamacro/clash](https://github.com/Dreamacro/clash): A rule-based tunnel in Go.
- [MetaCubeX/mihomo](https://github.com/MetaCubeX/mihomo): A rule-based tunnel in Go.
- [Watfaq/clash-rs](https://github.com/Watfaq/clash-rs): A custom protocol, rule based network proxy software.
- [Fndroid/clash_for_windows_pkg](https://github.com/Fndroid/clash_for_windows_pkg): A Windows/macOS GUI based on Clash.
- [vitejs/vite](https://github.com/vitejs/vite): Next generation frontend tooling. It's fast!
- [mui/material-ui](https://github.com/mui/material-ui): Ready-to-use foundational React components, free forever.

## Contributors

![Contributors](https://contrib.rocks/image?repo=libnyanpasu/clash-nyanpasu)

## License

GPL-3.0 License. See [License here](./LICENSE) for details.
