## [1.6.1] - 2024-09-07

### ✨ Features

- **dock:** Try to setup macos dock handler by @greenhat616

- **enhance:** Finish all filter test suites by @greenhat616

- **enhance:** Add sequence filter support and partial test suite by @greenhat616

- **enhance:** Add complex filter syntax support by @greenhat616

- **monaco:** Add onValidation before submit, and close #1491 by @greenhat616

- **monaco:** Add yaml config prompt by @greenhat616

- **nsis:** Cleanup reg while uninstall by @greenhat616

- **service:** Add manual prompt for service uninstall, stop, start by @greenhat616

- **service:** Add a manual install prompt while service install failed by @greenhat616

- **tun:** Support auto-route while clash-rs support it by @greenhat616

- Use cross-rs to build aarch64 by @greenhat616

- Try to support linux aarch64 build by @greenhat616

### 🐛 Bug Fixes

- **ci:** Update publish script by @greenhat616

- **dialog:** Position func err by @keiko233

- **nsis:** Cleanup app config and data dir if option is selected by @greenhat616

- **os:** Create no window by @greenhat616

- **shiki:** Shell lang loader by @greenhat616

- Monaco clash config prompt by @greenhat616

- Monaco url resolve issue by @greenhat616

- Try to resolve the yaml schema by @greenhat616

- Try to escape the string by @greenhat616

- Add service install error prompt by @greenhat616

- Shiki import by @greenhat616

- Try to fix create no window by @greenhat616

- Typo by @greenhat616

- Windows nightly build version issue by @greenhat616

- Build by @greenhat616

- Aarch build by @greenhat616

- Dont merge falsy theme settings by @greenhat616

### 🔨 Refactor

- Use @monaco-editor/react instead by @greenhat616

- Service shoutcuts use core manager internal state by @greenhat616

---

**Full Changelog**: https://github.com/libnyanpasu/clash-nyanpasu/compare/v1.6.0...v1.6.1

## [1.6.0] - 2024-08-29

### 💥 Breaking Changes

- Tsconfig options by @keiko233

### ⚡ Performance Improvements

- **hook:** Add debounce callback & do nothing when minimized by @keiko233

- **proxies:** Add useTransition by @keiko233

- **ui:** Memoized children node by @keiko233

- **ui:** Add ref support for BasePage by @keiko233

- Switch log page & rule page to async component by @keiko233

### ✨ Features

- **component:** Add children props support for PaperButton by @keiko233

- **connections:** Lazy load connections and close #1208 by @greenhat616

- **connections:** Add no connection display by @keiko233

- **connections:** New design for ConnectionsPage by @keiko233

- **custom-schema:** Experimental compatible with common clash schema by @greenhat616

- **custom-scheme:** Use one desktop file to process mime by @greenhat616

- **custom-theme:** Background color picker minor tweak by @keiko233

- **dashboard:** Add service status shortcuts card by @keiko233

- **dashboard:** Add proxy shortcuts panel by @keiko233

- **dashboard:** Special grid layout for drawer by @keiko233

- **dashboard:** Add health panel by @keiko233

- **dashboard:** Init Dashboard Page by @keiko233

- **delay-button:** Minor tweaks for animetion by @keiko233

- **downloader:** Make downloader status readable by @greenhat616

- **drawer:** Enable panel collapsible by @keiko233

- **drawer:** Add small size layout by @keiko233

- **drawer:** Minor tweak for small size by @keiko233

- **enhance:** Experimental add lua runner support by @greenhat616

- **enhance:** Make merge process more powerful by @greenhat616

- **experimental:** Initial react compiler support by @keiko233

- **interface:** Initial ClashWS by @keiko233

- **interface:** Add profile js interface by @keiko233

- **interface:** Add current clash mode interface by @keiko233

- **interface:** Add useClashCore hook method by @keiko233

- **interface:** Add app tauri invoke interface by @keiko233

- **interface:** Add profiles api with SWR by @keiko233

- **interface:** Add ClashInfo interface with SWR by @keiko233

- **interface:** Init code by @keiko233

- **ipc:** Replace timing utils ofetch to tokio by @keiko233

- **ipc:** Export delay test and core status call by @greenhat616

- **layout:** Add scrollbar track margin by @keiko233

- **logs:** New design LogsPage by @keiko233

- **macos:** Try to impl dock show/hide api by @greenhat616

- **macos:** Add traffic control offset for macos by @keiko233

- **migration:** Add discard method for discarding changes while migration failed by @greenhat616

- **monaco:** Add monaco types support by @keiko233

- **monaco:** Add typescript language service by @keiko233

- **monaco:** Import lua language support by @keiko233

- **monaco-edit:** Switch to lazy load module by @keiko233

- **monaco-editor:** Support props value changes and language switching by @keiko233

- **monaco-editor:** Support language change on prop by @keiko233

- **motion:** Add lighten animation effects config by @keiko233

- **nyanpasu:** Node list support proxy delay testing by @keiko233

- **nyanpasu:** Import react devtools on dev env by @keiko233

- **nyanpasu:** Use new design Proxies Page by @keiko233

- **nyanpasu:** Import tailwind css by @keiko233

- **nyanpasu:** Experimentally added new settings interface by @keiko233

- **nyanpasu:** Add SettingLegacy component by @keiko233

- **nyanpasu:** Add SettingNyanpasuVersion component by @keiko233

- **nyanpasu:** Add SettingNyanpasuUI component by @keiko233

- **nyanpasu:** Add SettingNyanpasuPath component by @keiko233

- **nyanpasu:** Add SettingNyanpasuPath component by @keiko233

- **nyanpasu:** Add PaperButton component by @keiko233

- **nyanpasu:** Add SettingNyanpasuTasks component by @keiko233

- **nyanpasu:** Add SettingSystemService component by @keiko233

- **nyanpasu:** Add SettingSystemBehavior component by @keiko233

- **nyanpasu:** Add SettingSystemClash component by @keiko233

- **nyanpasu:** Add SettingClashCore component by @keiko233

- **nyanpasu:** Use grid layout for SettingClashWeb by @keiko233

- **nyanpasu:** Add SettingClashField component by @keiko233

- **nyanpasu:** Add SettingClashWeb component by @keiko233

- **nyanpasu:** Add SettingClashExternal component by @keiko233

- **nyanpasu:** Add SettingClashPort component by @keiko233

- **nyanpasu:** Add SettingClashBase component by @keiko233

- **nyanpasu:** Add nyanpasu setting props creator by @keiko233

- **nyanpasu:** Use new theme create method by @keiko233

- **nynapasu:** Add SettingNyanpasuMisc component by @keiko233

- **profiles:** Adapting scroll area & add position animation by @keiko233

- **profiles:** Add diff dialog hint by @greenhat616

- **profiles:** Add max log level triggered notice, and close #1291 by @greenhat616

- **profiles:** Add black touch new option by @greenhat616

- **profiles:** Add text carousel for subscription expires and updated time by @greenhat616

- **profiles:** Minor tweaks & add click card to apply profile by @keiko233

- **profiles:** Add split pane support & minor tweaks by @keiko233

- **profiles:** Profiles new design by @keiko233

- **profiles:** Add proxy chain side page by @keiko233

- **profiles:** Add monaco editor for ProfileItem by @keiko233

- **profiles:** Complete profile operation menu by @keiko233

- **profiles:** Redesign profile cards & new profile editor by @keiko233

- **profiles:** Profile dialog support edit mode by @keiko233

- **profiles:** Add QuickImport text arae component by @keiko233

- **profiles:** Init new profile page by @keiko233

- **providers:** Add proxy provider traffic display support by @keiko233

- **providers:** Support proxies providers by @keiko233

- **providers:** New design ProvidersPage by @keiko233

- **proxies:** Filter proxies nodes by @greenhat616

- **proxies:** Adapting scroll area by @keiko233

- **proxies:** Support proxy group test url by @keiko233

- **proxies:** Add scroll to current node button by @keiko233

- **proxies:** Add node card animation by @keiko233

- **proxies:** Group name transition use framer motion by @keiko233

- **proxies:** Add none proxies tips by @keiko233

- **proxies:** Add virtual scrolling to grid node list by @keiko233

- **proxies:** Group list use virtual scrolling by @keiko233

- **proxies:** Add node list sorting function by @keiko233

- **proxies:** Add group name text transition by @keiko233

- **proxies:** Add diff clash mode page layout by @keiko233

- **proxies:** Support group icon show by @keiko233

- **proxies:** Disable button when type is not selecor by @keiko233

- **rules:** Move filter text input to header by @keiko233

- **rules:** New design for RulesPage by @keiko233

- **service:** Add a service control panel and sidecar check script by @greenhat616

- **setting-clash-base:** Add uwp tools support by @keiko233

- **setting-clash-core:** Support core update by @keiko233

- **setting-clash-field:** Add ClashFieldFilter switch by @keiko233

- **sotre:** Add persistence support by @keiko233

- **theme:** Add MDYPaper style override by @keiko233

- **tray:** Add custom tray icon support by @greenhat616

- **tray:** Add submenu proxies selector by @greenhat616

- **ui:** Md3 style segmented button by @greenhat616

- **ui:** Add scroll area support for side page by @keiko233

- **ui:** Tailwind css support mui breakpoint by @keiko233

- **ui:** Base page use radix-ui scroll area by @keiko233

- **ui:** Dialog allow windows drag when prop full is true by @keiko233

- **ui:** Add full screen style for dialog by @keiko233

- **ui:** Minor tweaks for border radius by @keiko233

- **ui:** Replace Switch to LoadingSwitch for SwitchItem by @keiko233

- **ui:** Init sparkline chart by @keiko233

- **ui:** Add sideClassName props for SidePage component by @keiko233

- **ui:** Add reverse icon props for ExpandMore component by @keiko233

- **ui:** Add MuiLinearProgress material you style override by @keiko233

- **ui:** Add more props support for BaseDialog by @keiko233

- **ui:** Add side toggle animation & reverse layout props by @keiko233

- **ui:** Add SidePage component by @keiko233

- **ui:** Add TextItem component by @keiko233

