## [1.6.0] - 2024-08-29

### 💥 Breaking Changes

- Tsconfig options by keiko233

### ⚡ Performance Improvements

- **hook:** Add debounce callback & do nothing when minimized by keiko233

- **proxies:** Add useTransition by keiko233

- **ui:** Memoized children node by keiko233

- **ui:** Add ref support for BasePage by keiko233

- Switch log page & rule page to async component by keiko233

### ✨ Features

- **component:** Add children props support for PaperButton by keiko233

- **connections:** Lazy load connections and close #1208 by Jonson Petard

- **connections:** Add no connection display by keiko233

- **connections:** New design for ConnectionsPage by keiko233

- **custom-schema:** Experimental compatible with common clash schema by Petard Jonson

- **custom-scheme:** Use one desktop file to process mime by Petard Jonson

- **custom-theme:** Background color picker minor tweak by keiko233

- **dashboard:** Add service status shortcuts card by keiko233

- **dashboard:** Add proxy shortcuts panel by keiko233

- **dashboard:** Special grid layout for drawer by keiko233

- **dashboard:** Add health panel by keiko233

- **dashboard:** Init Dashboard Page by keiko233

- **delay-button:** Minor tweaks for animetion by keiko233

- **downloader:** Make downloader status readable by Petard Jonson

- **drawer:** Enable panel collapsible by keiko233

- **drawer:** Add small size layout by keiko233

- **drawer:** Minor tweak for small size by keiko233

- **enhance:** Experimental add lua runner support by Petard Jonson

- **enhance:** Make merge process more powerful by Petard Jonson

- **experimental:** Initial react compiler support by keiko233

- **interface:** Initial ClashWS by keiko233

- **interface:** Add profile js interface by keiko233

- **interface:** Add current clash mode interface by keiko233

- **interface:** Add useClashCore hook method by keiko233

- **interface:** Add app tauri invoke interface by keiko233

- **interface:** Add profiles api with SWR by keiko233

- **interface:** Add ClashInfo interface with SWR by keiko233

- **interface:** Init code by keiko233

- **ipc:** Replace timing utils ofetch to tokio by keiko233

- **ipc:** Export delay test and core status call by Petard Jonson

- **layout:** Add scrollbar track margin by keiko233

- **logs:** New design LogsPage by keiko233

- **macos:** Try to impl dock show/hide api by Petard Jonson

- **macos:** Add traffic control offset for macos by keiko233

- **migration:** Add discard method for discarding changes while migration failed by Petard Jonson

- **monaco:** Add monaco types support by keiko233

- **monaco:** Add typescript language service by keiko233

- **monaco:** Import lua language support by keiko233

- **monaco-edit:** Switch to lazy load module by keiko233

- **monaco-editor:** Support props value changes and language switching by keiko233

- **monaco-editor:** Support language change on prop by keiko233

- **motion:** Add lighten animation effects config by keiko233

- **nyanpasu:** Node list support proxy delay testing by keiko233

- **nyanpasu:** Import react devtools on dev env by keiko233

- **nyanpasu:** Use new design Proxies Page by keiko233

- **nyanpasu:** Import tailwind css by keiko233

- **nyanpasu:** Experimentally added new settings interface by keiko233

- **nyanpasu:** Add SettingLegacy component by keiko233

- **nyanpasu:** Add SettingNyanpasuVersion component by keiko233

- **nyanpasu:** Add SettingNyanpasuUI component by keiko233

- **nyanpasu:** Add SettingNyanpasuPath component by keiko233

- **nyanpasu:** Add SettingNyanpasuPath component by keiko233

- **nyanpasu:** Add PaperButton component by keiko233

- **nyanpasu:** Add SettingNyanpasuTasks component by keiko233

- **nyanpasu:** Add SettingSystemService component by keiko233

- **nyanpasu:** Add SettingSystemBehavior component by keiko233

- **nyanpasu:** Add SettingSystemClash component by keiko233

- **nyanpasu:** Add SettingClashCore component by keiko233

- **nyanpasu:** Use grid layout for SettingClashWeb by keiko233

- **nyanpasu:** Add SettingClashField component by keiko233

- **nyanpasu:** Add SettingClashWeb component by keiko233

- **nyanpasu:** Add SettingClashExternal component by keiko233

- **nyanpasu:** Add SettingClashPort component by keiko233

- **nyanpasu:** Add SettingClashBase component by keiko233

- **nyanpasu:** Add nyanpasu setting props creator by keiko233

- **nyanpasu:** Use new theme create method by keiko233

- **nynapasu:** Add SettingNyanpasuMisc component by keiko233

- **profiles:** Adapting scroll area & add position animation by keiko233

- **profiles:** Add diff dialog hint by Jonson Petard

- **profiles:** Add max log level triggered notice, and close #1291 by Jonson Petard

- **profiles:** Add black touch new option by Jonson Petard

- **profiles:** Add text carousel for subscription expires and updated time by Petard Jonson

- **profiles:** Minor tweaks & add click card to apply profile by keiko233

- **profiles:** Add split pane support & minor tweaks by keiko233

- **profiles:** Profiles new design by keiko233

- **profiles:** Add proxy chain side page by keiko233

- **profiles:** Add monaco editor for ProfileItem by keiko233

- **profiles:** Complete profile operation menu by keiko233

- **profiles:** Redesign profile cards & new profile editor by keiko233

- **profiles:** Profile dialog support edit mode by keiko233

- **profiles:** Add QuickImport text arae component by keiko233

- **profiles:** Init new profile page by keiko233

- **providers:** Add proxy provider traffic display support by keiko233

- **providers:** Support proxies providers by keiko233

- **providers:** New design ProvidersPage by keiko233

- **proxies:** Filter proxies nodes by Petard Jonson

- **proxies:** Adapting scroll area by keiko233

- **proxies:** Support proxy group test url by keiko233

- **proxies:** Add scroll to current node button by keiko233

- **proxies:** Add node card animation by keiko233

- **proxies:** Group name transition use framer motion by keiko233

- **proxies:** Add none proxies tips by keiko233

- **proxies:** Add virtual scrolling to grid node list by keiko233

- **proxies:** Group list use virtual scrolling by keiko233

- **proxies:** Add node list sorting function by keiko233

- **proxies:** Add group name text transition by keiko233

- **proxies:** Add diff clash mode page layout by keiko233

- **proxies:** Support group icon show by keiko233

- **proxies:** Disable button when type is not selecor by keiko233

- **rules:** Move filter text input to header by keiko233

- **rules:** New design for RulesPage by keiko233

- **service:** Add a service control panel and sidecar check script by Petard Jonson

- **setting-clash-base:** Add uwp tools support by keiko233

- **setting-clash-core:** Support core update by keiko233

- **setting-clash-field:** Add ClashFieldFilter switch by keiko233

- **sotre:** Add persistence support by keiko233

- **theme:** Add MDYPaper style override by keiko233

- **tray:** Add custom tray icon support by Petard Jonson

- **tray:** Add submenu proxies selector by Petard Jonson

- **ui:** Md3 style segmented button by Petard Jonson

- **ui:** Add scroll area support for side page by keiko233

- **ui:** Tailwind css support mui breakpoint by keiko233

- **ui:** Base page use radix-ui scroll area by keiko233

- **ui:** Dialog allow windows drag when prop full is true by keiko233

- **ui:** Add full screen style for dialog by keiko233

- **ui:** Minor tweaks for border radius by keiko233

- **ui:** Replace Switch to LoadingSwitch for SwitchItem by keiko233

- **ui:** Init sparkline chart by keiko233

- **ui:** Add sideClassName props for SidePage component by keiko233

- **ui:** Add reverse icon props for ExpandMore component by keiko233

- **ui:** Add MuiLinearProgress material you style override by keiko233

- **ui:** Add more props support for BaseDialog by keiko233

- **ui:** Add side toggle animation & reverse layout props by keiko233

- **ui:** Add SidePage component by keiko233

- **ui:** Add TextItem component by keiko233

- **ui:** Add BaseItem component by keiko233

- **ui:** Add TextFieldProps for NumberItem by keiko233

- **ui:** Add ExpandMore component by keiko233

- **ui:** Add loading props support for BaseCard by keiko233

