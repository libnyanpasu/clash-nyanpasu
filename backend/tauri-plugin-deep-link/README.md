# Deep link plugin for Tauri

[![](https://img.shields.io/crates/v/tauri-plugin-deep-link.svg)](https://crates.io/crates/tauri-plugin-deep-link) [![](https://img.shields.io/docsrs/tauri-plugin-deep-link)](https://docs.rs/tauri-plugin-deep-link)

**This plugin will be migrated to https://github.com/tauri-apps/plugins-workspace/.** `0.1.2` will be the last release in this repo.

~~Temporary solution until https://github.com/tauri-apps/tauri/issues/323 lands.~~

Depending on your use case, for example a `Login with Google` button, you may want to take a look at https://github.com/FabianLars/tauri-plugin-oauth instead. It uses a minimalistic localhost server for the OAuth process instead of custom uri schemes because some oauth providers, like the aforementioned Google, require this setup. Personally, I think it's easier to use too.

Check out the [`example/`](https://github.com/FabianLars/tauri-plugin-deep-link/tree/main/example) directory for a minimal example. You must copy it into an actual tauri app first!

## macOS

In case you're one of the very few people that didn't know this already: macOS hates developers! Not only is that why the macOS implementation took me so long, it also means _you_ have to be a bit more careful if your app targets macOS:

- Read through the methods' platform-specific notes.
- On macOS you need to register the schemes in a `Info.plist` file at build time, the plugin can't change the schemes at runtime.
- macOS apps are in single-instance by default so this plugin will not manually shut down secondary instances in release mode.
  - To make development via `tauri dev` a little bit more pleasant, the plugin will work similar-ish to Linux and Windows _in debug mode_ but you will see secondary instances show on the screen for a split second and the event will trigger twice in the primary instance (one of these events will be an empty string). You still have to install a `.app` bundle you got from `tauri build --debug` for this to work!