- **ui:** Add BaseItem component by @keiko233

- **ui:** Add TextFieldProps for NumberItem by @keiko233

- **ui:** Add ExpandMore component by @keiko233

- **ui:** Add loading props support for BaseCard by @keiko233

- **ui:** Add LoadingSwitch component by @keiko233

- **ui:** Add divider props support for BaseDialog by @keiko233

- **ui:** Add BaseDialog component by @keiko233

- **ui:** Add MuiDialog material you override by @keiko233

- **ui:** Add disabled props for MenuItem by @keiko233

- **ui:** Add selectSx for MenuItem component by @keiko233

- **ui:** Add divider props for NumberItem by @keiko233

- **ui:** Add Expand component by @keiko233

- **ui:** Add NumberItem component by @keiko233

- **ui:** Add MenuItem component by @keiko233

- **ui:** Add SwitchItem component by @keiko233

- **ui:** Add BaseCard label props undefined type support by @keiko233

- **ui:** Add MDYBaseCard component by @keiko233

- **ui:** Add MuiSwitch material you override by @keiko233

- **ui:** Add MuiCard & MuiCardContent material you override by @keiko233

- **ui:** Custom breakpoints by @keiko233

- **ui:** Add memo suuport for MDYBasePage header by @keiko233

- **ui:** Add MuiPaper material you override by @keiko233

- **ui:** Add MDYBasePage component by @keiko233

- **ui:** Add MuiButtonGroup material you override by @keiko233

- **ui:** Add MuiButton material you override by @keiko233

- **ui:** Add new mui theme create method for material you by @keiko233

- **updater:** Add a view github button by @greenhat616

- **use-message:** Add nyanpasu title prefix by @keiko233

- **util:** Add a util to collect env infos to submit issues by @greenhat616

- **web:** Replace default utl to Dashboard Page by @keiko233

- **window:** Always on top by @greenhat616

- Minor tweaks for app layout by @keiko233

- Draft updater dialog, and close #1328 by @greenhat616

- Add core updater progress by @keiko233

- Draft core updater progres by @greenhat616

- Add lazy loading for proxies icons by @greenhat616

- Allow select on rule page & log page by @keiko233

- Add clash icon local cache by @greenhat616

- Add runtime config diff dialog by @greenhat616

- Add tun stack selector by @greenhat616