- **ui:** Add LoadingSwitch component by keiko233

- **ui:** Add divider props support for BaseDialog by keiko233

- **ui:** Add BaseDialog component by keiko233

- **ui:** Add MuiDialog material you override by keiko233

- **ui:** Add disabled props for MenuItem by keiko233

- **ui:** Add selectSx for MenuItem component by keiko233

- **ui:** Add divider props for NumberItem by keiko233

- **ui:** Add Expand component by keiko233

- **ui:** Add NumberItem component by keiko233

- **ui:** Add MenuItem component by keiko233

- **ui:** Add SwitchItem component by keiko233

- **ui:** Add BaseCard label props undefined type support by keiko233

- **ui:** Add MDYBaseCard component by keiko233

- **ui:** Add MuiSwitch material you override by keiko233

- **ui:** Add MuiCard & MuiCardContent material you override by keiko233

- **ui:** Custom breakpoints by keiko233

- **ui:** Add memo suuport for MDYBasePage header by keiko233

- **ui:** Add MuiPaper material you override by keiko233

- **ui:** Add MDYBasePage component by keiko233

- **ui:** Add MuiButtonGroup material you override by keiko233

- **ui:** Add MuiButton material you override by keiko233

- **ui:** Add new mui theme create method for material you by keiko233

- **updater:** Add a view github button by Petard Jonson

- **use-message:** Add nyanpasu title prefix by keiko233

- **util:** Add a util to collect env infos to submit issues by Petard Jonson

- **web:** Replace default utl to Dashboard Page by keiko233

- **window:** Always on top by Petard Jonson

- Minor tweaks for app layout by keiko233

- Draft updater dialog, and close #1328 by Petard Jonson

- Add core updater progress by keiko233

- Draft core updater progres by Jonson Petard

- Add lazy loading for proxies icons by Jonson Petard

- Allow select on rule page & log page by keiko233

- Add clash icon local cache by Petard Jonson

- Add runtime config diff dialog by Petard Jonson

- Add tun stack selector by Petard Jonson

