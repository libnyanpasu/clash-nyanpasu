<p align="center">
  <img src="https://nyanpasu.org/images/banner/nyanpasu_banner.png" alt="Clash Nyanpasu" />
</p>

<h1 align="center">Clash Nyanpasu</h1>

<p align="center">
  A multi-core proxy manager with traffic insights and a Material You interface.
</p>

<p align="center">
  <a href="https://github.com/libnyanpasu/clash-nyanpasu/releases/latest">Download</a> ·
  <a href="https://nyanpasu.org">Documentation</a> ·
  <a href="https://github.com/libnyanpasu/clash-nyanpasu/releases/pre-release">Nightly builds</a> ·
  <a href="https://github.com/libnyanpasu/clash-nyanpasu/issues/new/choose">Report an issue</a>
</p>

<p align="center">
  <a href="https://github.com/libnyanpasu/clash-nyanpasu/releases/latest">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://nyanpasu.org/readme/badges/release-dark.svg" />
      <img src="https://nyanpasu.org/readme/badges/release-light.svg" alt="Latest release" height="32" />
    </picture>
  </a>
  <a href="https://github.com/libnyanpasu/clash-nyanpasu/actions/workflows/ci.yml">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://nyanpasu.org/readme/badges/build-dark.svg" />
      <img src="https://nyanpasu.org/readme/badges/build-light.svg" alt="Build status" height="32" />
    </picture>
  </a>
  <a href="https://github.com/libnyanpasu/clash-nyanpasu/stargazers">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://nyanpasu.org/readme/badges/stars-dark.svg" />
      <img src="https://nyanpasu.org/readme/badges/stars-light.svg" alt="GitHub stars" height="32" />
    </picture>
  </a>
  <a href="https://github.com/libnyanpasu/clash-nyanpasu/releases">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://nyanpasu.org/readme/badges/downloads-dark.svg" />
      <img src="https://nyanpasu.org/readme/badges/downloads-light.svg" alt="Total release downloads" height="32" />
    </picture>
  </a>
  <a href="LICENSE">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://nyanpasu.org/readme/badges/license-dark.svg" />
      <img src="https://nyanpasu.org/readme/badges/license-light.svg" alt="License" height="32" />
    </picture>
  </a>
  <a href="https://github.com/libnyanpasu/clash-nyanpasu/issues">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://nyanpasu.org/readme/badges/issues-dark.svg" />
      <img src="https://nyanpasu.org/readme/badges/issues-light.svg" alt="Open issues" height="32" />
    </picture>
  </a>
</p>

<p align="center">
  <a href="https://x.com/ClashNyanpasu">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://nyanpasu.org/readme/badges/follow-dark.svg" />
      <img src="https://nyanpasu.org/readme/badges/follow-light.svg" alt="Follow @ClashNyanpasu on X" height="32" />
    </picture>
  </a>
  <a href="https://nyanpasu.org">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://nyanpasu.org/readme/badges/docs-dark.svg" />
      <img src="https://nyanpasu.org/readme/badges/docs-light.svg" alt="Official documentation" height="32" />
    </picture>
  </a>
  <a href="https://t.me/ClashNyanpasu">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://nyanpasu.org/readme/badges/telegram-channel-dark.svg" />
      <img src="https://nyanpasu.org/readme/badges/telegram-channel-light.svg" alt="Telegram channel" height="32" />
    </picture>
  </a>
  <a href="https://t.me/+IGNnkH5k7WE3MzA1">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://nyanpasu.org/readme/badges/telegram-group-dark.svg" />
      <img src="https://nyanpasu.org/readme/badges/telegram-group-light.svg" alt="Telegram group" height="32" />
    </picture>
  </a>
  <a href="https://deepwiki.com/libnyanpasu/clash-nyanpasu">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://nyanpasu.org/readme/badges/deepwiki-dark.svg" />
      <img src="https://nyanpasu.org/readme/badges/deepwiki-light.svg" alt="Ask DeepWiki" height="32" />
    </picture>
  </a>
</p>

Clash Nyanpasu brings proxy core management, profile enhancement, and traffic
analysis into one application for Windows, macOS, and Linux. Manage your
configuration, switch proxy groups, inspect connections, and explore where your
traffic goes through a customizable Material You interface.

## Preview

<picture>
  <source media="(prefers-color-scheme: dark)" type="image/avif" srcset="https://nyanpasu.org/readme/material-dark@60f.avif" />
  <source media="(prefers-color-scheme: light)" type="image/avif" srcset="https://nyanpasu.org/readme/material-light@60f.avif" />
  <source media="(prefers-color-scheme: dark)" type="image/webp" srcset="https://nyanpasu.org/readme/material-dark@60f-q90.webp" />
  <img src="https://nyanpasu.org/readme/material-light@60f-q90.webp" alt="Clash Nyanpasu preview" width="100%" />