- Impl script esm and async support (#1266) by @greenhat616 in [#1266](https://github.com/libnyanpasu/clash-nyanpasu/pull/1266)

- Should hidden speed chip while no history by @greenhat616

- Add auto migration before app run by @greenhat616

- Add migrations manager and cmds to run migration by @greenhat616

- Add swift feedback button by @greenhat616

- Print better build info by @greenhat616

- Add a experimental mutlithread file download util by @greenhat616

- Experimental add draggable logo by @greenhat616

- Resizable sidebar without config presistant by @greenhat616

- Use node octokit deps by @keiko233

- Profile spec chains support by @greenhat616

- Support lua script type and do a lot refactor by @greenhat616

### 🐛 Bug Fixes

- **app-setting:** Missing fields with template by @keiko233

- **chians:** Throw backend log on use native dialog by @keiko233

- **ci:** Update publish script by @greenhat616

- **ci:** Updater checkout issue by @greenhat616

- **ci:** Updater checkout issue by @greenhat616

- **ci:** Prepend changelog by @greenhat616

- **ci:** Build by @greenhat616

- **clash:** Accpet clash rs status code and handle status error by @greenhat616

- **clash:** Hidden ipv6 setting while clash rs by @greenhat616

- **clash-web:** Fix reversed Boolean value by @keiko233

- **clash-web:** Empty array err by @keiko233

- **config:** Replace enable_auto_check_update by @keiko233

- **connections:** Table type filed err by @keiko233

- **connections:** Host undefined err by @keiko233

- **csp:** Allow loading local cache server assets by @greenhat616

- **csp:** Allow img-src from https by @keiko233

- **custom-scheme:** Xdg-mime default wrong call format by @greenhat616

- **custom-scheme:** Front page redirect by @greenhat616

- **custom-scheme:** Should pass single-instance while launched by custom schema by @greenhat616

- **custom-scheme:** Support mutiple scheme by @greenhat616

- **custom-theme:** Unregister event when the themoe mode is not system by @keiko233

- **custom-theme:** Fix custom theme effect & system theme sync event by @keiko233

- **dashboard:** Data panel layer size err by @keiko233

- **dashboard:** Zero value display err by @keiko233

- **deep link:** Use different identifiers in dev mode by @keiko233

- **deps:** Add misssing deps by @keiko233

- **deps:** Vite-plugin-monaco-editor version err by @keiko233

- **dev:** When dev feature force use dev app dir by @keiko233

- **drawer:** Style prop merge err by @keiko233

- **drawer:** Offset value err by @keiko233

- **drawer:** Small size drawer layout err by @keiko233

- **drawer:** Minor tweaks by @keiko233

- **drawer:** Fix scroll err & hidden scrollbar by @keiko233

- **drawer:** Fix padding & text position by @keiko233

- **enhance:** Rm useless use_lowercase hook, and close #1323 by @greenhat616

- **enhance:** Use oxc ast to wrap function main, close #1298 by @greenhat616

- **enhance:** Should update after editing activated chain item by @greenhat616

- **enhance:** Transform allow lan decrepation by @greenhat616

- **enhance:** Should export default by @greenhat616

- **enhance:** Use indexmap to ensure the process order by @greenhat616

- **enhance:** Mark process fn async by @greenhat616

- **guard:** Remove ipv6 field while core is clash rs by @greenhat616

- **hook:** Replace DebounceFn to ThrottleFn by @keiko233

- **image-resize:** Correct image buffer extraction and resizing logic by @keiko233

- **interface:** Close all connections err by @keiko233

- **interface:** Drop defalut clash mode set by @keiko233

- **interface:** Bad references by @keiko233

- **interface:** Add clash rs version format method by @keiko233

- **interface:** Request clash when use set by @keiko233

- **interface:** Data type err by @keiko233

- **interface:** Typos by @keiko233

- **layout:** Bringup layout control to top layer by @keiko233

- **lint:** Prettier plugin load err by @keiko233

- **linux:** Replace backdrop blur to background opacity by @keiko233

- **linux:** Service controls gui prompt, and close #1443 by @greenhat616

- **linux:** Try to use symbol to fix tray issue by @greenhat616

- **linux:** Use a workaround to make tray select work by @greenhat616

- **linux:** Try to solve sysproxy resolver in appimage by @greenhat616

- **linux:** Try to solve xdg-open in AppImage by @greenhat616

- **logs:** Disable log state err by @keiko233

- **logs:** Logs page freeze by @keiko233

- **logs:** Logs page style err by @keiko233

- **macos:** App icon size by @keiko233

- **macos:** Dialog layout position err by @keiko233

- **macos:** Remove prevent close block in macos by @greenhat616

- **macos:** Rename single instance check path by @greenhat616

- **macos:** Try to use another name to fix create dir error by @greenhat616

- **node-card:** Layout err by @keiko233

- **nsis:** Uninstall service check by @greenhat616

- **nsis:** Stop running core by service while install and rm service dir while uninstall by @greenhat616

- **nyanpasu:** Missing of recoil drop commit by @keiko233

- **nyanpasu:** Missing tailwind css import by @keiko233

- **nyanpasu:** Word typos by @keiko233

- **nyanpasu:** Undfined value err by @keiko233

- **nyanpasu:** Props usage error by @keiko233

- **nyanpasu:** Drop tooltips to fix mui warning by @keiko233

- **portable:** Add nyanpasu service binary by @greenhat616

- **profile:** Dialog padding err by @keiko233

- **profile:** Just invisble progress by @greenhat616

- **profile:** Correctly handle filtering of script types in filterProfiles function by @keiko233

- **profile-viewer:** Replace default profile user agent to clash-nyanpasu by @keiko233

- **profiles:** Dont use sub component to solve the loss data issue by @greenhat616

- **profiles:** Scoped chians state update err by @keiko233

- **profiles:** Add missing open file on chains menu by @keiko233

- **profiles:** Monaco dialog style err by @keiko233

- **profiles:** Fix new chain method err by @keiko233

- **profiles:** Fix profile item selected color on dark mode by @keiko233

- **profiles:** Fix color on dark mode by @keiko233

- **profiles:** Add missing open file method by @keiko233

- **profiles:** Profile traffic percent calculation error by @keiko233

- **profiles:** Add selected props for ProfileItem by @keiko233

- **providers:** Single line layout err by @keiko233

- **proxies:** Proxy node select err & render err by @keiko233

- **proxies:** Sorting cannot be performed in global mode by @keiko233

- **proxies:** Nodecard transition by @keiko233

- **proxies:** Delay sort & timeout string by @keiko233

- **proxies:** Global proxy select err by @keiko233

- **proxies:** Incorrect judgment leading to value transfer error by @keiko233

- **proxies:** Missing import by @keiko233

- **proxies:** Current group get err by @keiko233

- **route:** Reaplce icon dashboard to Dashboard by @keiko233

- **rules:** Rules page display err by @keiko233

- **script:** Decompress nyanpasu-service by @greenhat616

- **script:** Replace appimage to rpm pkg by @keiko233

- **script:** Use latest node version by @keiko233

- **script:** Fix build with nightly prepare script by @keiko233

- **script:** Nightly prepare package.json path by @keiko233

- **scripts:** Typos by @keiko233

- **scripts:** Telegram notify failed to request github repo releases info by @keiko233

- **service:** Restart core while service mode enabled and service state changed by @greenhat616

- **service:** Adapt the current ui by @greenhat616

- **setting:** Service mod toggle by @keiko233

- **setting-clash-core:** Disable initial animetion by @keiko233

- **setting-clash-core:** Add user triger check update loading status by @keiko233

- **setting-nyanpasu-version:** Incorrect value passing by @keiko233

- **setting-system-proxy:** Grid layout breakpoint value by @keiko233

- **setting-web-ui:** Zero value for index err by @keiko233

- **settings:** Version pkg import err by @keiko233

- **settings:** Swr use err by @keiko233

- **settings:** Page masonry layout err by @keiko233

- **settings:** Fix auto check update fileld stats err by @keiko233

- **single-instance:** Should use path instead of namespace in linux by @greenhat616

- **string:** Typo in side-chain.tsx (#999) by @NalCol in [#999](https://github.com/libnyanpasu/clash-nyanpasu/pull/999)

- **styles:** Try to use normalize.css to solve webkit font issue by @greenhat616

- **tauri:** Missing dialog features by @keiko233

- **tauri:** Mixed content err by @keiko233

- **theme:** Fix value merge null err by @keiko233

- **theme:** Update breakpoint value by @keiko233

- **tray:** Add a barrier to try to solve the tray selector issue in linux by @greenhat616

- **tsconfig:** Typescript type reference issue by @keiko233

- **tun:** Compatible with clash rs by @greenhat616

- **ui:** Dialog exit animation err by @keiko233

- **ui:** Close animetion position err by @keiko233

- **ui:** Fix dialog unmount err by @keiko233

- **ui:** Missing dialog z index css prop by @keiko233

- **ui:** Refactor dialog use radix ui portal by @keiko233

- **ui:** Scroll bar hidden on no padding by @keiko233

- **ui:** Base page dom layout err by @keiko233

- **ui:** Add Menu Paper box shadow by @keiko233

- **ui:** Fixed FloatingButton position by @keiko233

- **ui:** Fixed FloatingButton position by @keiko233

- **ui:** Force set FloadtingButton posotion absolute by @keiko233

- **ui:** Drop memo children too by @keiko233

- **ui:** Drop SidePage memo by @keiko233

- **ui:** Hide SidePage side content when there is no side by @keiko233

- **ui:** Drop width for MDYBasePage-content by @keiko233

- **ui:** Fix BasePage content width by @keiko233

- **ui:** Disable loading mask animetion initial for BaseCard by @keiko233

- **ui:** Default unmount dialog modal by @keiko233

- **ui:** Replace padding to Box element by @keiko233

- **ui:** Disable initial animetion for Expand component by @keiko233

- **ui:** Add disabled overlay for MuiSwitch by @keiko233

- **ui:** Fix BaseDialog content height err by @keiko233

- **ui:** Pin MenuItem width by @keiko233

- **ui:** Disbale MuiPaper override by @keiko233

- **updater:** Invaild date issue by @greenhat616

- **updater:** Fetch version.json from main branch (#968) by @aviraxp in [#968](https://github.com/libnyanpasu/clash-nyanpasu/pull/968)

- **util:** Speed test should use desc order by @greenhat616

- **webkit:** Border radius not apply on absolute layout by @keiko233

- **window:** Show window when frontend mounted by @keiko233

- **windows:** Window controller position by @keiko233

- **windows:** Custom scheme call by @greenhat616

- Disable migrate app dir feature in macos, linux by @greenhat616

- Custom scheme url parser in webkit by @greenhat616

- Try to fix read profile state again by @greenhat616

- Add a key to try to solve read profile issue by @greenhat616

- Log time issue, and close #1447 by @greenhat616

- Disable core update check in linux by @greenhat616

- Disable app updater for linux expect AppImage by @greenhat616

- Rm macos unsupport transparent by @greenhat616

- Try to fix cross platform save win state issue by @greenhat616

- Lint by @greenhat616

- Lint by @greenhat616

- Use open_that workaround for appimage by @greenhat616

- React deps by @greenhat616

- Check button issue by @greenhat616

- Lint by @greenhat616

- Profile runtime config button color by @greenhat616

- Nsis build issue by @greenhat616

- Exhaustive-deps lint by @greenhat616

- Disable react complier lint until it fixes bug by @greenhat616

- Add 172.16.0.0/12 system proxy passby on windows (#1405) by @Remonli in [#1405](https://github.com/libnyanpasu/clash-nyanpasu/pull/1405)

- Use tauri client for asn request by @greenhat616

- Proxies nodes list update issue, and close #1402 by @greenhat616

- Lint by @greenhat616

- Mutate core version while updater finished by @greenhat616

- Updater replace issue, and close #1377 by @greenhat616

- Script prepare gh token by @greenhat616

- Lint by @greenhat616

- Build by @greenhat616

- Build by @greenhat616

- Build by @greenhat616

- Lint by @greenhat616

- Lint by @greenhat616

- Try to fix ts project import issue by @greenhat616

- Ts project settings (#1394) by @greenhat616 in [#1394](https://github.com/libnyanpasu/clash-nyanpasu/pull/1394)

- Ts project lint by @greenhat616

- Correct the update order to ensure the script changes get applied by @greenhat616

- Clash config select issue, and close #1303 by @greenhat616

- Spawn orientation random updater id by @keiko233

- Throw single instance create error by @greenhat616

- Connection page lazy loading by @greenhat616

- Config detect, and close #1305 by @greenhat616

- Quick import submit when enter press by @greenhat616

- Icon loader should not lazy by @greenhat616

- Icon lazy image by @greenhat616

- Show a error dialog while check latest cores error, and close #1302 by @greenhat616

- Issues by @greenhat616

- Marquee by @greenhat616

- No need retry while os error 232 by @greenhat616

- Not save clash overrides config, close #1295 by @greenhat616

- Fix broken pipe causing too many logs #637 by @4o3F

- Fix tray not able to reset by @4o3F

- Update sysproxy-rs to support KDE by @4o3F

- Fix url scheme issue #902 by @4o3F

- Use window open counter to prevent double-click opening the window immediately by @greenhat616

- Should update match by @greenhat616

- Make profile yaml file to be formatted by serde yaml by @greenhat616

- Update config while patch profile scoped chain by @greenhat616

- Lint by @greenhat616

- Lint by @greenhat616

- Lint by @greenhat616

- Clash rs core switch by @greenhat616

- Patch profile chains by @greenhat616

- Patch profile chains by @greenhat616

- Lint by @greenhat616

- Ignore deleteConnection error while applying new profile by @greenhat616

- Make port strategy check better by @greenhat616

- No exit code on unix platform by @greenhat616

- Try to solve the migration failed issue by @greenhat616

- Lint by @greenhat616

- Ui service control and updater path by @greenhat616

- Cleanup codes by @greenhat616

- Lint by @greenhat616

- Lint by @greenhat616

- Skip migration while home dir is not exist, and close #1235 by @greenhat616

- Skip migration while home dir is not exist, and close #1235 by @greenhat616

- Lint by @greenhat616

- Should create data dir and config dir when fetch it if not exist by @greenhat616

- Styles by @greenhat616

- Lint by @greenhat616

- Migration panic by @greenhat616

- Migrate all upcoming migrations while pending by @greenhat616

- Migration missing dirs touch by @keiko233

- Left container scrollbar gutter (#1225) by @fu050409 in [#1225](https://github.com/libnyanpasu/clash-nyanpasu/pull/1225)

- Add quote prefix, and solve the undefined issue by @greenhat616

- Drawer resize panel style by @keiko233

- Lint by @greenhat616

- Lint by @greenhat616

- Build by @keiko233

- Build by @keiko233

- Missing export by @keiko233

- Lint in linux by @greenhat616

- Enhance process panic while profiles is empty by @greenhat616

- Fmt by @greenhat616

- Log path by @greenhat616

- Use webview2-com-bridge to solve ra crash issue by @greenhat616

- Lint by @greenhat616

- Minor issues (#884) by @greenhat616 in [#884](https://github.com/libnyanpasu/clash-nyanpasu/pull/884)

- Ci by @greenhat616

- Lint by @greenhat616

- Vite plugin monaco editor overrides by @greenhat616

- Fix issue #776 by @4o3F

- Mac x64 use mihomo compatible core (#773) by @Sakurasan

- Lint by @keiko233

- Change storage_db name by @4o3F

- Fix database creation issue by @4o3F

### 📚 Documentation

- **readme:** Add nyanpasu 1.6.0 label by @keiko233

- **readme:** Fix resource path err by @keiko233

- Fix dev build shields card link err by @keiko233

- Update screenshot & clean up docs by @keiko233

### 🔨 Refactor

- **chains:** Use bitflags instead of custom support struct by @greenhat616

- **connections:** Drop mui/x-data-grid & use material-react-table by @keiko233

- **core:** Use new core manager from nyanpasu utils to prepare for new nyanpasu service by @greenhat616

- **custom-scheme:** Use nonblocking io and create window if window is not exist by @greenhat616

- **dashboard:** Split health panel by @keiko233

- **dirs:** Split home_dir into config_dir and data_dir by @greenhat616

- **drawer:** Use react-split-grid replace react-resizable-panels by @keiko233

- **frontend:** Make monorepo by @keiko233

- **hook:** Use-breakpoint hook with react-use by @keiko233

- **hook:** Optimize useBreakpoint hook to reduce unnecessary updates by @keiko233

- **hotkeys:** First draft hotkeys setting dialog by @greenhat616

- **interface!:** Increase code readability by @keiko233

- **interface/service:** Tauri interface writing by @keiko233

- **layout:** New layout design by @keiko233

- **nsis:** Use nsis's built-in com plugin instead of ApplicationID plugin (#9606) by @amrbashir

- **profiles:** Chians component by @keiko233

- **proxies:** Drop memo use effert to update by @keiko233

- **proxies:** Delay button using tailwind css and memo by @keiko233

- **script:** Manifest generator script by @keiko233

- **script:** Resource check script by @keiko233

- **service:** Add new service backend support by @greenhat616

- **theme:** Migrating to CSS theme variables by @keiko233

- **ui:** Drop mui dialog & use redix-ui with framer motion by @keiko233

- **updater:** Support speedtest and updater concurrency by @greenhat616

- Drop async component use react suspense by @keiko233

- Proxies page use new interface by @keiko233

- Refactor rocksdb into redb, this should solve #452 by @4o3F in [#755](https://github.com/libnyanpasu/clash-nyanpasu/pull/755)

- Refactor rocksdb into redb, this should fix #452 by @4o3F

---

## New Contributors

- @Remonli made their first contribution in [#1405](https://github.com/libnyanpasu/clash-nyanpasu/pull/1405)
- @fu050409 made their first contribution in [#1225](https://github.com/libnyanpasu/clash-nyanpasu/pull/1225)
- @NalCol made their first contribution in [#999](https://github.com/libnyanpasu/clash-nyanpasu/pull/999)
- @aviraxp made their first contribution in [#968](https://github.com/libnyanpasu/clash-nyanpasu/pull/968)
- @amrbashir made their first contribution
- @Sakurasan made their first contribution

**Full Changelog**: https://github.com/libnyanpasu/clash-nyanpasu/compare/v1.5.1...v1.6.0

## [1.5.1] - 2024-04-08

### ✨ Features

- **backend:** Allow to hide tray selector (#626) by @greenhat616 in [#626](https://github.com/libnyanpasu/clash-nyanpasu/pull/626)

- **config:** Support custom app dir in windows (#582) by @greenhat616 in [#582](https://github.com/libnyanpasu/clash-nyanpasu/pull/582)

- **custom-schema:** Add support for name and desc fields by @greenhat616

- Perf motion transition by @keiko233

- Lock rustup toolchain to stable channel by @4o3F

- New design log page by @keiko233

- New desigin rules page by @keiko233

- Improve WebSocket reconnection in useWebsocket hook by @keiko233

### 🐛 Bug Fixes

- **bundler/nsis:** Don't use /R flag on installation dir by @keiko233

- **chains:** Only guard fields should be overwritten (#629) by @greenhat616 in [#629](https://github.com/libnyanpasu/clash-nyanpasu/pull/629)

- **cmds:** Migrate custom app dir typo (#628) by @greenhat616 in [#628](https://github.com/libnyanpasu/clash-nyanpasu/pull/628)

- **cmds:** `path` in changing app dir call (#591) by @greenhat616 in [#591](https://github.com/libnyanpasu/clash-nyanpasu/pull/591)

- **docs:** Fix url typos by @keiko233

- **notification:** Unexpected `}` (#563) by @WOSHIZHAZHA120 in [#563](https://github.com/libnyanpasu/clash-nyanpasu/pull/563)

- Revert previous commit by @greenhat616

- Subscription info parse issue, closing #729 by @greenhat616

- Fix misinterprete of tauri's application args by @4o3F

- Missing github repo context by @keiko233

- Try to add a launch command to make restart application work by @greenhat616

- Try to use delayed singleton check to make restart app work by @greenhat616

- Panic while quit application by @greenhat616

- Restart application not work by @greenhat616

- Fix migration issue for path with space by @4o3F

- Fix migration child process issue by @4o3F

- Fix rename permission issue by @4o3F

- Connection page NaN and first enter animation by @greenhat616

- Use shiki intead of shikiji by @greenhat616

- Use clash verge rev patch to resolve Content-Disposition Filename issue, closing #703 by @greenhat616

- Lint by @greenhat616

- Command path by @greenhat616

- Draft patch to resolve custom app config migration by @greenhat616

- Proxy groups virtuoso also overscan by @keiko233

- Top item no padding by @keiko233

- Use overscan to prevent blank scrolling by @keiko233

- Profiles when drag sort container scroll style by @keiko233

- Profile-box border radius value by @keiko233

- Slinet start get_window err by @keiko233

- MDYSwitch-thumb size by @keiko233

- Build by @keiko233

- Disable webview2 SwipeNavigation by @keiko233

- Fix wrong window size and position by @4o3F

- Fix single instance check failing on macos by @4o3F

### 📚 Documentation

- Add clash-verge-rev acknowledgement by @greenhat616

- Add twitter img tag by @keiko233

- Add license img tag by @keiko233

- Align center tag imgs by @keiko233

- Update readme by @keiko233

- Update issues template by @greenhat616

### 🔨 Refactor

- Use lazy load routes to improve performance by @greenhat616

---

## New Contributors

- @WOSHIZHAZHA120 made their first contribution in [#563](https://github.com/libnyanpasu/clash-nyanpasu/pull/563)

**Full Changelog**: https://github.com/libnyanpasu/clash-nyanpasu/compare/v1.5.0...v1.5.1

## [1.5.0] - 2024-03-03

### 💥 Breaking Changes

- **backend:** Add tray proxies selector support (#417) by @greenhat616 in [#417](https://github.com/libnyanpasu/clash-nyanpasu/pull/417)

- **clash:** Add default core secret and impl port checker before clash start (#533) by @greenhat616 in [#533](https://github.com/libnyanpasu/clash-nyanpasu/pull/533)

### ✨ Features

- **config:** Add migration for old config dir (#419) by @4o3F in [#419](https://github.com/libnyanpasu/clash-nyanpasu/pull/419)

- **connection:** Allow filter out process name by @greenhat616

- **locale:** Use system locale as default (#437) by @greenhat616 in [#437](https://github.com/libnyanpasu/clash-nyanpasu/pull/437)

- **tray:** Add tray icon resize logic to improve icon rendering (#540) by @greenhat616 in [#540](https://github.com/libnyanpasu/clash-nyanpasu/pull/540)

- **tray:** Add diff check for system tray partial update (#477) by @4o3F in [#477](https://github.com/libnyanpasu/clash-nyanpasu/pull/477)

- Custom schema support (#516) by @4o3F in [#516](https://github.com/libnyanpasu/clash-nyanpasu/pull/516)

- Add Auto Check Updates Switch by @keiko233

- Refactor UpdateViewer by @keiko233

- OnCheckUpdate button supports loading animation & refactoring error removal notification using dialog by @keiko233

- Add margin for SettingItem extra element by @keiko233

- Add useMessage hook by @keiko233

- Refactor GuardStatus & support loading status by @keiko233

- MDYSwitch support loading prop by @keiko233

- Add MDYSwitch & replace all Switches with MDYSwitch by @keiko233

- Color select use MuiColorInput by @keiko233

- Make profile material you by @keiko233

- New style design profile item drag sort by @keiko233

### 🐛 Bug Fixes

- **ci:** Replace github workflow token by @keiko233

- **config:** Fix config migration (#433) by @4o3F in [#433](https://github.com/libnyanpasu/clash-nyanpasu/pull/433)

- **custom-schema:** Fix schema not working for new opening and dialog not showing with certain route (#534) by @4o3F in [#534](https://github.com/libnyanpasu/clash-nyanpasu/pull/534)

- **deps:** Update rust crates by @greenhat616

- **macos:** Use rfd to prevent panic by @greenhat616

- **nsis:** Should not stop verge service while updating by @greenhat616

- **proxies:** Use indexmap instead to correct order by @greenhat616

- **proxies:** Reduce tray updating interval by @greenhat616

- **tray:** Use base64 encoded id to fix item not found issue by @greenhat616

- **tray:** Should disable click expect Selector and Fallback type by @greenhat616

- **tray:** Proxies updating deadlock by @greenhat616

- Release ci by @greenhat616

- Release ci by @greenhat616

- Fix wrong window position and size with multiple screen by @4o3F

- Resolve save windows state event by @greenhat616

- Media screen value typos by @keiko233

- Layout error when window width is small by @keiko233

- Lint by @greenhat616

- Line breaks typos by @keiko233

- MDYSwitch switchBase padding value by @keiko233

- Lint by @greenhat616

- Fmt by @greenhat616

- Build issue by @greenhat616

- Config migration issue by @greenhat616

- Ci by @greenhat616

- Proxy item box-shadow err by @keiko233

### 🔨 Refactor

- **clash:** Move api and core manager into one mod (#411) by @greenhat616 in [#411](https://github.com/libnyanpasu/clash-nyanpasu/pull/411)

- **i18n:** Change backend localization to rust-i18n (#425) by @4o3F in [#425](https://github.com/libnyanpasu/clash-nyanpasu/pull/425)

- **logging:** Use `tracing` instead of `log4rs` (#486) by @greenhat616 in [#486](https://github.com/libnyanpasu/clash-nyanpasu/pull/486)

- **proxies:** Proxies hash and diff logic by @greenhat616

- **single-instance:** Refactor single instance check (#499) by @4o3F in [#499](https://github.com/libnyanpasu/clash-nyanpasu/pull/499)

---

**Full Changelog**: https://github.com/libnyanpasu/clash-nyanpasu/compare/v1.4.5...v1.5.0

## [1.4.5] - 2024-02-08

### 💥 Breaking Changes

- **nsis:** Switch to both installMode by @greenhat616

- **updater:** Use nsis instead of msi by @greenhat616

### 🐛 Bug Fixes

- **bundle:** Instance is running while updating app (#393) by @greenhat616 in [#393](https://github.com/libnyanpasu/clash-nyanpasu/pull/393)

- **bundler:** Kill processes while updating in windows by @greenhat616

- **ci:** Daily updater issue (#392) by @greenhat616 in [#392](https://github.com/libnyanpasu/clash-nyanpasu/pull/392)

- **ci:** Nightly updater issue by @greenhat616

- **nsis:** Kill nyanpasu processes while updating (#403) by @greenhat616 in [#403](https://github.com/libnyanpasu/clash-nyanpasu/pull/403)

- Portable issues (#395) by @greenhat616 in [#395](https://github.com/libnyanpasu/clash-nyanpasu/pull/395)

- Minimize icon is wrong while resize window (#394) by @greenhat616 in [#394](https://github.com/libnyanpasu/clash-nyanpasu/pull/394)

- Sort connection in numerical comparison for `Download`, `DL Speed`, etc (#367) by @Jeremy-Hibiki in [#367](https://github.com/libnyanpasu/clash-nyanpasu/pull/367)

- Resources missing by @greenhat616 in [#354](https://github.com/libnyanpasu/clash-nyanpasu/pull/354)

---

## New Contributors

- @Jeremy-Hibiki made their first contribution in [#367](https://github.com/libnyanpasu/clash-nyanpasu/pull/367)

**Full Changelog**: https://github.com/libnyanpasu/clash-nyanpasu/compare/v1.4.4...v1.4.5

## [1.4.4] - 2024-01-29

### 🐛 Bug Fixes

- **backend:** Fix deadlock issue on config (#312) by @4o3F in [#312](https://github.com/libnyanpasu/clash-nyanpasu/pull/312)

- **ci:** Publish & updater by @greenhat616

- **ci:** Should generate manifest in dev branch for compatible with <= 1.4.3 (#292) by @greenhat616 in [#292](https://github.com/libnyanpasu/clash-nyanpasu/pull/292)

- **deps:** Update deps (#294) by @greenhat616 in [#294](https://github.com/libnyanpasu/clash-nyanpasu/pull/294)

- **portable:** Portable bundle issue (#335) by @greenhat616 in [#335](https://github.com/libnyanpasu/clash-nyanpasu/pull/335)

- **portable:** Do not use system notification api while app is portable (#334) by @greenhat616 in [#334](https://github.com/libnyanpasu/clash-nyanpasu/pull/334)

- **updater:** Use release body as updater note (#333) by @greenhat616 in [#333](https://github.com/libnyanpasu/clash-nyanpasu/pull/333)

- Use if let instead (#309) by @greenhat616 in [#309](https://github.com/libnyanpasu/clash-nyanpasu/pull/309)

### 📚 Documentation

- Add ArchLinux AUR install suggestion (#293) by @Kimiblock in [#293](https://github.com/libnyanpasu/clash-nyanpasu/pull/293)

### 🔨 Refactor

- **backend:** Improve code robustness (#303) by @greenhat616 in [#303](https://github.com/libnyanpasu/clash-nyanpasu/pull/303)

---

**Full Changelog**: https://github.com/libnyanpasu/clash-nyanpasu/compare/v1.4.3...v1.4.4

## [1.4.3] - 2024-01-20

### ✨ Features

- New release workflow (#284) by @greenhat616 in [#284](https://github.com/libnyanpasu/clash-nyanpasu/pull/284)

- Proxies ui minor tweaks by @keiko233

- Make proxies material you by @keiko233

### 🐛 Bug Fixes

- **ci:** Pin rust version to 1.74.1 (#213) by @greenhat616 in [#213](https://github.com/libnyanpasu/clash-nyanpasu/pull/213)

- **ci:** Use latest action by @greenhat616

- **ci:** Use dev commit hash when schedule dispatch by @greenhat616

- **log:** Incorrect color in light mode by @greenhat616

- **rocksdb:** Use TransactionDB instead of OptimisticTransactionDB (#194) by @greenhat616 in [#194](https://github.com/libnyanpasu/clash-nyanpasu/pull/194)

- **updater:** Should use nyanpasu proxy or system proxy when performing request (#273) by @greenhat616 in [#273](https://github.com/libnyanpasu/clash-nyanpasu/pull/273)

- **updater:** Add status code judge by @greenhat616

- **updater:** Allow to use elevated permission to copy and override core by @greenhat616

- **vite:** Rm useless shikiji langs support (#267) by @greenhat616 in [#267](https://github.com/libnyanpasu/clash-nyanpasu/pull/267)

- Release ci by @greenhat616

- Publish ci by @greenhat616

- Notification premission check (#263) by @greenhat616 in [#263](https://github.com/libnyanpasu/clash-nyanpasu/pull/263)

- Notification fallback (#262) by @greenhat616 in [#262](https://github.com/libnyanpasu/clash-nyanpasu/pull/262)

- Stable channel build issue (#248) by @greenhat616 in [#248](https://github.com/libnyanpasu/clash-nyanpasu/pull/248)

- Virtuoso scroller bottom not padding by @keiko233

- Windrag err by @keiko233

- Same text color for `REJECT-DROP` policy as `REJECT` (#236) by @xkww3n in [#236](https://github.com/libnyanpasu/clash-nyanpasu/pull/236)

- Enable_tun block the process (#232) by @dyxushuai

- #212 by @greenhat616

- Lint by @greenhat616

- Updater by @greenhat616

- Dark mode flash in win by @greenhat616

- Open file, closing #197 by @greenhat616

- Add a panic hook to collect logs and show a dialog (#191) by @greenhat616 in [#191](https://github.com/libnyanpasu/clash-nyanpasu/pull/191)

---

## New Contributors

- @xkww3n made their first contribution in [#236](https://github.com/libnyanpasu/clash-nyanpasu/pull/236)

**Full Changelog**: https://github.com/libnyanpasu/clash-nyanpasu/compare/v1.4.2...v1.4.3

## [1.4.2] - 2023-12-24

### ✨ Features

- **updater:** Finish ui by @greenhat616

- **updater:** Finish core updater backend by @greenhat616

- Use christmas logo by @keiko233

- Auto add dns according this method by @yswtrue

- Backport concurrency of latency test by @greenhat616

- Auto log clear by @greenhat616

- Nightly build with updater by @greenhat616

- Rules providers by @greenhat616

- Improve animations by @greenhat616

- Quick logs collect by @greenhat616

- Bundled mihomo alpha by @greenhat616

- New style win tray icon & add blue icon when tun enable by @keiko233

### 🐛 Bug Fixes

- **ci:** Release build by @greenhat616

- **ci:** Updater and dev build by @greenhat616

- **dialog:** Align center and overflow issue by @greenhat616

- **lint:** Toml fmt by @greenhat616

- **resources:** Win service support and mihomo alpha version proxy by @greenhat616

- **updater:** Copy logic by @greenhat616

- **window:** Preserve window state before window minimized by @greenhat616

- **window:** Add a workaround for close event in windows by @greenhat616

- Minor tweak base-content width by @keiko233

- Shikiji text wrapping err by @keiko233

- Dark shikiji display color err by @keiko233

- Pin runas to v1.0.0 by @greenhat616

- Lint by @greenhat616

- Bump nightly version after publish by @greenhat616

- I18n resources by @greenhat616

- Format ansi in log viewer by @greenhat616

- Delay color, closing #124 by @greenhat616

- #96 by @greenhat616

- #92 by @greenhat616

- Lint by @greenhat616

- Ci by @greenhat616

- Ci by @greenhat616

- Ci by @greenhat616

- Dev build branch issue by @greenhat616

- Icon issues, close #55 by @greenhat616

- Use a workaroud to reduce #59 by @greenhat616

- Win state by @greenhat616

### 📚 Documentation

- Put issue config into effect (#148) by @txyyh in [#148](https://github.com/libnyanpasu/clash-nyanpasu/pull/148)

- Upload missing issue config by @txyyh

- Update issues template & upload ISSUE.md by @keiko233

### 🔨 Refactor

- **tasks:** Provide a universal abstract layer for task managing (#15) by @greenhat616

- Profile updater by @greenhat616

---

## New Contributors

- @yswtrue made their first contribution
- @txyyh made their first contribution in [#148](https://github.com/libnyanpasu/clash-nyanpasu/pull/148)

**Full Changelog**: https://github.com/libnyanpasu/clash-nyanpasu/compare/v1.4.1...v1.4.2

## [1.4.1] - 2023-12-06

### ✨ Features

- **transition:** Add none and transparent variants by @greenhat616

- Use twemoji to display flags in win (#48) by @greenhat616 in [#48](https://github.com/libnyanpasu/clash-nyanpasu/pull/48)

- Add page transition mode and duration options by @keiko233 in [#42](https://github.com/libnyanpasu/clash-nyanpasu/pull/42)

- Add page transition duration options by @greenhat616

- Add page transition mode switch by @greenhat616

- Use framer-motion for smooth page transition by @greenhat616

- Support new clash field by @greenhat616

- Support drag profile item (#36) by @Kuingsmile in [#36](https://github.com/libnyanpasu/clash-nyanpasu/pull/36)

- Use tauri notification api by @keiko233

- Update new clash.meta close #20 (#30) by @Kuingsmile in [#30](https://github.com/libnyanpasu/clash-nyanpasu/pull/30)

- Support random mixed port (#29) by @Kuingsmile in [#29](https://github.com/libnyanpasu/clash-nyanpasu/pull/29)

- Use workspace in backend by @greenhat616

- New style win tray icon by @keiko233

- Add tooltip for tray (#24) by @Kuingsmile in [#24](https://github.com/libnyanpasu/clash-nyanpasu/pull/24)

- Experimental support `clash-rs` (#23) by @greenhat616 in [#23](https://github.com/libnyanpasu/clash-nyanpasu/pull/23)

- Add UWP tool support, fix install service bug (#19) by @Kuingsmile in [#19](https://github.com/libnyanpasu/clash-nyanpasu/pull/19)

### 🐛 Bug Fixes

- Taskbar maximize toggle icon state (#46) by @greenhat616 in [#46](https://github.com/libnyanpasu/clash-nyanpasu/pull/46)

- Missing scss import by @greenhat616

- Lint by @greenhat616

- Lint by @greenhat616

- Workflow script typos by @keiko233

- Osx-aarch64-upload bundlePath typos by @keiko233

- Portable target dir by @keiko233

- Portable missing clash-rs core by @keiko233

- Item col width too narrow by @keiko233

- I18n typos by @keiko233

### 📚 Documentation

- Add preview gif by @keiko233

### 🔨 Refactor

- **scripts:** Use ts and consola instead by @greenhat616

- Use `workspace` in backend by @keiko233 in [#28](https://github.com/libnyanpasu/clash-nyanpasu/pull/28)

---

## New Contributors

- @Kuingsmile made their first contribution in [#36](https://github.com/libnyanpasu/clash-nyanpasu/pull/36)

**Full Changelog**: https://github.com/libnyanpasu/clash-nyanpasu/compare/v1.4.0...v1.4.1

## [1.4.0] - 2023-11-15

### ✅ Testing

- Windows service by @zzzgydi

### ✨ Features

- **layout:** Add logo & update style by @zzzgydi

- **macOS:** Support cmd+w and cmd+q by @zzzgydi

- **proxy:** Finish proxy page ui and api support by @zzzgydi

- **style:** Adjust style impl by @zzzgydi

- **system tray:** Support switch rule/global/direct/script mode in system tray by @Limsanity

- **traffic:** Api support & adjust by @zzzgydi

- Minor tweaks by @keiko233

- Nyanpasu Misc by @keiko233

- Add baseContentIn animation by @keiko233

- Add route transition by @keiko233

- Material You! by @keiko233

- Default disable ipv6 by @keiko233

- Default enable unified-delay & tcp-concurrent with use meta core by @keiko233

- Support copy CMD & PowerShell proxy env by @keiko233

- Default use meta core by @keiko233

- Update Clash Default bypass addrs by @keiko233

- Theme: change color by @keiko233

- Profiles: import btn with loading state by @keiko233

- Profile-viewer: handleOk with loading state by @keiko233

- Base-dialog: okBtn use LoadingButton by @keiko233

- Nyanpasu Misc by @keiko233

- Theme support modify --background-color by @keiko233

- Settings use Grid layout by @keiko233

- Add Connections Info to ConnectionsPage by @keiko233

- ClashFieldViewer BaseDialog maxHeight usage percentage (#813) by @keiko233

- Add Open Dashboard to the hotkey, close #723 by @zzzgydi

- Add check for updates button, close #766 by @zzzgydi

- Add paste and clear icon by @zzzgydi

- Subscription URL TextField use multiline (#761) by @keiko233

- Show loading when change profile by @zzzgydi

- Support proxy provider update by @zzzgydi

- Add repo link by @zzzgydi

- Support clash meta memory usage display by @zzzgydi

- Supports show connection detail by @zzzgydi

- Update connection table with wider process column and click to show full detail (#696) by @whitemirror33

- More trace logs by @zzzgydi

- Add Russian Language (#697) by @shvchk

- Center window when out of monitor by @zzzgydi

- Support copy environment variable by @zzzgydi

- Save window size and position by @zzzgydi

- App log level add silent by @zzzgydi

- Overwrite resource file according to file modified by @zzzgydi

- Support app log level settings by @zzzgydi

- Use polkit to elevate permission instaed of sudo (#678) by @Kimiblock

- Add unified-delay field by @zzzgydi

- Add error boundary to the app root by @zzzgydi

- Show tray icon variants in different status (#537) by @w568w

- Auto restart core after grand permission by @zzzgydi

- Add restart core button by @zzzgydi

- Support update all profiles by @zzzgydi

- Support to grant permission to clash core by @zzzgydi

- Support clash fields filter in ui by @zzzgydi

- Open dir on the tray by @zzzgydi

- Support to disable clash fields filter by @zzzgydi

- Adjust macOS window style by @zzzgydi

- Recover core after panic, close #353 by @zzzgydi

- Use decorations in Linux, close #354 by @zzzgydi

- Auto proxy layout column by @zzzgydi

- Support to change proxy layout column by @zzzgydi

- Support to open core dir by @zzzgydi

- Profile page ui by @zzzgydi

- Save some fields in the runtime config, close #292 by @zzzgydi

- Add meta feature by @zzzgydi

- Display proxy group type by @zzzgydi

- Add use clash hook by @zzzgydi

- Guard the mixed-port and external-controller by @zzzgydi

- Adjust builtin script and support meta guard script by @zzzgydi

- Disable script mode when use clash meta by @zzzgydi

- Check config when change core by @zzzgydi

- Support builtin script for enhanced mode by @zzzgydi

- Adjust profiles page ui by @zzzgydi

- Optimize proxy page ui by @zzzgydi

- Add error boundary by @zzzgydi

- Adjust clash log by @zzzgydi

- Add draft by @zzzgydi

- Change default latency test url by @zzzgydi

- Auto close connection when proxy changed by @zzzgydi

- Support to change external controller by @zzzgydi

- Add sub-rules by @zzzgydi

- Add version on tray by @zzzgydi

- Add animation by @zzzgydi

- Add animation to ProfileNew component (#252) by @angryLid

- Check remote profile field by @zzzgydi

- System tray support zh language by @zzzgydi

- Display delay check result timely by @zzzgydi

- Update profile with system proxy/clash proxy by @zzzgydi

- Change global mode ui, close #226 by @zzzgydi

- Default user agent same with app version by @zzzgydi

- Optimize config feedback by @zzzgydi

- Show connections with table layout by @zzzgydi

- Show loading on proxy group delay check by @zzzgydi

- Add chains[0] and process to connections display (#205) by @riverscn

- Adjust connection page ui by @zzzgydi

- Yaml merge key by @zzzgydi

- Toggle log ws by @zzzgydi

- Add rule page by @zzzgydi

- Hotkey viewer by @zzzgydi

- Refresh ui when hotkey clicked by @zzzgydi

- Support hotkey (wip) by @zzzgydi

- Hide window on macos by @zzzgydi

- System proxy setting by @zzzgydi

- Change default singleton port and support to change the port by @zzzgydi

- Log info by @zzzgydi

- Kill clash by pid by @zzzgydi

- Change clash port in dialog by @zzzgydi

- Add proxy item check loading by @zzzgydi

- Compatible with proxy providers health check by @zzzgydi

- Add empty ui by @zzzgydi

- Complete i18n by @zzzgydi

- Windows portable version do not check update by @zzzgydi

- Adjust clash info parsing logs by @zzzgydi

- Adjust runtime config by @zzzgydi

- Support restart app on tray by @zzzgydi

- Optimize profile page by @zzzgydi

- Refactor by @zzzgydi

- Adjust tun mode config by @zzzgydi

- Reimplement enhanced mode by @zzzgydi

- Use rquickjs crate by @zzzgydi

- Reimplement enhanced mode by @zzzgydi

- Finish clash field control by @zzzgydi

- Clash field viewer wip by @zzzgydi

- Support web ui by @zzzgydi

- Adjust setting page style by @zzzgydi

- Runtime config viewer by @zzzgydi

- Improve log rule by @zzzgydi

- Theme mode support follows system by @zzzgydi

- Improve yaml file error log by @zzzgydi

- Save proxy page state by @zzzgydi

- Light mode wip (#96) by @ctaoist

- Clash meta core supports by @zzzgydi

- Script mode by @zzzgydi

- Clash meta core support (wip) by @zzzgydi

- Reduce gpu usage when hidden by @zzzgydi

- Interval update from now field by @zzzgydi

- Adjust theme by @zzzgydi

- Supports more remote headers close #81 by @zzzgydi

- Check the remote profile by @zzzgydi

- Fix typo by tianyoulan

- Remove trailing comma by tianyoulan

- Remove outdated config by tianyoulan

- Windows service mode ui by @zzzgydi

- Add some commands by @zzzgydi

- Windows service mode by @zzzgydi

- Add update interval by @zzzgydi

- Refactor and supports cron tasks by @zzzgydi

- Supports cron update profiles by @zzzgydi

- Optimize traffic graph quadratic curve by @zzzgydi

- Optimize the animation of the traffic graph by @zzzgydi

- System tray add tun mode by @zzzgydi

- Supports change config dir by @zzzgydi

- Add default user agent by @zzzgydi

- Connections page supports filter by @zzzgydi

- Log page supports filter by @zzzgydi

- Optimize delay checker concurrency strategy by @zzzgydi

- Support sort proxy node and custom test url by @zzzgydi

- Handle remote clash config fields by @zzzgydi

- Add text color by @zzzgydi

- Control final tun config by @zzzgydi

- Support css injection by @zzzgydi

- Support theme setting by @zzzgydi

- Add text color by @zzzgydi

- Add theme setting by @zzzgydi

- Enhanced mode supports more fields by @zzzgydi

- Supports edit profile file by @zzzgydi

- Supports silent start by @zzzgydi

- Use crate open by @zzzgydi

- Enhance connections display order by @zzzgydi

- Save global selected by @zzzgydi

- System tray supports system proxy setting by @zzzgydi

- Prevent context menu on Windows close #22 by @zzzgydi

- Create local profile with selected file by @zzzgydi

- Reduce the impact of the enhanced mode by @zzzgydi

- Parse update log by @zzzgydi

- Fill i18n by @zzzgydi

- Dayjs i18n by @zzzgydi

- Connections page simply support by @zzzgydi

- Add wintun.dll by default by @zzzgydi

- Event emit when clash config update by @zzzgydi

- I18n supports by @zzzgydi

- Change open command on linux by @zzzgydi

- Support more options for remote profile by @zzzgydi

- Linux system proxy by @zzzgydi

- Enhance profile status by @zzzgydi

- Menu item refresh enhanced mode by @zzzgydi

- Profile enhanced mode by @zzzgydi

- Profile enhanced ui by @zzzgydi

- Profile item adjust by @zzzgydi

- Enhanced profile (wip) by @zzzgydi

- Edit profile item by @zzzgydi

- Use nanoid by @zzzgydi

- Compatible profile config by @zzzgydi

- Native menu supports by @zzzgydi

- Filter proxy and display type by @zzzgydi

- Use lock fn by @zzzgydi

- Refactor proxy page by @zzzgydi

- Proxy group auto scroll to current by @zzzgydi

- Clash tun mode supports by @zzzgydi

- Use enhanced guard-state by @zzzgydi

- Guard state supports debounce guard by @zzzgydi

- Adjust clash version display by @zzzgydi

- Hide command window by @zzzgydi

- Enhance log data by @zzzgydi

- Change window style by @zzzgydi

- Fill verge template by @zzzgydi

- Enable customize guard duration by @zzzgydi

- System proxy guard by @zzzgydi

- Enable show or hide traffic graph by @zzzgydi

- Traffic line graph by @zzzgydi

- Adjust profile item ui by @zzzgydi

- Adjust fetch profile url by @zzzgydi

- Inline config file template by @zzzgydi

- Kill sidecars when update app by @zzzgydi

- Delete file by @zzzgydi

- Lock some async functions by @zzzgydi

- Support open dir by @zzzgydi

- Change allow list by @zzzgydi

- Support check delay by @zzzgydi

- Scroll to proxy item by @zzzgydi

- Edit system proxy bypass by @zzzgydi

- Disable user select by @zzzgydi

- New profile able to edit name and desc by @zzzgydi

- Update tauri version by @zzzgydi

- Display clash core version by @zzzgydi

- Adjust profile item menu by @zzzgydi

- Profile item ui by @zzzgydi

- Support new profile by @zzzgydi

- Support open command for viewing by @zzzgydi

- Global proxies use virtual list by @zzzgydi

- Enable change proxy mode by @zzzgydi

- Update styles by @zzzgydi

- Manage clash mode by @zzzgydi

- Change system porxy when changed port by @zzzgydi

- Enable change mixed port by @zzzgydi

- Manage clash config by @zzzgydi

- Enable update clash info by @zzzgydi

- Rename edit as view by @zzzgydi

- Test auto gen update.json ci by @zzzgydi

- Adjust setting typography by @zzzgydi

- Enable force select profile by @zzzgydi

- Support edit profile item by @zzzgydi

- Adjust control ui by @zzzgydi

- Update profile supports noproxy by @zzzgydi

- Rename page by @zzzgydi

- Refactor and adjust ui by @zzzgydi

- Rm some commands by @zzzgydi

- Change type by @zzzgydi

- Supports auto launch on macos and windows by @zzzgydi

- Adjust proxy page by @zzzgydi

- Press esc hide the window by @zzzgydi

- Show system proxy info by @zzzgydi

- Support blur window by @zzzgydi

- Windows support startup by @zzzgydi

- Window self startup by @zzzgydi

- Use tauri updater by @zzzgydi

- Support update checker by @zzzgydi

- Support macos proxy config by @zzzgydi

- Custom window decorations by @zzzgydi

- Profiles add menu and delete button by @zzzgydi

- Delay put profiles and retry by @zzzgydi

- Window Send and Sync by @zzzgydi

- Support restart sidecar tray event by @zzzgydi

- Prevent click same by @zzzgydi

- Scroller stable by @zzzgydi

- Compatible with macos(wip) by @zzzgydi

- Record selected proxy by @zzzgydi

- Display version by @zzzgydi

- Enhance system proxy setting by @zzzgydi

- Profile loading animation by @zzzgydi

- Github actions support by @zzzgydi

- Rename profile page by @zzzgydi

- Add pre-dev script by @zzzgydi

- Implement a simple singleton process by @zzzgydi

- Use paper for list bg by @zzzgydi

- Supprt log ui by @zzzgydi

- Auto update profiles by @zzzgydi

- Proxy page use swr by @zzzgydi

- Profile item support display updated time by @zzzgydi

- Change the log level order by @zzzgydi

- Only put some fields by @zzzgydi

- Setting page by @zzzgydi

- Add serval commands by @zzzgydi

- Change log file format by @zzzgydi

- Adjust code by @zzzgydi

- Refactor commands and support update profile by @zzzgydi

- System proxy command demo by @zzzgydi

- Support set system proxy command by @zzzgydi

- Profiles ui and put profile support by @zzzgydi

- Remove sec field by @zzzgydi

- Put profile works by @zzzgydi

- Distinguish level notice by @zzzgydi

- Add use-notice hook by @zzzgydi

- Pus_clash_profile support `secret` field by @zzzgydi

- Add put_profiles cmd by @zzzgydi

- Update rule page by @zzzgydi

- Use external controller field by @zzzgydi

- Lock profiles file and support more cmds by @zzzgydi

- Put new profile to clash by default by @zzzgydi

- Enhance clash caller & support more commands by @zzzgydi

- Read clash config by @zzzgydi

- Get profile file name from response by @zzzgydi

- Change the naming strategy by @zzzgydi

- Change rule page by @zzzgydi

- Import profile support by @zzzgydi

- Init verge config struct by @zzzgydi

- Add some clash api by @zzzgydi

- Optimize the proxy group order by @zzzgydi

- Refactor system proxy config by @zzzgydi

- Use resources dir to save files by @zzzgydi

- New setting page by @zzzgydi

- Sort groups by @zzzgydi

- Add favicon by @zzzgydi

- Update icons by @zzzgydi

- Update layout style by @zzzgydi

- Support dark mode by @zzzgydi

- Set min windows by @zzzgydi

- Finish some features by @zzzgydi

- Finish main layout by @zzzgydi

- Use vite by @zzzgydi

### 🐛 Bug Fixes

- **icon:** Change ico file to fix windows tray by @zzzgydi

- **macos:** Set auto launch path to application by @zzzgydi

- **style:** Reduce my by @zzzgydi

- Rust lint by @keiko233

- Valid with unified-delay & tcp-concurrent by @keiko233

- Touchpad scrolling causes blank area to appear by @keiko233

- Typos by @keiko233

- Download clash core from backup repo by @keiko233

- Use meta Country.mmdb by @keiko233

- I18n by @zzzgydi

- Fix page undefined exception, close #770 by @zzzgydi

- Set min window size, close #734 by @zzzgydi

- Rm debug code by @zzzgydi

- Use sudo when pkexec not found by @zzzgydi

- Remove div by @zzzgydi

- List key by @zzzgydi

- Websocket disconnect when window focus by @zzzgydi

- Try fix undefined error by @zzzgydi

- Blurry tray icon in Windows by @zzzgydi

- Enable context menu in editable element by @zzzgydi

- Save window size and pos in Windows by @zzzgydi

- Optimize traffic graph high CPU usage when hidden by @zzzgydi

- Remove fallback group select status, close #659 by @zzzgydi

- Error boundary with key by @zzzgydi

- Connections is null by @zzzgydi

- Font family not works in some interfaces, close #639 by @zzzgydi

- EncodeURIComponent secret by @zzzgydi

- Encode controller secret, close #601 by @zzzgydi

- Linux not change icon by @zzzgydi

- Try fix blank error by @zzzgydi

- Close all connections when change mode by @zzzgydi

- Macos not change icon by @zzzgydi

- Error message null by @zzzgydi

- Profile data undefined error, close #566 by @zzzgydi

- Import url error (#543) by @yettera765

- Linux DEFAULT_BYPASS (#503) by @Mr-Spade

- Open file with vscode by @zzzgydi

- Do not render div as a descendant of p (#494) by @tatiustaitus

- Use replace instead by @zzzgydi

- Escape path space by @zzzgydi

- Escape the space in path (#451) by @dyxushuai

- Add target os linux by @zzzgydi

- Appimage path unwrap panic by @zzzgydi

- Remove esc key listener in macOS by @zzzgydi

- Adjust style by @zzzgydi

- Adjust swr option by @zzzgydi

- Infinite retry when websocket error by @zzzgydi

- Type error by @zzzgydi

- Do not parse log except the clash core by @zzzgydi

- Field sort for filter by @zzzgydi

- Add meta fields by @zzzgydi

- Runtime config user select by @zzzgydi

- App_handle as_ref by @zzzgydi

- Use crate by @zzzgydi

- Appimage auto launch, close #403 by @zzzgydi

- Compatible with UTF8 BOM, close #283 by @zzzgydi

- Use selected proxy after profile changed by @zzzgydi

- Error log by @zzzgydi

- Adjust fields order by @zzzgydi

- Add meta fields by @zzzgydi

- Add os platform value by @zzzgydi

- Reconnect traffic websocket by @zzzgydi

- Parse bytes precision, close #334 by @zzzgydi

- Trigger new profile dialog, close #356 by @zzzgydi

- Parse log cause panic by @zzzgydi

- Avoid setting login item repeatedly, close #326 by @zzzgydi

- Adjust code by @zzzgydi

- Adjust delay check concurrency by @zzzgydi

- Change default column to auto by @zzzgydi

- Change default app version by @zzzgydi

- Adjust rule ui by @zzzgydi

- Adjust log ui by @zzzgydi

- Keep delay data by @zzzgydi

- Use list item button by @zzzgydi

- Proxy item style by @zzzgydi

- Virtuoso no work in legacy browsers (#318) by @moeshin

- Adjust ui by @zzzgydi

- Refresh websocket by @zzzgydi

- Adjust ui by @zzzgydi

- Parse bytes base 1024 by @zzzgydi

- Add clash fields by @zzzgydi

- Direct mode hide proxies by @zzzgydi

- Profile can not edit by @zzzgydi

- Parse logger time by @zzzgydi

- Adjust service mode ui by @zzzgydi

- Adjust style by @zzzgydi

- Check hotkey and optimize hotkey input, close #287 by @zzzgydi

- Mutex dead lock by @zzzgydi

- Adjust item ui by @zzzgydi

- Regenerate config before change core by @zzzgydi

- Close connections when profile change by @zzzgydi

- Lint by @zzzgydi

- Windows service mode by @zzzgydi

- Init config file by @zzzgydi

- Service mode error and fallback to sidecar by @zzzgydi

- Service mode viewer ui by @zzzgydi

- Create theme error, close #294 by @zzzgydi

- MatchMedia().addEventListener #258 (#296) by @moeshin

- Check config by @zzzgydi

- Show global when no rule groups by @zzzgydi

- Service viewer ref by @zzzgydi

- Service ref error by @zzzgydi

- Group proxies render list is null by @zzzgydi

- Pretty bytes by @zzzgydi

- Use verge hook by @zzzgydi

- Adjust notice by @zzzgydi

- Windows issue by @zzzgydi

- Change dev log level by @zzzgydi

- Patch clash config by @zzzgydi

- Cmds params by @zzzgydi

- Adjust singleton detect by @zzzgydi

- Change template by @zzzgydi

- Copy resource file by @zzzgydi

- MediaQueryList addEventListener polyfill by @zzzgydi

- Change default tun dns-hijack by @zzzgydi

- Something by @zzzgydi

- Provider proxy sort by delay by @zzzgydi

- Profile item menu ui dense by @zzzgydi

- Disable auto scroll to proxy by @zzzgydi

- Check remote profile by @zzzgydi

- Remove smoother by @zzzgydi

- Icon button color by @zzzgydi

- Init system proxy correctly by @zzzgydi

- Open file by @zzzgydi

- Reset proxy by @zzzgydi

- Init config error by @zzzgydi

- Adjust reset proxy by @zzzgydi

- Adjust code by @zzzgydi

- Add https proxy by @zzzgydi

- Auto scroll into view when sorted proxies changed by @zzzgydi

- Refresh proxies interval, close #235 by @zzzgydi

- Style by @zzzgydi

- Fetch profile with system proxy, close #249 by @zzzgydi

- The profile is replaced when the request fails. (#246) by @loosheng

- Default dns config by @zzzgydi

- Kill clash when exit in service mode, close #241 by @zzzgydi

- Icon button color inherit by @zzzgydi

- App version to string by @zzzgydi

- Break loop when core terminated by @zzzgydi

- Api error handle by @zzzgydi

- Clash meta not load geoip, close #212 by @zzzgydi

- Sort proxy during loading, close #221 by @zzzgydi

- Not create windows when enable slient start by @zzzgydi

- Root background color by @zzzgydi

- Create window correctly by @zzzgydi

- Set_activation_policy by @zzzgydi

- Disable spell check by @zzzgydi

- Adjust init launch on dev by @zzzgydi

- Ignore disable auto launch error by @zzzgydi

- I18n by @zzzgydi

- Style by @zzzgydi

- Save enable log on localstorage by @zzzgydi

- Typo in api.ts (#207) by @Priestch

- Refresh clash ui await patch by @zzzgydi

- Remove dead code by @zzzgydi

- Style by @zzzgydi

- Handle is none by @zzzgydi

- Unused by @zzzgydi

- Style by @zzzgydi

- Windows logo size by @zzzgydi

- Do not kill sidecar during updating by @zzzgydi

- Delay update config by @zzzgydi

- Reduce logo size by @zzzgydi

- Window center by @zzzgydi

- Log level warn value by @zzzgydi

- Increase delay checker concurrency by @zzzgydi

- External controller allow lan by @zzzgydi

- Remove useless optimizations by @zzzgydi

- Reduce unsafe unwrap by @zzzgydi

- Timer restore at app launch by @FoundTheWOUT

- Adjust log text by @zzzgydi

- Only script profile can display console by @zzzgydi

- Fill button title attr by @zzzgydi

- Do not reset system proxy when consistent by @zzzgydi

- Adjust web ui item style by @zzzgydi

- Clash field state error by @zzzgydi

- Badge color error by @zzzgydi

- Web ui port value error by @zzzgydi

- Delay show window by @zzzgydi

- Adjust dialog action button variant by @zzzgydi

- Script code error by @zzzgydi

- Script exception handle by @zzzgydi

- Change fields by @zzzgydi

- Silent start (#150) by @FoundTheWOUT

- Save profile when update by @zzzgydi

- List compare wrong by @zzzgydi

- Button color by @zzzgydi

- Limit theme mode value by @zzzgydi

- Add valid clash field by @zzzgydi

- Icon style by @zzzgydi

- Reduce unwrap by @zzzgydi

- Import mod by @zzzgydi

- Add tray separator by @zzzgydi

- Instantiate core after init app, close #122 by @zzzgydi

- Rm macOS transition props by @zzzgydi

- Improve external-controller parse and log by @zzzgydi

- Show windows on click by @zzzgydi

- Adjust update profile notice error by @zzzgydi

- Style issue on mac by @zzzgydi

- Check script run on all OS by @FoundTheWOUT

- MacOS disable transparent by @zzzgydi

- Window transparent and can not get hwnd by @zzzgydi

- Create main window by @zzzgydi

- Adjust notice by @zzzgydi

- Label text by @zzzgydi

- Icon path by @zzzgydi

- Icon issue by @zzzgydi

- Notice ui blocking by @zzzgydi

- Service mode error by @zzzgydi

- Win11 drag lag by @zzzgydi

- Rm unwrap by @zzzgydi

- Edit profile info by @zzzgydi

- Change window default size by @zzzgydi

- Change service installer and uninstaller by @zzzgydi

- Adjust connection scroll by @zzzgydi

- Adjust something by @zzzgydi

- Adjust debounce wait time by @zzzgydi

- Adjust dns config by @zzzgydi

- Traffic graph adapt to different fps by @zzzgydi

- Optimize clash launch by @zzzgydi

- Reset after exit by @zzzgydi

- Adjust code by @zzzgydi

- Adjust log by @zzzgydi

- Check button hover style by @zzzgydi

- Icon button color inherit by @zzzgydi

- Remove the lonely zero by @zzzgydi

- I18n add value by @zzzgydi

- Proxy page first render by @zzzgydi

- Console warning by @zzzgydi

- Icon button title by @zzzgydi

- MacOS transition flickers close #47 by @zzzgydi

- Csp image data by @zzzgydi

- Close dialog after save by @zzzgydi

- Change to deep copy by @zzzgydi

- Window style close #45 by @zzzgydi

- Manage global proxy correctly by @zzzgydi

- Tauri csp by @zzzgydi

- Windows style by @zzzgydi

- Update state by @zzzgydi

- Profile item loading state by @zzzgydi

- Adjust windows style by @zzzgydi

- Change mixed port error by @zzzgydi

- Auto launch path by @zzzgydi

- Tun mode config by @zzzgydi

- Adjsut open cmd error by @zzzgydi

- Parse external-controller by @zzzgydi

- Config file case close #18 by @zzzgydi

- Patch item option by @zzzgydi

- User agent not works by @zzzgydi

- External-controller by @zzzgydi

- Change proxy bypass on mac by @zzzgydi

- Kill sidecars after install still in test by @zzzgydi

- Log some error by @zzzgydi

- Apply_blur parameter by @zzzgydi

- Limit enhanced profile range by @zzzgydi

- Profile updated field by @zzzgydi

- Profile field check by @zzzgydi

- Create dir panic by @zzzgydi

- Only error when selected by @zzzgydi

- Enhanced profile consistency by @zzzgydi

- Simply compatible with proxy providers by @zzzgydi

- Component warning by @zzzgydi

- When updater failed by @zzzgydi

- Log file by @zzzgydi

- Result by @zzzgydi

- Cover profile extra by @zzzgydi

- Display menu only on macos by @zzzgydi

- Proxy global showType by @zzzgydi

- Use full clash config by @zzzgydi

- Reconnect websocket when restart clash by @zzzgydi

- Wrong exe path by @zzzgydi

- Patch verge config by @zzzgydi

- Fetch profile panic by @zzzgydi

- Spawn command by @zzzgydi

- Import error by @zzzgydi

- Not open file when new profile by @zzzgydi

- Reset value correctly by @zzzgydi

- Something by @zzzgydi

- Menu without fragment by @zzzgydi

- Proxy list error by @zzzgydi

- Something by @zzzgydi

- Macos auto launch fail by @zzzgydi

- Type error by @zzzgydi

- Restart clash should update something by @zzzgydi

- Script error... by @zzzgydi

- Tag error by @zzzgydi

- Script error by @zzzgydi

- Remove cargo test by @zzzgydi

- Reduce proxy item height by @zzzgydi

- Put profile request with no proxy by @zzzgydi

- Ci strategy by @zzzgydi

- Version update error by @zzzgydi

- Text by @zzzgydi

- Update profile after restart clash by @zzzgydi

- Get proxies multiple times by @zzzgydi

- Delete profile item command by @zzzgydi

- Initialize profiles state by @zzzgydi

- Item header bgcolor by @zzzgydi

- Null type error by @zzzgydi

- Api loading delay by @zzzgydi

- Mutate at the same time may be wrong by @zzzgydi

- Port value not rerender by @zzzgydi

- Change log file format by @zzzgydi

- Proxy bypass add <local> by @zzzgydi

- Sidecar dir by @zzzgydi

- Web resource outDir by @zzzgydi

- Use io by @zzzgydi

### 💅 Styling

- Resolve formatting problem by @Limsanity

### 📚 Documentation

- Fix img width by @zzzgydi

- Update by @zzzgydi

### 🔨 Refactor

- **hotkey:** Use tauri global shortcut by @zzzgydi

- Copy_clash_env by @keiko233

- Adjust base components export by @zzzgydi

- Adjust setting dialog component by @zzzgydi

- Done by @zzzgydi

- Adjust all path methods and reduce unwrap by @zzzgydi

- Rm code by @zzzgydi

- Fix by @zzzgydi

- Rm dead code by @zzzgydi

- For windows by @zzzgydi

- Wip by @zzzgydi

- Wip by @zzzgydi

- Wip by @zzzgydi

- Rm update item block_on by @zzzgydi

- Fix by @zzzgydi

- Fix by @zzzgydi

- Wip by @zzzgydi

- Optimize by @zzzgydi

- Ts path alias by @zzzgydi

- Mode manage on tray by @zzzgydi

- Verge by @zzzgydi

- Wip by @zzzgydi

- Mutex by @zzzgydi

- Wip by @zzzgydi

- Proxy head by @zzzgydi

- Update profile menu by @zzzgydi

- Enhanced mode ui component by @zzzgydi

- Ui theme by @zzzgydi

- Optimize enhance mode strategy by @zzzgydi

- Profile config by @zzzgydi

- Use anyhow to handle error by @zzzgydi

- Rename profiles & command state by @zzzgydi

- Something by @zzzgydi

- Notice caller by @zzzgydi

- Setting page by @zzzgydi

- Rename by @zzzgydi

- Impl structs methods by @zzzgydi

- Impl as struct methods by @zzzgydi

- Api and command by @zzzgydi

- Import profile by @zzzgydi

- Adjust dirs structure by @zzzgydi

---

## New Contributors

- @zzzgydi made their first contribution
- @whitemirror33 made their first contribution
- @shvchk made their first contribution
- @w568w made their first contribution
- @yettera765 made their first contribution
- @tatiustaitus made their first contribution
- @Mr-Spade made their first contribution
- @solancer made their first contribution
- @me1ting made their first contribution
- @boatrainlsz made their first contribution
- @inRm3D made their first contribution
- @moeshin made their first contribution
- @angryLid made their first contribution
- @loosheng made their first contribution
- @ParticleG made their first contribution
- @HougeLangley made their first contribution
- @Priestch made their first contribution
- @riverscn made their first contribution
- @FoundTheWOUT made their first contribution
- @Limsanity made their first contribution
- @ctaoist made their first contribution
- @ made their first contribution
- @ttys3 made their first contribution
