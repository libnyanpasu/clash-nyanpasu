<h1 align="center">
  <img src="./frontend/nyanpasu/src/assets/image/logo.png" alt="Clash" width="128" />
  <br>
  Clash Nyanpasu
  <br>
</h1>

<h3 align="center">
A <a href="https://github.com/Dreamacro/clash">Clash</a> GUI based on <a href="https://github.com/tauri-apps/tauri">tauri</a>.
</h3>

<p align="center">
  <a href="https://github.com/LibNyanpasu/clash-nyanpasu/releases/latest"><img src="https://img.shields.io/github/v/release/LibNyanpasu/clash-nyanpasu?style=flat-square" alt="Nyanpasu Release" /></a>
  <a href="https://github.com/LibNyanpasu/clash-nyanpasu/releases/pre-release"><img src="https://img.shields.io/github/actions/workflow/status/LibNyanpasu/clash-nyanpasu/dev.yaml?style=flat-square" alt="Dev Build Status" /></a>
  <a href="https://github.com/LibNyanpasu/clash-nyanpasu/stargazers"><img src="https://img.shields.io/github/stars/LibNyanpasu/clash-nyanpasu?style=flat-square" alt="Nyanpasu stars" /></a>
  <a href="https://github.com/LibNyanpasu/clash-nyanpasu/releases/latest"><img src="https://img.shields.io/github/downloads/LibNyanpasu/clash-nyanpasu/total?style=flat-square" alt="GitHub Downloads (all assets, all releases)" /></a>
  <a href="https://github.com/LibNyanpasu/clash-nyanpasu/blob/main/LICENSE"><img src="https://img.shields.io/github/license/LibNyanpasu/clash-nyanpasu?style=flat-square" alt="Nyanpasu License" /></a>
</p>

<p align="center">
  <a href="https://twitter.com/ClashNyanpasu"><img src="https://img.shields.io/twitter/follow/ClashNyanpasu?style=flat-square" alt="Nyanpasu Twitter" /></a>
</p>

## Features

- Full `clash` config supported, Partial `clash premium` config supported.
- Built-in support [Clash.Meta](https://github.com/MetaCubeX/mihomo) core & [ClashRs](https://github.com/Watfaq/clash-rs) core.
- Profiles management and enhancement (by yaml and Javascript). [Doc](https://nyanpasu.elaina.moe/tutorial/proxy-chain.html)
- Material You Design UI and amimation support.
- System proxy setting and guard.

## Preview

![preview](./docs/preview.gif)

## Links

- [Install](https://nyanpasu.elaina.moe/tutorial/install.html)
- [FAQ](https://nyanpasu.elaina.moe/others/faq.html)
- [Q&A Convention](https://nyanpasu.elaina.moe/others/issues.html)
- [How To Ask Questions](https://nyanpasu.elaina.moe/others/how-to-ask.html)

## Development

### Configure your development environment

You should install Rust and Nodejs, see [here](https://tauri.app/v1/guides/getting-started/prerequisites) for more details.

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

- [zzzgydi/clash-verge](https://github.com/zzzgydi/clash-verge): A Clash GUI based on tauri. Supports Windows, macOS and Linux.
- [clash-verge-rev/clash-verge-rev](https://github.com/clash-verge-rev/clash-verge-rev): Another fork of Clash Verge. Some patches are included for bug fixes.
- [tauri-apps/tauri](https://github.com/tauri-apps/tauri): Build smaller, faster, and more secure desktop applications with a web frontend.
- [Dreamacro/clash](https://github.com/Dreamacro/clash): A rule-based tunnel in Go.
- [MetaCubeX/Clash.Meta](https://github.com/MetaCubeX/mihomo): A rule-based tunnel in Go.
- [ClashRs](https://github.com/Watfaq/clash-rs): A custom protocol, rule based network proxy software.
- [Fndroid/clash_for_windows_pkg](https://github.com/Fndroid/clash_for_windows_pkg): A Windows/macOS GUI based on Clash.
- [vitejs/vite](https://github.com/vitejs/vite): Next generation frontend tooling. It's fast!
- [mui/material-ui](https://github.com/mui/material-ui): Ready-to-use foundational React components, free forever.

## Contributors

![Contributors](https://contrib.rocks/image?repo=LibNyanpasu/clash-nyanpasu)

## License

GPL-3.0 License. See [License here](./LICENSE) for details.