</picture>

## Features

- **Multiple proxy cores.** Manage Mihomo, Clash Rust, Meow, and Clash Premium,
  with separate stable and Alpha choices for Mihomo, Clash Rust, and Meow.
- **Profile management.** Organize local profiles and subscriptions.
- **Configuration enhancement.** Merge and transform profiles with YAML,
  JavaScript, and Lua.
- **Provider management.** Manage proxy providers and rule providers.
- **Connection inspection.** Browse active and closed connections with process,
  destination, and routing information.
- **Traffic analysis.** Review traffic history and usage by process, target,
  rule, and exit through rankings, connection paths, and geographic views.
- **Customizable dashboard.** Arrange widgets for traffic, connections, proxy
  shortcuts, profiles, and providers to keep the information you need in view.
- **Proxy controls.** Manage system proxy and TUN mode from the application.
- **Material You interface.** Enjoy configurable themes,
  light and dark modes, and animated navigation.

Core capabilities vary. The selected core determines which protocols and runtime
features are available.

## Get started

1. Download a package for your platform from
   [GitHub Releases](https://github.com/libnyanpasu/clash-nyanpasu/releases/latest).
   See the [installation guide](https://nyanpasu.org/tutorial/install) for details.
2. Import a local profile or add a subscription, then select a compatible core.
3. Choose your proxy group and enable system proxy or TUN mode as needed.

For development builds, visit
[Nightly releases](https://github.com/libnyanpasu/clash-nyanpasu/releases/pre-release).

## Documentation and support

- [Documentation](https://nyanpasu.org)
- [Profile enhancement](https://nyanpasu.org/tutorial/proxy-chain)
- [Frequently asked questions](https://nyanpasu.org/others/faq)
- [Issue guidelines](https://nyanpasu.org/others/issues)
- [How to ask a useful question](https://nyanpasu.org/others/how-to-ask)
- [Follow on X](https://x.com/ClashNyanpasu)
- [Telegram channel](https://t.me/ClashNyanpasu)
- [Telegram group](https://t.me/+IGNnkH5k7WE3MzA1)
- [Explore the code with DeepWiki](https://deepwiki.com/libnyanpasu/clash-nyanpasu)

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for platform prerequisites and the full
development setup. Read the [development standards](docs/development/README.md)
before changing the code.

After installing the prerequisites:

```sh
git submodule update --init --recursive
pnpm install
deno task prepare:check
pnpm dev
```

The desktop application uses Rust and Tauri with a React and TypeScript frontend.
Shared backend capabilities are being extracted into `nyanpasu-core` to support
additional frontends. See the
[architecture guide](docs/development/architecture.md#core-and-frontend-separation)
for the current separation work.

## Contributing

Bug reports, translations, documentation improvements, and pull requests are
welcome. Please search existing issues before opening a new one, then use the
[issue templates](https://github.com/libnyanpasu/clash-nyanpasu/issues/new/choose)
and include the steps and logs needed to understand the problem.

For code contributions, follow [CONTRIBUTING.md](CONTRIBUTING.md).

## Acknowledgements

Clash Nyanpasu builds on the work of the open-source community, including:

- [Clash Verge](https://github.com/zzzgydi/clash-verge) and
  [Clash Verge Rev](https://github.com/clash-verge-rev/clash-verge-rev), for the
  project's foundations, inspiration, and fixes.
- [Clash](https://github.com/Dreamacro/clash),
  [Mihomo](https://github.com/MetaCubeX/mihomo),
  [Clash Rust](https://github.com/ibigbug/clash-rs), and
  [Meow](https://github.com/meow-rs/meow-rs), for their proxy cores.
- [Clash for Windows](https://github.com/Fndroid/clash_for_windows_pkg), for
  inspiration.
- [Tauri](https://github.com/tauri-apps/tauri),
  [React](https://github.com/facebook/react), and
  [Vite](https://github.com/vitejs/vite), for the application stack.
- [TanStack](https://tanstack.com), for routing and application data fetching.
- [Radix UI](https://www.radix-ui.com), for accessible UI primitives.

## Contributors

<a href="https://github.com/libnyanpasu/clash-nyanpasu/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=libnyanpasu/clash-nyanpasu" alt="Clash Nyanpasu contributors" />
</a>

## License

Licensed under [GPL-3.0](LICENSE).