- Impl script esm and async support (#1266) by Jonson Petard

- Should hidden speed chip while no history by Petard Jonson

- Add auto migration before app run by Petard Jonson

- Add migrations manager and cmds to run migration by Petard Jonson

- Add swift feedback button by Petard Jonson

- Print better build info by Petard Jonson

- Add a experimental mutlithread file download util by Petard Jonson

- Experimental add draggable logo by Petard Jonson

- Resizable sidebar without config presistant by Petard Jonson

- Use node octokit deps by keiko233

- Profile spec chains support by Petard Jonson

- Support lua script type and do a lot refactor by Petard Jonson

### 🐛 Bug Fixes

- **app-setting:** Missing fields with template by keiko233

- **chians:** Throw backend log on use native dialog by keiko233

- **ci:** Build by Petard Jonson

- **clash:** Accpet clash rs status code and handle status error by Petard Jonson

- **clash:** Hidden ipv6 setting while clash rs by Petard Jonson

- **clash-web:** Fix reversed Boolean value by keiko233

- **clash-web:** Empty array err by keiko233

- **config:** Replace enable_auto_check_update by keiko233

- **connections:** Table type filed err by keiko233

- **connections:** Host undefined err by keiko233

- **csp:** Allow loading local cache server assets by Petard Jonson

- **csp:** Allow img-src from https by keiko233

- **custom-scheme:** Xdg-mime default wrong call format by Petard Jonson

- **custom-scheme:** Front page redirect by Petard Jonson

- **custom-scheme:** Should pass single-instance while launched by custom schema by Petard Jonson

- **custom-scheme:** Support mutiple scheme by Petard Jonson

- **custom-theme:** Unregister event when the themoe mode is not system by keiko233

- **custom-theme:** Fix custom theme effect & system theme sync event by keiko233

- **dashboard:** Data panel layer size err by keiko233

- **dashboard:** Zero value display err by keiko233

- **deep link:** Use different identifiers in dev mode by keiko233

- **deps:** Add misssing deps by keiko233

- **deps:** Vite-plugin-monaco-editor version err by keiko233

- **dev:** When dev feature force use dev app dir by keiko233

- **drawer:** Style prop merge err by keiko233

- **drawer:** Offset value err by keiko233

- **drawer:** Small size drawer layout err by keiko233

- **drawer:** Minor tweaks by keiko233

- **drawer:** Fix scroll err & hidden scrollbar by keiko233

- **drawer:** Fix padding & text position by keiko233

- **enhance:** Rm useless use_lowercase hook, and close #1323 by Jonson Petard

- **enhance:** Use oxc ast to wrap function main, close #1298 by Petard Jonson

- **enhance:** Should update after editing activated chain item by Petard Jonson

- **enhance:** Transform allow lan decrepation by Petard Jonson

- **enhance:** Should export default by Petard Jonson

- **enhance:** Use indexmap to ensure the process order by Petard Jonson

- **enhance:** Mark process fn async by Petard Jonson

- **guard:** Remove ipv6 field while core is clash rs by Petard Jonson

- **hook:** Replace DebounceFn to ThrottleFn by keiko233

- **image-resize:** Correct image buffer extraction and resizing logic by keiko233

- **interface:** Close all connections err by keiko233

- **interface:** Drop defalut clash mode set by keiko233

- **interface:** Bad references by keiko233

- **interface:** Add clash rs version format method by keiko233

- **interface:** Request clash when use set by keiko233

- **interface:** Data type err by keiko233

- **interface:** Typos by keiko233

- **layout:** Bringup layout control to top layer by keiko233

- **lint:** Prettier plugin load err by keiko233

- **linux:** Replace backdrop blur to background opacity by keiko233

- **linux:** Service controls gui prompt, and close #1443 by Petard Jonson

- **linux:** Try to use symbol to fix tray issue by Petard Jonson

- **linux:** Use a workaround to make tray select work by Petard Jonson

- **linux:** Try to solve sysproxy resolver in appimage by Petard Jonson

- **linux:** Try to solve xdg-open in AppImage by Petard Jonson

- **logs:** Disable log state err by keiko233

- **logs:** Logs page freeze by keiko233

- **logs:** Logs page style err by keiko233

- **macos:** App icon size by keiko233

- **macos:** Dialog layout position err by keiko233

- **macos:** Remove prevent close block in macos by Petard Jonson

- **macos:** Rename single instance check path by Petard Jonson

- **macos:** Try to use another name to fix create dir error by Petard Jonson

- **node-card:** Layout err by keiko233

- **nsis:** Uninstall service check by Petard Jonson

- **nsis:** Stop running core by service while install and rm service dir while uninstall by Petard Jonson

- **nyanpasu:** Missing of recoil drop commit by keiko233

- **nyanpasu:** Missing tailwind css import by keiko233

- **nyanpasu:** Word typos by keiko233

- **nyanpasu:** Undfined value err by keiko233

- **nyanpasu:** Props usage error by keiko233

- **nyanpasu:** Drop tooltips to fix mui warning by keiko233

- **portable:** Add nyanpasu service binary by Petard Jonson

- **profile:** Dialog padding err by keiko233

- **profile:** Just invisble progress by Petard Jonson

- **profile:** Correctly handle filtering of script types in filterProfiles function by keiko233

- **profile-viewer:** Replace default profile user agent to clash-nyanpasu by keiko233

- **profiles:** Dont use sub component to solve the loss data issue by Petard Jonson

- **profiles:** Scoped chians state update err by keiko233

- **profiles:** Add missing open file on chains menu by keiko233

- **profiles:** Monaco dialog style err by keiko233

- **profiles:** Fix new chain method err by keiko233

- **profiles:** Fix profile item selected color on dark mode by keiko233

- **profiles:** Fix color on dark mode by keiko233

- **profiles:** Add missing open file method by keiko233

- **profiles:** Profile traffic percent calculation error by keiko233

- **profiles:** Add selected props for ProfileItem by keiko233

- **providers:** Single line layout err by keiko233

- **proxies:** Proxy node select err & render err by keiko233

- **proxies:** Sorting cannot be performed in global mode by keiko233

- **proxies:** Nodecard transition by keiko233

- **proxies:** Delay sort & timeout string by keiko233

- **proxies:** Global proxy select err by keiko233

- **proxies:** Incorrect judgment leading to value transfer error by keiko233

- **proxies:** Missing import by keiko233

- **proxies:** Current group get err by keiko233

- **route:** Reaplce icon dashboard to Dashboard by keiko233

- **rules:** Rules page display err by keiko233

- **script:** Decompress nyanpasu-service by Petard Jonson

- **script:** Replace appimage to rpm pkg by keiko233

- **script:** Use latest node version by keiko233

- **script:** Fix build with nightly prepare script by keiko233

- **script:** Nightly prepare package.json path by keiko233

- **service:** Restart core while service mode enabled and service state changed by Petard Jonson

- **service:** Adapt the current ui by Petard Jonson

- **setting:** Service mod toggle by keiko233

- **setting-clash-core:** Disable initial animetion by keiko233

- **setting-clash-core:** Add user triger check update loading status by keiko233

- **setting-nyanpasu-version:** Incorrect value passing by keiko233

- **setting-system-proxy:** Grid layout breakpoint value by keiko233

- **setting-web-ui:** Zero value for index err by keiko233

- **settings:** Swr use err by keiko233

- **settings:** Page masonry layout err by keiko233

- **settings:** Fix auto check update fileld stats err by keiko233

- **single-instance:** Should use path instead of namespace in linux by Jonson Petard

- **string:** Typo in side-chain.tsx (#999) by NaturalCool

- **styles:** Try to use normalize.css to solve webkit font issue by Petard Jonson

- **tauri:** Missing dialog features by keiko233

- **tauri:** Mixed content err by keiko233

- **theme:** Fix value merge null err by keiko233

- **theme:** Update breakpoint value by keiko233

- **tray:** Add a barrier to try to solve the tray selector issue in linux by Petard Jonson

- **tsconfig:** Typescript type reference issue by keiko233

- **tun:** Compatible with clash rs by Petard Jonson

- **ui:** Dialog exit animation err by keiko233

- **ui:** Close animetion position err by keiko233

- **ui:** Fix dialog unmount err by keiko233

- **ui:** Missing dialog z index css prop by keiko233

- **ui:** Refactor dialog use radix ui portal by keiko233

- **ui:** Scroll bar hidden on no padding by keiko233

- **ui:** Base page dom layout err by keiko233

- **ui:** Add Menu Paper box shadow by keiko233

- **ui:** Fixed FloatingButton position by keiko233

- **ui:** Fixed FloatingButton position by keiko233

- **ui:** Force set FloadtingButton posotion absolute by keiko233

- **ui:** Drop memo children too by keiko233

- **ui:** Drop SidePage memo by keiko233

- **ui:** Hide SidePage side content when there is no side by keiko233

- **ui:** Drop width for MDYBasePage-content by keiko233

- **ui:** Fix BasePage content width by keiko233

- **ui:** Disable loading mask animetion initial for BaseCard by keiko233

- **ui:** Default unmount dialog modal by keiko233

- **ui:** Replace padding to Box element by keiko233

- **ui:** Disable initial animetion for Expand component by keiko233

- **ui:** Add disabled overlay for MuiSwitch by keiko233

- **ui:** Fix BaseDialog content height err by keiko233

- **ui:** Pin MenuItem width by keiko233

- **ui:** Disbale MuiPaper override by keiko233

- **updater:** Invaild date issue by Petard Jonson

- **updater:** Fetch version.json from main branch (#968) by Wang Han

- **util:** Speed test should use desc order by Petard Jonson

- **webkit:** Border radius not apply on absolute layout by keiko233

- **window:** Show window when frontend mounted by keiko233

- **windows:** Window controller position by keiko233

- **windows:** Custom scheme call by Petard Jonson

- Disable migrate app dir feature in macos, linux by Petard Jonson

- Custom scheme url parser in webkit by Petard Jonson

- Try to fix read profile state again by Petard Jonson

- Add a key to try to solve read profile issue by Petard Jonson

- Log time issue, and close #1447 by Petard Jonson

- Disable core update check in linux by Petard Jonson

- Disable app updater for linux expect AppImage by Petard Jonson

- Rm macos unsupport transparent by Petard Jonson

- Try to fix cross platform save win state issue by Petard Jonson

- Lint by Petard Jonson

- Lint by Petard Jonson

- Use open_that workaround for appimage by Petard Jonson

- React deps by Petard Jonson

- Check button issue by Petard Jonson

- Lint by Petard Jonson

- Profile runtime config button color by Petard Jonson

- Nsis build issue by Petard Jonson

- Exhaustive-deps lint by Petard Jonson

- Disable react complier lint until it fixes bug by Petard Jonson

- Add 172.16.0.0/12 system proxy passby on windows (#1405) by Remember

- Use tauri client for asn request by Petard Jonson

- Proxies nodes list update issue, and close #1402 by Petard Jonson

- Lint by Petard Jonson

- Mutate core version while updater finished by Petard Jonson

- Updater replace issue, and close #1377 by Petard Jonson

- Script prepare gh token by Petard Jonson

- Lint by Petard Jonson

- Build by Petard Jonson

- Build by Petard Jonson

- Build by Petard Jonson

- Lint by Petard Jonson

- Lint by Petard Jonson

- Try to fix ts project import issue by Petard Jonson

- Ts project settings (#1394) by Jonson Petard

- Ts project lint by Petard Jonson

- Correct the update order to ensure the script changes get applied by Jonson Petard

- Clash config select issue, and close #1303 by Jonson Petard

- Spawn orientation random updater id by keiko233

- Throw single instance create error by Jonson Petard

- Connection page lazy loading by Jonson Petard

- Config detect, and close #1305 by Jonson Petard

- Quick import submit when enter press by Jonson Petard

- Icon loader should not lazy by Jonson Petard

- Icon lazy image by Jonson Petard

- Show a error dialog while check latest cores error, and close #1302 by Jonson Petard

- Issues by Petard Jonson

- Marquee by Petard Jonson

- No need retry while os error 232 by Petard Jonson

- Not save clash overrides config, close #1295 by Petard Jonson

- Fix broken pipe causing too many logs #637 by 4o3F

- Fix tray not able to reset by 4o3F

- Update sysproxy-rs to support KDE by 4o3F

- Fix url scheme issue #902 by 4o3F

- Use window open counter to prevent double-click opening the window immediately by Petard Jonson

- Should update match by Petard Jonson

- Make profile yaml file to be formatted by serde yaml by Petard Jonson

- Update config while patch profile scoped chain by Petard Jonson

- Lint by Petard Jonson

- Lint by Petard Jonson

- Lint by Petard Jonson

- Clash rs core switch by Petard Jonson

- Patch profile chains by Petard Jonson

- Patch profile chains by Petard Jonson

- Lint by Petard Jonson

- Ignore deleteConnection error while applying new profile by Petard Jonson

- Make port strategy check better by Petard Jonson

- No exit code on unix platform by Petard Jonson

- Try to solve the migration failed issue by Petard Jonson

- Lint by Petard Jonson

- Ui service control and updater path by Petard Jonson

- Cleanup codes by Petard Jonson

- Lint by Petard Jonson

- Lint by Petard Jonson

- Skip migration while home dir is not exist, and close #1235 by Petard Jonson

- Skip migration while home dir is not exist, and close #1235 by Petard Jonson

- Lint by Petard Jonson

- Should create data dir and config dir when fetch it if not exist by Petard Jonson

- Styles by Petard Jonson

- Lint by Petard Jonson

- Migration panic by Petard Jonson

- Migrate all upcoming migrations while pending by Petard Jonson

- Migration missing dirs touch by keiko233

- Left container scrollbar gutter (#1225) by 苏向夜

- Add quote prefix, and solve the undefined issue by Petard Jonson

- Drawer resize panel style by keiko233

- Lint by Petard Jonson

- Lint by Petard Jonson

- Build by keiko233

- Build by keiko233

- Missing export by keiko233

- Lint in linux by Jonson Petard

- Enhance process panic while profiles is empty by Petard Jonson

- Fmt by Petard Jonson

- Log path by Petard Jonson

- Use webview2-com-bridge to solve ra crash issue by Petard Jonson

- Lint by Petard Jonson

- Minor issues (#884) by Jonson Petard

- Ci by Petard Jonson

- Lint by Petard Jonson

- Vite plugin monaco editor overrides by Petard Jonson

- Fix issue #776 by 4o3F

- Mac x64 use mihomo compatible core (#773) by Sakurasan

- Lint by keiko233

- Change storage_db name by 4o3F

- Fix database creation issue by 4o3F

### 📚 Documentation

- **readme:** Add nyanpasu 1.6.0 label by keiko233

- **readme:** Fix resource path err by keiko233

- Fix dev build shields card link err by keiko233

- Update screenshot & clean up docs by keiko233

### 🔨 Refactor

- **chains:** Use bitflags instead of custom support struct by Petard Jonson

- **connections:** Drop mui/x-data-grid & use material-react-table by keiko233

- **core:** Use new core manager from nyanpasu utils to prepare for new nyanpasu service by Petard Jonson

- **custom-scheme:** Use nonblocking io and create window if window is not exist by Petard Jonson

- **dashboard:** Split health panel by keiko233

- **dirs:** Split home_dir into config_dir and data_dir by Petard Jonson

- **drawer:** Use react-split-grid replace react-resizable-panels by keiko233

- **frontend:** Make monorepo by keiko233

- **hook:** Use-breakpoint hook with react-use by keiko233

- **hook:** Optimize useBreakpoint hook to reduce unnecessary updates by keiko233

- **hotkeys:** First draft hotkeys setting dialog by Petard Jonson

- **interface!:** Increase code readability by keiko233

- **interface/service:** Tauri interface writing by keiko233

- **layout:** New layout design by keiko233

- **nsis:** Use nsis's built-in com plugin instead of ApplicationID plugin (#9606) by Amr Bashir

- **profiles:** Chians component by keiko233

- **proxies:** Drop memo use effert to update by keiko233

- **proxies:** Delay button using tailwind css and memo by keiko233

- **script:** Manifest generator script by keiko233

- **script:** Resource check script by keiko233

- **service:** Add new service backend support by Petard Jonson

- **theme:** Migrating to CSS theme variables by keiko233

- **ui:** Drop mui dialog & use redix-ui with framer motion by keiko233

- **updater:** Support speedtest and updater concurrency by Petard Jonson

- Drop async component use react suspense by keiko233

- Proxies page use new interface by keiko233

- Refactor rocksdb into redb, this should solve #452 by 403F

- Refactor rocksdb into redb, this should fix #452 by 4o3F

---

**Full Changelog**: https://github.com///compare/v1.5.1...v1.6.0

## [1.5.1] - 2024-04-08

### ✨ Features

- **backend:** Allow to hide tray selector (#626) by Jonson Petard

- **config:** Support custom app dir in windows (#582) by Jonson Petard

- **custom-schema:** Add support for name and desc fields by Jonson Petard

- Perf motion transition by keiko233

- Lock rustup toolchain to stable channel by 4o3F

- New design log page by keiko233

- New desigin rules page by keiko233

- Improve WebSocket reconnection in useWebsocket hook by keiko233

### 🐛 Bug Fixes

- **bundler/nsis:** Don't use /R flag on installation dir by keiko233

- **chains:** Only guard fields should be overwritten (#629) by Jonson Petard

- **cmds:** Migrate custom app dir typo (#628) by Jonson Petard

- **cmds:** `path` in changing app dir call (#591) by Jonson Petard

- **docs:** Fix url typos by keiko233

- **notification:** Unexpected `}` (#563) by 渣渣120

- Revert previous commit by Jonson Petard

- Subscription info parse issue, closing #729 by Jonson Petard

- Fix misinterprete of tauri's application args by 4o3F

- Missing github repo context by keiko233

- Try to add a launch command to make restart application work by Jonson Petard

- Try to use delayed singleton check to make restart app work by Jonson Petard

- Panic while quit application by Jonson Petard

- Restart application not work by Jonson Petard

- Fix migration issue for path with space by 4o3F

- Fix migration child process issue by 4o3F

- Fix rename permission issue by 4o3F

- Connection page NaN and first enter animation by Jonson Petard

- Use shiki intead of shikiji by Jonson Petard

- Use clash verge rev patch to resolve Content-Disposition Filename issue, closing #703 by Jonson Petard

- Lint by Jonson Petard

- Command path by Jonson Petard

- Draft patch to resolve custom app config migration by Jonson Petard

- Proxy groups virtuoso also overscan by keiko233

- Top item no padding by keiko233

- Use overscan to prevent blank scrolling by keiko233

- Profiles when drag sort container scroll style by keiko233

- Profile-box border radius value by keiko233

- Slinet start get_window err by keiko233

- MDYSwitch-thumb size by keiko233

- Build by keiko233

- Disable webview2 SwipeNavigation by keiko233

- Fix wrong window size and position by 4o3F

- Fix single instance check failing on macos by 4o3F

### 📚 Documentation

- Add clash-verge-rev acknowledgement by Jonson Petard

- Add twitter img tag by keiko233

- Add license img tag by keiko233

- Align center tag imgs by keiko233

- Update readme by keiko233

- Update issues template by Jonson Petard

### 🔨 Refactor

- Use lazy load routes to improve performance by Jonson Petard

---

**Full Changelog**: https://github.com///compare/v1.5.0...v1.5.1

## [1.5.0] - 2024-03-03

### 💥 Breaking Changes

- **backend:** Add tray proxies selector support (#417) by Jonson Petard

- **clash:** Add default core secret and impl port checker before clash start (#533) by Jonson Petard

### ✨ Features

- **config:** Add migration for old config dir (#419) by 403F

- **connection:** Allow filter out process name by Jonson Petard

- **locale:** Use system locale as default (#437) by Jonson Petard

- **tray:** Add tray icon resize logic to improve icon rendering (#540) by Jonson Petard

- **tray:** Add diff check for system tray partial update (#477) by 403F

- Custom schema support (#516) by 403F

- Add Auto Check Updates Switch by keiko233

- Refactor UpdateViewer by keiko233

- OnCheckUpdate button supports loading animation & refactoring error removal notification using dialog by keiko233

- Add margin for SettingItem extra element by keiko233

- Add useMessage hook by keiko233

- Refactor GuardStatus & support loading status by keiko233

- MDYSwitch support loading prop by keiko233

- Add MDYSwitch & replace all Switches with MDYSwitch by keiko233

- Color select use MuiColorInput by keiko233

- Make profile material you by keiko233

- New style design profile item drag sort by keiko233

### 🐛 Bug Fixes

- **ci:** Replace github workflow token by keiko233

- **config:** Fix config migration (#433) by 403F

- **custom-schema:** Fix schema not working for new opening and dialog not showing with certain route (#534) by 403F

- **deps:** Update rust crates by Jonson Petard

- **macos:** Use rfd to prevent panic by Jonson Petard

- **nsis:** Should not stop verge service while updating by Jonson Petard

- **proxies:** Use indexmap instead to correct order by Jonson Petard

- **proxies:** Reduce tray updating interval by Jonson Petard

- **tray:** Use base64 encoded id to fix item not found issue by Jonson Petard

- **tray:** Should disable click expect Selector and Fallback type by Jonson Petard

- **tray:** Proxies updating deadlock by Jonson Petard

- Release ci by Jonson Petard

- Release ci by Jonson Petard

- Fix wrong window position and size with multiple screen by 4o3F

- Resolve save windows state event by Jonson Petard

- Media screen value typos by keiko233

- Layout error when window width is small by keiko233

- Lint by Jonson Petard

- Line breaks typos by keiko233

- MDYSwitch switchBase padding value by keiko233

- Lint by Jonson Petard

- Fmt by greenhat616

- Build issue by greenhat616

- Config migration issue by Jonson Petard

- Ci by Jonson Petard

- Proxy item box-shadow err by keiko233

### 🔨 Refactor

- **clash:** Move api and core manager into one mod (#411) by Jonson Petard

- **i18n:** Change backend localization to rust-i18n (#425) by 403F

- **logging:** Use `tracing` instead of `log4rs` (#486) by Jonson Petard

- **proxies:** Proxies hash and diff logic by Jonson Petard

- **single-instance:** Refactor single instance check (#499) by 403F

---

**Full Changelog**: https://github.com///compare/v1.4.5...v1.5.0

## [1.4.5] - 2024-02-08

### 💥 Breaking Changes

- **nsis:** Switch to both installMode by Jonson Petard

- **updater:** Use nsis instead of msi by Jonson Petard

### 🐛 Bug Fixes

- **bundle:** Instance is running while updating app (#393) by Jonson Petard

- **bundler:** Kill processes while updating in windows by Jonson Petard

- **ci:** Daily updater issue (#392) by Jonson Petard

- **ci:** Nightly updater issue by Jonson Petard

- **nsis:** Kill nyanpasu processes while updating (#403) by Jonson Petard

- Portable issues (#395) by Jonson Petard

- Minimize icon is wrong while resize window (#394) by Jonson Petard

- Sort connection in numerical comparison for `Download`, `DL Speed`, etc (#367) by Jeremy JIANG

- Resources missing by Jonson Petard

---

**Full Changelog**: https://github.com///compare/v1.4.4...v1.4.5

## [1.4.4] - 2024-01-29

### 🐛 Bug Fixes

- **backend:** Fix deadlock issue on config (#312) by 403F

- **ci:** Publish & updater by Jonson Petard

- **ci:** Should generate manifest in dev branch for compatible with <= 1.4.3 (#292) by Jonson Petard

- **deps:** Update deps (#294) by Jonson Petard

- **portable:** Portable bundle issue (#335) by Jonson Petard

- **portable:** Do not use system notification api while app is portable (#334) by Jonson Petard

- **updater:** Use release body as updater note (#333) by Jonson Petard

- Use if let instead (#309) by Jonson Petard

### 📚 Documentation

- Add ArchLinux AUR install suggestion (#293) by Kimiblock Moe

### 🔨 Refactor

- **backend:** Improve code robustness (#303) by Jonson Petard

---

**Full Changelog**: https://github.com///compare/v1.4.3...v1.4.4

## [1.4.3] - 2024-01-20

### ✨ Features

- New release workflow (#284) by Jonson Petard

- Proxies ui minor tweaks by keiko233

- Make proxies material you by keiko233

### 🐛 Bug Fixes

- **ci:** Pin rust version to 1.74.1 (#213) by Jonson Petard

- **ci:** Use latest action by Jonson Petard

- **ci:** Use dev commit hash when schedule dispatch by Jonson Petard

- **log:** Incorrect color in light mode by Jonson Petard

- **rocksdb:** Use TransactionDB instead of OptimisticTransactionDB (#194) by Jonson Petard

- **updater:** Should use nyanpasu proxy or system proxy when performing request (#273) by Jonson Petard

- **updater:** Add status code judge by Jonson Petard

- **updater:** Allow to use elevated permission to copy and override core by Jonson Petard

- **vite:** Rm useless shikiji langs support (#267) by Jonson Petard

- Release ci by Jonson Petard

- Publish ci by Jonson Petard

- Notification premission check (#263) by Jonson Petard

- Notification fallback (#262) by Jonson Petard

- Stable channel build issue (#248) by Jonson Petard

- Virtuoso scroller bottom not padding by keiko233

- Windrag err by keiko233

- Same text color for `REJECT-DROP` policy as `REJECT` (#236) by xkww3n

- Enable_tun block the process (#232) by John Smith

- #212 by Jonson Petard

- Lint by Jonson Petard

- Updater by Jonson Petard

- Dark mode flash in win by Jonson Petard

- Open file, closing #197 by Jonson Petard

- Add a panic hook to collect logs and show a dialog (#191) by Jonson Petard

---

**Full Changelog**: https://github.com///compare/v1.4.2...v1.4.3

## [1.4.2] - 2023-12-24

### ✨ Features

- **updater:** Finish ui by Jonson Petard

- **updater:** Finish core updater backend by Jonson Petard

- Use christmas logo by keiko233

- Auto add dns according this method by roy

- Backport concurrency of latency test by Jonson Petard

- Auto log clear by Jonson Petard

- Nightly build with updater by Jonson Petard

- Rules providers by Jonson Petard

- Improve animations by Jonson Petard

- Quick logs collect by Jonson Petard

- Bundled mihomo alpha by Jonson Petard

- New style win tray icon & add blue icon when tun enable by keiko233

### 🐛 Bug Fixes

- **ci:** Release build by Jonson Petard

- **ci:** Updater and dev build by Jonson Petard

- **dialog:** Align center and overflow issue by Jonson Petard

- **lint:** Toml fmt by Jonson Petard

- **resources:** Win service support and mihomo alpha version proxy by Jonson Petard

- **updater:** Copy logic by Jonson Petard

- **window:** Preserve window state before window minimized by Jonson Petard

- **window:** Add a workaround for close event in windows by Jonson Petard

- Minor tweak base-content width by keiko233

- Shikiji text wrapping err by keiko233

- Dark shikiji display color err by keiko233

- Pin runas to v1.0.0 by Jonson Petard

- Lint by Jonson Petard

- Bump nightly version after publish by Jonson Petard

- I18n resources by Jonson Petard

- Format ansi in log viewer by Jonson Petard

- Delay color, closing #124 by Jonson Petard

- #96 by Jonson Petard

- #92 by Jonson Petard

- Lint by Jonson Petard

- Ci by Jonson Petard

- Ci by Jonson Petard

- Ci by Jonson Petard

- Dev build branch issue by Jonson Petard

- Icon issues, close #55 by Jonson Petard

- Use a workaroud to reduce #59 by Jonson Petard

- Win state by Jonson Petard

### 📚 Documentation

- Put issue config into effect (#148) by txyyh

- Upload missing issue config by txyyh

- Update issues template & upload ISSUE.md by keiko233

### 🔨 Refactor

- **tasks:** Provide a universal abstract layer for task managing (#15) by Jonson Petard

- Profile updater by Jonson Petard

---

**Full Changelog**: https://github.com///compare/v1.4.1...v1.4.2

## [1.4.1] - 2023-12-06

### ✨ Features

- **transition:** Add none and transparent variants by Jonson Petard

- Use twemoji to display flags in win (#48) by Jonson Petard

- Add page transition mode and duration options by Majokeiko

- Add page transition duration options by Jonson Petard

- Add page transition mode switch by Jonson Petard

- Use framer-motion for smooth page transition by Jonson Petard

- Support new clash field by Jonson Petard

- Support drag profile item (#36) by Kuingsmile

- Use tauri notification api by keiko233

- Update new clash.meta close #20 (#30) by Kuingsmile

- Support random mixed port (#29) by Kuingsmile

- Use workspace in backend by Jonson Petard

- New style win tray icon by keiko233

- Add tooltip for tray (#24) by Kuingsmile

- Experimental support `clash-rs` (#23) by Jonson Petard

- Add UWP tool support, fix install service bug (#19) by Kuingsmile

### 🐛 Bug Fixes

- Taskbar maximize toggle icon state (#46) by Jonson Petard

- Missing scss import by Jonson Petard

- Lint by Jonson Petard

- Lint by Jonson Petard

- Workflow script typos by keiko233

- Osx-aarch64-upload bundlePath typos by keiko233

- Portable target dir by keiko233

- Portable missing clash-rs core by keiko233

- Item col width too narrow by keiko233

- I18n typos by keiko233

### 📚 Documentation

- Add preview gif by keiko233

### 🔨 Refactor

- **scripts:** Use ts and consola instead by Jonson Petard

- Use `workspace` in backend by Majokeiko

---

**Full Changelog**: https://github.com///compare/v1.4.0...v1.4.1

## [1.4.0] - 2023-11-15

### ✅ Testing

- Windows service by GyDi

### ✨ Features

- **layout:** Add logo & update style by GyDi

- **macOS:** Support cmd+w and cmd+q by GyDi

- **proxy:** Finish proxy page ui and api support by GyDi

- **style:** Adjust style impl by GyDi

- **system tray:** Support switch rule/global/direct/script mode in system tray by limsanity

- **traffic:** Api support & adjust by GyDi

- Minor tweaks by keiko233

- Nyanpasu Misc by keiko233

- Add baseContentIn animation by keiko233

- Add route transition by keiko233

- Material You! by keiko233

- Default disable ipv6 by keiko233

- Default enable unified-delay & tcp-concurrent with use meta core by keiko233

- Support copy CMD & PowerShell proxy env by keiko233

- Default use meta core by keiko233

- Update Clash Default bypass addrs by keiko233

- Theme: change color by keiko233

- Profiles: import btn with loading state by keiko233

- Profile-viewer: handleOk with loading state by keiko233

- Base-dialog: okBtn use LoadingButton by keiko233

- Nyanpasu Misc by keiko233

- Theme support modify --background-color by keiko233

- Settings use Grid layout by keiko233

- Add Connections Info to ConnectionsPage by keiko233

- ClashFieldViewer BaseDialog maxHeight usage percentage (#813) by Majokeiko

- Add Open Dashboard to the hotkey, close #723 by GyDi

- Add check for updates button, close #766 by GyDi

- Add paste and clear icon by GyDi

- Subscription URL TextField use multiline (#761) by Majokeiko

- Show loading when change profile by GyDi

- Support proxy provider update by GyDi

- Add repo link by GyDi

- Support clash meta memory usage display by GyDi

- Supports show connection detail by GyDi

- Update connection table with wider process column and click to show full detail (#696) by whitemirror33

- More trace logs by GyDi

- Add Russian Language (#697) by Andrei Shevchuk

- Center window when out of monitor by GyDi

- Support copy environment variable by GyDi

- Save window size and position by GyDi

- App log level add silent by GyDi

- Overwrite resource file according to file modified by GyDi

- Support app log level settings by GyDi

- Use polkit to elevate permission instaed of sudo (#678) by Kimiblock Moe

- Add unified-delay field by GyDi

- Add error boundary to the app root by GyDi

- Show tray icon variants in different status (#537) by w568w

- Auto restart core after grand permission by GyDi

- Add restart core button by GyDi

- Support update all profiles by GyDi

- Support to grant permission to clash core by GyDi

- Support clash fields filter in ui by GyDi

- Open dir on the tray by GyDi

- Support to disable clash fields filter by GyDi

- Adjust macOS window style by GyDi

- Recover core after panic, close #353 by GyDi

- Use decorations in Linux, close #354 by GyDi

- Auto proxy layout column by GyDi

- Support to change proxy layout column by GyDi

- Support to open core dir by GyDi

- Profile page ui by GyDi

- Save some fields in the runtime config, close #292 by GyDi

- Add meta feature by GyDi

- Display proxy group type by GyDi

- Add use clash hook by GyDi

- Guard the mixed-port and external-controller by GyDi

- Adjust builtin script and support meta guard script by GyDi

- Disable script mode when use clash meta by GyDi

- Check config when change core by GyDi

- Support builtin script for enhanced mode by GyDi

- Adjust profiles page ui by GyDi

- Optimize proxy page ui by GyDi

- Add error boundary by GyDi

- Adjust clash log by GyDi

- Add draft by GyDi

- Change default latency test url by GyDi

- Auto close connection when proxy changed by GyDi

- Support to change external controller by GyDi

- Add sub-rules by GyDi

- Add version on tray by GyDi

- Add animation by GyDi

- Add animation to ProfileNew component (#252) by angrylid

- Check remote profile field by GyDi

- System tray support zh language by GyDi

- Display delay check result timely by GyDi

- Update profile with system proxy/clash proxy by GyDi

- Change global mode ui, close #226 by GyDi

- Default user agent same with app version by GyDi

- Optimize config feedback by GyDi

- Show connections with table layout by GyDi

- Show loading on proxy group delay check by GyDi

- Add chains[0] and process to connections display (#205) by Shun Li

- Adjust connection page ui by GyDi

- Yaml merge key by GyDi

- Toggle log ws by GyDi

- Add rule page by GyDi

- Hotkey viewer by GyDi

- Refresh ui when hotkey clicked by GyDi

- Support hotkey (wip) by GyDi

- Hide window on macos by GyDi

- System proxy setting by GyDi

- Change default singleton port and support to change the port by GyDi

- Log info by GyDi

- Kill clash by pid by GyDi

- Change clash port in dialog by GyDi

- Add proxy item check loading by GyDi

- Compatible with proxy providers health check by GyDi

- Add empty ui by GyDi

- Complete i18n by GyDi

- Windows portable version do not check update by GyDi

- Adjust clash info parsing logs by GyDi

- Adjust runtime config by GyDi

- Support restart app on tray by GyDi

- Optimize profile page by GyDi

- Refactor by GyDi

- Adjust tun mode config by GyDi

- Reimplement enhanced mode by GyDi

- Use rquickjs crate by GyDi

- Reimplement enhanced mode by GyDi

- Finish clash field control by GyDi

- Clash field viewer wip by GyDi

- Support web ui by GyDi

- Adjust setting page style by GyDi

- Runtime config viewer by GyDi

- Improve log rule by GyDi

- Theme mode support follows system by GyDi

- Improve yaml file error log by GyDi

- Save proxy page state by GyDi

- Light mode wip (#96) by ctaoist

- Clash meta core supports by GyDi

- Script mode by GyDi

- Clash meta core support (wip) by GyDi

- Reduce gpu usage when hidden by GyDi

- Interval update from now field by GyDi

- Adjust theme by GyDi

- Supports more remote headers close #81 by GyDi

- Check the remote profile by GyDi

- Fix typo by tianyoulan

- Remove trailing comma by tianyoulan

- Remove outdated config by tianyoulan

- Windows service mode ui by GyDi

- Add some commands by GyDi

- Windows service mode by GyDi

- Add update interval by GyDi

- Refactor and supports cron tasks by GyDi

- Supports cron update profiles by GyDi

- Optimize traffic graph quadratic curve by GyDi

- Optimize the animation of the traffic graph by GyDi

- System tray add tun mode by GyDi

- Supports change config dir by GyDi

- Add default user agent by GyDi

- Connections page supports filter by GyDi

- Log page supports filter by GyDi

- Optimize delay checker concurrency strategy by GyDi

- Support sort proxy node and custom test url by GyDi

- Handle remote clash config fields by GyDi

- Add text color by GyDi

- Control final tun config by GyDi

- Support css injection by GyDi

- Support theme setting by GyDi

- Add text color by GyDi

- Add theme setting by GyDi

- Enhanced mode supports more fields by GyDi

- Supports edit profile file by GyDi

- Supports silent start by GyDi

- Use crate open by GyDi

- Enhance connections display order by GyDi

- Save global selected by GyDi

- System tray supports system proxy setting by GyDi

- Prevent context menu on Windows close #22 by GyDi

- Create local profile with selected file by GyDi

- Reduce the impact of the enhanced mode by GyDi

- Parse update log by GyDi

- Fill i18n by GyDi

- Dayjs i18n by GyDi

- Connections page simply support by GyDi

- Add wintun.dll by default by GyDi

- Event emit when clash config update by GyDi

- I18n supports by GyDi

- Change open command on linux by GyDi

- Support more options for remote profile by GyDi

- Linux system proxy by GyDi

- Enhance profile status by GyDi

- Menu item refresh enhanced mode by GyDi

- Profile enhanced mode by GyDi

- Profile enhanced ui by GyDi

- Profile item adjust by GyDi

- Enhanced profile (wip) by GyDi

- Edit profile item by GyDi

- Use nanoid by GyDi

- Compatible profile config by GyDi

- Native menu supports by GyDi

- Filter proxy and display type by GyDi

- Use lock fn by GyDi

- Refactor proxy page by GyDi

- Proxy group auto scroll to current by GyDi

- Clash tun mode supports by GyDi

- Use enhanced guard-state by GyDi

- Guard state supports debounce guard by GyDi

- Adjust clash version display by GyDi

- Hide command window by GyDi

- Enhance log data by GyDi

- Change window style by GyDi

- Fill verge template by GyDi

- Enable customize guard duration by GyDi

- System proxy guard by GyDi

- Enable show or hide traffic graph by GyDi

- Traffic line graph by GyDi

- Adjust profile item ui by GyDi

- Adjust fetch profile url by GyDi

- Inline config file template by GyDi

- Kill sidecars when update app by GyDi

- Delete file by GyDi

- Lock some async functions by GyDi

- Support open dir by GyDi

- Change allow list by GyDi

- Support check delay by GyDi

- Scroll to proxy item by GyDi

- Edit system proxy bypass by GyDi

- Disable user select by GyDi

- New profile able to edit name and desc by GyDi

- Update tauri version by GyDi

- Display clash core version by GyDi

- Adjust profile item menu by GyDi

- Profile item ui by GyDi

- Support new profile by GyDi

- Support open command for viewing by GyDi

- Global proxies use virtual list by GyDi

- Enable change proxy mode by GyDi

- Update styles by GyDi

- Manage clash mode by GyDi

- Change system porxy when changed port by GyDi

- Enable change mixed port by GyDi

- Manage clash config by GyDi

- Enable update clash info by GyDi

- Rename edit as view by GyDi

- Test auto gen update.json ci by GyDi

- Adjust setting typography by GyDi

- Enable force select profile by GyDi

- Support edit profile item by GyDi

- Adjust control ui by GyDi

- Update profile supports noproxy by GyDi

- Rename page by GyDi

- Refactor and adjust ui by GyDi

- Rm some commands by GyDi

- Change type by GyDi

- Supports auto launch on macos and windows by GyDi

- Adjust proxy page by GyDi

- Press esc hide the window by GyDi

- Show system proxy info by GyDi

- Support blur window by GyDi

- Windows support startup by GyDi

- Window self startup by GyDi

- Use tauri updater by GyDi

- Support update checker by GyDi

- Support macos proxy config by GyDi

- Custom window decorations by GyDi

- Profiles add menu and delete button by GyDi

- Delay put profiles and retry by GyDi

- Window Send and Sync by GyDi

- Support restart sidecar tray event by GyDi

- Prevent click same by GyDi

- Scroller stable by GyDi

- Compatible with macos(wip) by GyDi

- Record selected proxy by GyDi

- Display version by GyDi

- Enhance system proxy setting by GyDi

- Profile loading animation by GyDi

- Github actions support by GyDi

- Rename profile page by GyDi

- Add pre-dev script by GyDi

- Implement a simple singleton process by GyDi

- Use paper for list bg by GyDi

- Supprt log ui by GyDi

- Auto update profiles by GyDi

- Proxy page use swr by GyDi

- Profile item support display updated time by GyDi

- Change the log level order by GyDi

- Only put some fields by GyDi

- Setting page by GyDi

- Add serval commands by GyDi

- Change log file format by GyDi

- Adjust code by GyDi

- Refactor commands and support update profile by GyDi

- System proxy command demo by GyDi

- Support set system proxy command by GyDi

- Profiles ui and put profile support by GyDi

- Remove sec field by GyDi

- Put profile works by GyDi

- Distinguish level notice by GyDi

- Add use-notice hook by GyDi

- Pus_clash_profile support `secret` field by GyDi

- Add put_profiles cmd by GyDi

- Update rule page by GyDi

- Use external controller field by GyDi

- Lock profiles file and support more cmds by GyDi

- Put new profile to clash by default by GyDi

- Enhance clash caller & support more commands by GyDi

- Read clash config by GyDi

- Get profile file name from response by GyDi

- Change the naming strategy by GyDi

- Change rule page by GyDi

- Import profile support by GyDi

- Init verge config struct by GyDi

- Add some clash api by GyDi

- Optimize the proxy group order by GyDi

- Refactor system proxy config by GyDi

- Use resources dir to save files by GyDi

- New setting page by GyDi

- Sort groups by GyDi

- Add favicon by GyDi

- Update icons by GyDi

- Update layout style by GyDi

- Support dark mode by GyDi

- Set min windows by GyDi

- Finish some features by GyDi

- Finish main layout by GyDi

- Use vite by GyDi

### 🐛 Bug Fixes

- **icon:** Change ico file to fix windows tray by GyDi

- **macos:** Set auto launch path to application by GyDi

- **style:** Reduce my by GyDi

- Rust lint by keiko233

- Valid with unified-delay & tcp-concurrent by keiko233

- Touchpad scrolling causes blank area to appear by keiko233

- Typos by keiko233

- Download clash core from backup repo by keiko233

- Use meta Country.mmdb by keiko233

- I18n by GyDi

- Fix page undefined exception, close #770 by GyDi

- Set min window size, close #734 by GyDi

- Rm debug code by GyDi

- Use sudo when pkexec not found by GyDi

- Remove div by GyDi

- List key by GyDi

- Websocket disconnect when window focus by GyDi

- Try fix undefined error by GyDi

- Blurry tray icon in Windows by GyDi

- Enable context menu in editable element by GyDi

- Save window size and pos in Windows by GyDi

- Optimize traffic graph high CPU usage when hidden by GyDi

- Remove fallback group select status, close #659 by GyDi

- Error boundary with key by GyDi

- Connections is null by GyDi

- Font family not works in some interfaces, close #639 by GyDi

- EncodeURIComponent secret by GyDi

- Encode controller secret, close #601 by GyDi

- Linux not change icon by GyDi

- Try fix blank error by GyDi

- Close all connections when change mode by GyDi

- Macos not change icon by GyDi

- Error message null by GyDi

- Profile data undefined error, close #566 by GyDi

- Import url error (#543) by yettera765

- Linux DEFAULT_BYPASS (#503) by Mr-Spade

- Open file with vscode by GyDi

- Do not render div as a descendant of p (#494) by Tatius Titus

- Use replace instead by GyDi

- Escape path space by GyDi

- Escape the space in path (#451) by John Smith

- Add target os linux by GyDi

- Appimage path unwrap panic by GyDi

- Remove esc key listener in macOS by GyDi

- Adjust style by GyDi

- Adjust swr option by GyDi

- Infinite retry when websocket error by GyDi

- Type error by GyDi

- Do not parse log except the clash core by GyDi

- Field sort for filter by GyDi

- Add meta fields by GyDi

- Runtime config user select by GyDi

- App_handle as_ref by GyDi

- Use crate by GyDi

- Appimage auto launch, close #403 by GyDi

- Compatible with UTF8 BOM, close #283 by GyDi

- Use selected proxy after profile changed by GyDi

- Error log by GyDi

- Adjust fields order by GyDi

- Add meta fields by GyDi

- Add os platform value by GyDi

- Reconnect traffic websocket by GyDi

- Parse bytes precision, close #334 by GyDi

- Trigger new profile dialog, close #356 by GyDi

- Parse log cause panic by GyDi

- Avoid setting login item repeatedly, close #326 by GyDi

- Adjust code by GyDi

- Adjust delay check concurrency by GyDi

- Change default column to auto by GyDi

- Change default app version by GyDi

- Adjust rule ui by GyDi

- Adjust log ui by GyDi

- Keep delay data by GyDi

- Use list item button by GyDi

- Proxy item style by GyDi

- Virtuoso no work in legacy browsers (#318) by MoeShin

- Adjust ui by GyDi

- Refresh websocket by GyDi

- Adjust ui by GyDi

- Parse bytes base 1024 by GyDi

- Add clash fields by GyDi

- Direct mode hide proxies by GyDi

- Profile can not edit by GyDi

- Parse logger time by GyDi

- Adjust service mode ui by GyDi

- Adjust style by GyDi

- Check hotkey and optimize hotkey input, close #287 by GyDi

- Mutex dead lock by GyDi

- Adjust item ui by GyDi

- Regenerate config before change core by GyDi

- Close connections when profile change by GyDi

- Lint by GyDi

- Windows service mode by GyDi

- Init config file by GyDi

- Service mode error and fallback to sidecar by GyDi

- Service mode viewer ui by GyDi

- Create theme error, close #294 by GyDi

- MatchMedia().addEventListener #258 (#296) by MoeShin

- Check config by GyDi

- Show global when no rule groups by GyDi

- Service viewer ref by GyDi

- Service ref error by GyDi

- Group proxies render list is null by GyDi

- Pretty bytes by GyDi

- Use verge hook by GyDi

- Adjust notice by GyDi

- Windows issue by GyDi

- Change dev log level by GyDi

- Patch clash config by GyDi

- Cmds params by GyDi

- Adjust singleton detect by GyDi

- Change template by GyDi

- Copy resource file by GyDi

- MediaQueryList addEventListener polyfill by GyDi

- Change default tun dns-hijack by GyDi

- Something by GyDi

- Provider proxy sort by delay by GyDi

- Profile item menu ui dense by GyDi

- Disable auto scroll to proxy by GyDi

- Check remote profile by GyDi

- Remove smoother by GyDi

- Icon button color by GyDi

- Init system proxy correctly by GyDi

- Open file by GyDi

- Reset proxy by GyDi

- Init config error by GyDi

- Adjust reset proxy by GyDi

- Adjust code by GyDi

- Add https proxy by GyDi

- Auto scroll into view when sorted proxies changed by GyDi

- Refresh proxies interval, close #235 by GyDi

- Style by GyDi

- Fetch profile with system proxy, close #249 by GyDi

- The profile is replaced when the request fails. (#246) by LooSheng

- Default dns config by GyDi

- Kill clash when exit in service mode, close #241 by GyDi

- Icon button color inherit by GyDi

- App version to string by GyDi

- Break loop when core terminated by GyDi

- Api error handle by GyDi

- Clash meta not load geoip, close #212 by GyDi

- Sort proxy during loading, close #221 by GyDi

- Not create windows when enable slient start by GyDi

- Root background color by GyDi

- Create window correctly by GyDi

- Set_activation_policy by GyDi

- Disable spell check by GyDi

- Adjust init launch on dev by GyDi

- Ignore disable auto launch error by GyDi

- I18n by GyDi

- Style by GyDi

- Save enable log on localstorage by GyDi

- Typo in api.ts (#207) by Priestch

- Refresh clash ui await patch by GyDi

- Remove dead code by GyDi

- Style by GyDi

- Handle is none by GyDi

- Unused by GyDi

- Style by GyDi

- Windows logo size by GyDi

- Do not kill sidecar during updating by GyDi

- Delay update config by GyDi

- Reduce logo size by GyDi

- Window center by GyDi

- Log level warn value by GyDi

- Increase delay checker concurrency by GyDi

- External controller allow lan by GyDi

- Remove useless optimizations by GyDi

- Reduce unsafe unwrap by GyDi

- Timer restore at app launch by FoundTheWOUT

- Adjust log text by GyDi

- Only script profile can display console by GyDi

- Fill button title attr by GyDi

- Do not reset system proxy when consistent by GyDi

- Adjust web ui item style by GyDi

- Clash field state error by GyDi

- Badge color error by GyDi

- Web ui port value error by GyDi

- Delay show window by GyDi

- Adjust dialog action button variant by GyDi

- Script code error by GyDi

- Script exception handle by GyDi

- Change fields by GyDi

- Silent start (#150) by FoundTheWOUT

- Save profile when update by GyDi

- List compare wrong by GyDi

- Button color by GyDi

- Limit theme mode value by GyDi

- Add valid clash field by GyDi

- Icon style by GyDi

- Reduce unwrap by GyDi

- Import mod by GyDi

- Add tray separator by GyDi

- Instantiate core after init app, close #122 by GyDi

- Rm macOS transition props by GyDi

- Improve external-controller parse and log by GyDi

- Show windows on click by GyDi

- Adjust update profile notice error by GyDi

- Style issue on mac by GyDi

- Check script run on all OS by FoundTheWOUT

- MacOS disable transparent by GyDi

- Window transparent and can not get hwnd by GyDi

- Create main window by GyDi

- Adjust notice by GyDi

- Label text by GyDi

- Icon path by GyDi

- Icon issue by GyDi

- Notice ui blocking by GyDi

- Service mode error by GyDi

- Win11 drag lag by GyDi

- Rm unwrap by GyDi

- Edit profile info by GyDi

- Change window default size by GyDi

- Change service installer and uninstaller by GyDi

- Adjust connection scroll by GyDi

- Adjust something by GyDi

- Adjust debounce wait time by GyDi

- Adjust dns config by GyDi

- Traffic graph adapt to different fps by GyDi

- Optimize clash launch by GyDi

- Reset after exit by GyDi

- Adjust code by GyDi

- Adjust log by GyDi

- Check button hover style by GyDi

- Icon button color inherit by GyDi

- Remove the lonely zero by GyDi

- I18n add value by GyDi

- Proxy page first render by GyDi

- Console warning by GyDi

- Icon button title by GyDi

- MacOS transition flickers close #47 by GyDi

- Csp image data by GyDi

- Close dialog after save by GyDi

- Change to deep copy by GyDi

- Window style close #45 by GyDi

- Manage global proxy correctly by GyDi

- Tauri csp by GyDi

- Windows style by GyDi

- Update state by GyDi

- Profile item loading state by GyDi

- Adjust windows style by GyDi

- Change mixed port error by GyDi

- Auto launch path by GyDi

- Tun mode config by GyDi

- Adjsut open cmd error by GyDi

- Parse external-controller by GyDi

- Config file case close #18 by GyDi

- Patch item option by GyDi

- User agent not works by GyDi

- External-controller by GyDi

- Change proxy bypass on mac by GyDi

- Kill sidecars after install still in test by GyDi

- Log some error by GyDi

- Apply_blur parameter by GyDi

- Limit enhanced profile range by GyDi

- Profile updated field by GyDi

- Profile field check by GyDi

- Create dir panic by GyDi

- Only error when selected by GyDi

- Enhanced profile consistency by GyDi

- Simply compatible with proxy providers by GyDi

- Component warning by GyDi

- When updater failed by GyDi

- Log file by GyDi

- Result by GyDi

- Cover profile extra by GyDi

- Display menu only on macos by GyDi

- Proxy global showType by GyDi

- Use full clash config by GyDi

- Reconnect websocket when restart clash by GyDi

- Wrong exe path by GyDi

- Patch verge config by GyDi

- Fetch profile panic by GyDi

- Spawn command by GyDi

- Import error by GyDi

- Not open file when new profile by GyDi

- Reset value correctly by GyDi

- Something by GyDi

- Menu without fragment by GyDi

- Proxy list error by GyDi

- Something by GyDi

- Macos auto launch fail by GyDi

- Type error by GyDi

- Restart clash should update something by GyDi

- Script error... by GyDi

- Tag error by GyDi

- Script error by GyDi

- Remove cargo test by GyDi

- Reduce proxy item height by GyDi

- Put profile request with no proxy by GyDi

- Ci strategy by GyDi

- Version update error by GyDi

- Text by GyDi

- Update profile after restart clash by GyDi

- Get proxies multiple times by GyDi

- Delete profile item command by GyDi

- Initialize profiles state by GyDi

- Item header bgcolor by GyDi

- Null type error by GyDi

- Api loading delay by GyDi

- Mutate at the same time may be wrong by GyDi

- Port value not rerender by GyDi

- Change log file format by GyDi

- Proxy bypass add <local> by GyDi

- Sidecar dir by GyDi

- Web resource outDir by GyDi

- Use io by GyDi

### 💅 Styling

- Resolve formatting problem by limsanity

### 📚 Documentation

- Fix img width by GyDi

- Update by GyDi

### 🔨 Refactor

- **hotkey:** Use tauri global shortcut by GyDi

- Copy_clash_env by keiko233

- Adjust base components export by GyDi

- Adjust setting dialog component by GyDi

- Done by GyDi

- Adjust all path methods and reduce unwrap by GyDi

- Rm code by GyDi

- Fix by GyDi

- Rm dead code by GyDi

- For windows by GyDi

- Wip by GyDi

- Wip by GyDi

- Wip by GyDi

- Rm update item block_on by GyDi

- Fix by GyDi

- Fix by GyDi

- Wip by GyDi

- Optimize by GyDi

- Ts path alias by GyDi

- Mode manage on tray by GyDi

- Verge by GyDi

- Wip by GyDi

- Mutex by GyDi

- Wip by GyDi

- Proxy head by GyDi

- Update profile menu by GyDi

- Enhanced mode ui component by GyDi

- Ui theme by GyDi

- Optimize enhance mode strategy by GyDi

- Profile config by GyDi

- Use anyhow to handle error by GyDi

- Rename profiles & command state by GyDi

- Something by GyDi

- Notice caller by GyDi

- Setting page by GyDi

- Rename by GyDi

- Impl structs methods by GyDi

- Impl as struct methods by GyDi

- Api and command by GyDi

- Import profile by GyDi

- Adjust dirs structure by GyDi

---
