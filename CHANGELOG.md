## [2.0.0-rc.3] - 2026-10-23

### 💥 Breaking Changes

- **proxies:** Retain provider summaries in the cache (#5831) by @greenhat616 in [#5831](https://github.com/libnyanpasu/clash-nyanpasu/pull/5831)

### ⚡ Performance Improvements

- **clash:** Share connection frames and recycle rate storage (#5840) by @greenhat616 in [#5840](https://github.com/libnyanpasu/clash-nyanpasu/pull/5840)

- **config:** Share committed runtime effective snapshots (#5835) by @greenhat616 in [#5835](https://github.com/libnyanpasu/clash-nyanpasu/pull/5835)

- **logs:** Skip the scroll corrections while following the newest log (#5902) by @greenhat616 in [#5902](https://github.com/libnyanpasu/clash-nyanpasu/pull/5902)

- **router:** Stop a leaving page from rendering again during its slide (#5900) by @greenhat616 in [#5900](https://github.com/libnyanpasu/clash-nyanpasu/pull/5900)

- **runtime:** Enable native websocket text decoding (#5839) by @greenhat616 in [#5839](https://github.com/libnyanpasu/clash-nyanpasu/pull/5839)

- **runtime:** Use derived connection and scalar decoding (#5837) by @greenhat616 in [#5837](https://github.com/libnyanpasu/clash-nyanpasu/pull/5837)

- **snapshot:** Record known deltas and replay removals in order (#5904) by @greenhat616 in [#5904](https://github.com/libnyanpasu/clash-nyanpasu/pull/5904)

- **traffic:** Render a page's rows and map in the pass after it (#5901) by @greenhat616 in [#5901](https://github.com/libnyanpasu/clash-nyanpasu/pull/5901)

- **traffic:** Re-render only what a page switch changes (#5895) by @greenhat616 in [#5895](https://github.com/libnyanpasu/clash-nyanpasu/pull/5895)

- **traffic:** Share one tooltip among the ranking rows (#5893) by @greenhat616 in [#5893](https://github.com/libnyanpasu/clash-nyanpasu/pull/5893)

- **traffic:** Make the ranking listing cheap to open, poll and page (#5808) by @greenhat616 in [#5808](https://github.com/libnyanpasu/clash-nyanpasu/pull/5808)

- **tsdb:** Reuse decode buffers and sum series rows per combination (#5876) by @greenhat616 in [#5876](https://github.com/libnyanpasu/clash-nyanpasu/pull/5876)

- **ui:** Keep the sidebar and segmented button contexts across renders (#5899) by @greenhat616 in [#5899](https://github.com/libnyanpasu/clash-nyanpasu/pull/5899)

- **widget:** Render the macOS widget in software (#5824) by @greenhat616 in [#5824](https://github.com/libnyanpasu/clash-nyanpasu/pull/5824)

- **windows:** Opt the GUI process into the segment heap (#5821) by @greenhat616 in [#5821](https://github.com/libnyanpasu/clash-nyanpasu/pull/5821)

- Cap the shared tokio runtime at four worker threads (#5822) by @greenhat616 in [#5822](https://github.com/libnyanpasu/clash-nyanpasu/pull/5822)

### ✅ Testing

- **perf:** Count renders and report profiles and traces of the page benchmarks (#5915) by @greenhat616 in [#5915](https://github.com/libnyanpasu/clash-nyanpasu/pull/5915)

- **tsdb:** Bound sorted runs only in windows the cursor has passed (#5964) by @greenhat616 in [#5964](https://github.com/libnyanpasu/clash-nyanpasu/pull/5964)

- **tsdb:** Measure the store and record the profile evidence (#5853) by @greenhat616 in [#5853](https://github.com/libnyanpasu/clash-nyanpasu/pull/5853)

- **tsdb:** Fault every step and model every feature together (#5852) by @greenhat616 in [#5852](https://github.com/libnyanpasu/clash-nyanpasu/pull/5852)

### ✨ Features

- **backend:** Add typed local-socket channels for child processes (#5790) by @greenhat616 in [#5790](https://github.com/libnyanpasu/clash-nyanpasu/pull/5790)

- **debug:** Simulate a crash to test the reporter (#5962) by @greenhat616 in [#5962](https://github.com/libnyanpasu/clash-nyanpasu/pull/5962)

- **diagnose:** Collect the environment and redacted logs with progress (#5960) by @greenhat616 in [#5960](https://github.com/libnyanpasu/clash-nyanpasu/pull/5960)

- **dmg:** Add DMG finalization and background icon handling by @keiko233

- **inspect:** Rebuild Inspect as a full-height step browser (#5956) by @keiko233 in [#5956](https://github.com/libnyanpasu/clash-nyanpasu/pull/5956)

- **inspect:** Report the final node and each step's highest log level (#5955) by @keiko233 in [#5955](https://github.com/libnyanpasu/clash-nyanpasu/pull/5955)

- **nsis:** Add sidebar image for installer by @keiko233

- **panics:** Report frames as module offsets and archive Windows symbols (#5943) by @greenhat616 in [#5943](https://github.com/libnyanpasu/clash-nyanpasu/pull/5943)

- **profiles:** Rebuild Global Transform as a full-height tool (#5954) by @keiko233 in [#5954](https://github.com/libnyanpasu/clash-nyanpasu/pull/5954)

- **profiles:** Add profile folders to the Profiles UI (#5953) by @keiko233 in [#5953](https://github.com/libnyanpasu/clash-nyanpasu/pull/5953)

- **profiles:** Add profile folders to the backend (#5951) by @keiko233 in [#5951](https://github.com/libnyanpasu/clash-nyanpasu/pull/5951)

- **profiles:** Group the profiles sidebar in the M3E navigation list (#5950) by @keiko233 in [#5950](https://github.com/libnyanpasu/clash-nyanpasu/pull/5950)

- **profiles:** Confirm the kind and the source of an imported URL (#5932) by @greenhat616 in [#5932](https://github.com/libnyanpasu/clash-nyanpasu/pull/5932)

- **profiles:** Preview a profile URL before importing it (#5931) by @greenhat616 in [#5931](https://github.com/libnyanpasu/clash-nyanpasu/pull/5931)

- **profiles:** Classify a downloaded profile by its headers and content (#5930) by @greenhat616 in [#5930](https://github.com/libnyanpasu/clash-nyanpasu/pull/5930)

- **proxies:** Reorder and group proxy groups by drag and drop (#5975) by @keiko233 in [#5975](https://github.com/libnyanpasu/clash-nyanpasu/pull/5975)

- **proxies:** Show proxy groups in an M3E sidebar with folders (#5974) by @keiko233 in [#5974](https://github.com/libnyanpasu/clash-nyanpasu/pull/5974)

- **proxies:** Store proxy group layouts per profile (#5973) by @keiko233 in [#5973](https://github.com/libnyanpasu/clash-nyanpasu/pull/5973)

- **settings:** Group the settings sidebar and split crowded pages (#5917) by @keiko233 in [#5917](https://github.com/libnyanpasu/clash-nyanpasu/pull/5917)

- **traffic:** Choose the rankings and drawing under the flow and the map (#5918) by @greenhat616 in [#5918](https://github.com/libnyanpasu/clash-nyanpasu/pull/5918)

- **traffic:** Order the traffic pages by how often each view is used (#5807) by @greenhat616 in [#5807](https://github.com/libnyanpasu/clash-nyanpasu/pull/5807)

- **traffic:** Keep a filtered dimension's alternatives in its ranking (#5886) by @greenhat616 in [#5886](https://github.com/libnyanpasu/clash-nyanpasu/pull/5886)

- **traffic:** Import the schema-v3 traffic.redb into the tsdb store (#5875) by @greenhat616 in [#5875](https://github.com/libnyanpasu/clash-nyanpasu/pull/5875)

- **traffic:** Store traffic statistics in nyanpasu-tsdb (#5873) by @greenhat616 in [#5873](https://github.com/libnyanpasu/clash-nyanpasu/pull/5873)

- **tsdb:** Let a host restore its state on open and size its batches (#5874) by @greenhat616 in [#5874](https://github.com/libnyanpasu/clash-nyanpasu/pull/5874)

- **tsdb:** Persist loss accounting and report the store's status (#5851) by @greenhat616 in [#5851](https://github.com/libnyanpasu/clash-nyanpasu/pull/5851)

- **tsdb:** Retire, recover, suspend, close and compact the store (#5850) by @greenhat616 in [#5850](https://github.com/libnyanpasu/clash-nyanpasu/pull/5850)

- **tsdb:** Expire, consolidate and recycle in maintenance rounds (#5848) by @greenhat616 in [#5848](https://github.com/libnyanpasu/clash-nyanpasu/pull/5848)

- **tsdb:** Query series groups and event pages over a snapshot (#5847) by @greenhat616 in [#5847](https://github.com/libnyanpasu/clash-nyanpasu/pull/5847)

- **tsdb:** Write through a bounded catalog, a journal and L0 chunks (#5846) by @greenhat616 in [#5846](https://github.com/libnyanpasu/clash-nyanpasu/pull/5846)

- **tsdb:** Add the crate, chunk format, and memory and redb backends (#5845) by @greenhat616 in [#5845](https://github.com/libnyanpasu/clash-nyanpasu/pull/5845)

- **updater:** Show GitHub-flavored release notes for every skipped version (#5832) by @greenhat616 in [#5832](https://github.com/libnyanpasu/clash-nyanpasu/pull/5832)

- **widget:** Switch the statistic widget logo (#5828) by @greenhat616 in [#5828](https://github.com/libnyanpasu/clash-nyanpasu/pull/5828)

- **widget:** Launch the Slint statistic widget from the app (#5797) by @greenhat616 in [#5797](https://github.com/libnyanpasu/clash-nyanpasu/pull/5797)

- **widget:** Add Slint statistic widget crate (#5795) by @greenhat616 in [#5795](https://github.com/libnyanpasu/clash-nyanpasu/pull/5795)

- Diagnose for issue reports and crashes (#5961) by @greenhat616 in [#5961](https://github.com/libnyanpasu/clash-nyanpasu/pull/5961)

- Show fatal panics in a Slint crash reporter (#5945) by @greenhat616 in [#5945](https://github.com/libnyanpasu/clash-nyanpasu/pull/5945)

- Keep meow-cli from Meow archives and hand its skill to agents (#5934) by @gengjiawen in [#5934](https://github.com/libnyanpasu/clash-nyanpasu/pull/5934)

- Replace the UWP loopback tool with libnyanpasu/enable-loopback (#5867) by @greenhat616 in [#5867](https://github.com/libnyanpasu/clash-nyanpasu/pull/5867)

- Mark Clash Premium as deprecated in the GUI (#5881) by @greenhat616 in [#5881](https://github.com/libnyanpasu/clash-nyanpasu/pull/5881)

- Show open source credits in about settings (#5878) by @greenhat616 in [#5878](https://github.com/libnyanpasu/clash-nyanpasu/pull/5878)

- 2.0 release UI tweaks (#5810) by @keiko233 in [#5810](https://github.com/libnyanpasu/clash-nyanpasu/pull/5810)

### 🐛 Bug Fixes

- **app:** Start the async runtime only for the GUI (#5796) by @greenhat616 in [#5796](https://github.com/libnyanpasu/clash-nyanpasu/pull/5796)

- **artifacts:** Skip symbols in central publication inventory (#5957) by @keiko233 in [#5957](https://github.com/libnyanpasu/clash-nyanpasu/pull/5957)

- **ci:** Remove failing macOS dmg layout finalization by @keiko233

- **ci:** Generate Finder layout metadata for macOS dmg by @keiko233

- **deps:** Unpin tokio now that 1.54.1 fixes the timer abort (#5926) by @greenhat616 in [#5926](https://github.com/libnyanpasu/clash-nyanpasu/pull/5926)

- **deps:** Pin tokio to 1.53 to avoid a timer abort (#5885) by @greenhat616 in [#5885](https://github.com/libnyanpasu/clash-nyanpasu/pull/5885)

- **l10n:** Correct some words in simpchinese translations (#5826) by @3gf8jv4dv in [#5826](https://github.com/libnyanpasu/clash-nyanpasu/pull/5826)

- **linux:** Remove native window titlebars by default by @keiko233

- **local-channel:** Build without warnings when the tokio feature is off (#5803) by @greenhat616 in [#5803](https://github.com/libnyanpasu/clash-nyanpasu/pull/5803)

- **profiles:** Name the profile in a change that does not commit (#5936) by @greenhat616 in [#5936](https://github.com/libnyanpasu/clash-nyanpasu/pull/5936)

- **router:** Keep a leaving page on its own route during the slide (#5894) by @greenhat616 in [#5894](https://github.com/libnyanpasu/clash-nyanpasu/pull/5894)

- **runtime:** Keep unmanaged fields the field filter drops (#5907) by @greenhat616 in [#5907](https://github.com/libnyanpasu/clash-nyanpasu/pull/5907)

- **service:** Require service 2.0.1 (#5971) by @greenhat616 in [#5971](https://github.com/libnyanpasu/clash-nyanpasu/pull/5971)

- **settings:** Close the latency test dialog after applying (#5811) by @greenhat616 in [#5811](https://github.com/libnyanpasu/clash-nyanpasu/pull/5811)

- **styles:** Scope Tailwind scanning to the app directory by @keiko233

- **traffic:** Keep a card ranking row at its declared height (#5925) by @greenhat616 in [#5925](https://github.com/libnyanpasu/clash-nyanpasu/pull/5925)

- **traffic:** Pass owned frames in store test by @keiko233

- **tsdb:** Grow a short last page before starting another (#5942) by @greenhat616 in [#5942](https://github.com/libnyanpasu/clash-nyanpasu/pull/5942)

- **tun:** Keep the adapter DNS when Meow TUN is turned off on Windows (#5916) by @greenhat616 in [#5916](https://github.com/libnyanpasu/clash-nyanpasu/pull/5916)

- **tun:** Make auto-route independently manageable (#5909) by @greenhat616 in [#5909](https://github.com/libnyanpasu/clash-nyanpasu/pull/5909)

- **ui:** Stop narrow widget headers from flickering (#5780) by @keiko233 in [#5780](https://github.com/libnyanpasu/clash-nyanpasu/pull/5780)

- **update:** Follow installed channel after Nightly; redesign About update UI (#5779) by @keiko233 in [#5779](https://github.com/libnyanpasu/clash-nyanpasu/pull/5779)

- **window:** Hide the webview while its window is out of sight (#5817) by @greenhat616 in [#5817](https://github.com/libnyanpasu/clash-nyanpasu/pull/5817)

- Update import path for settings card in crash simulation module by @keiko233

- Log logos, scripts and modules as previews, not in full (#5941) by @greenhat616 in [#5941](https://github.com/libnyanpasu/clash-nyanpasu/pull/5941)

- Publish core agent skills after install and verify Meow archives (#5939) by @greenhat616 in [#5939](https://github.com/libnyanpasu/clash-nyanpasu/pull/5939)

- Verify sidebar ripple positioning instead of DOM nesting by @keiko233

- Keep scrolling text copies separated by @keiko233

- Prevent native link dragging out of the app by @keiko233

- Scroll overflowing widget library titles by @keiko233

- Hide button content until loading animation exits by @keiko233

- Move release notes to a trailing icon button by @keiko233

- Align profile details with remaining subscription quota (#5865) by @keiko233 in [#5865](https://github.com/libnyanpasu/clash-nyanpasu/pull/5865)

- Polish 2.0 UI interactions and update installation (#5830) by @keiko233 in [#5830](https://github.com/libnyanpasu/clash-nyanpasu/pull/5830)

### 📚 Documentation

- **widget:** Document the widget design and Linux/macOS gaps (#5799) by @greenhat616 in [#5799](https://github.com/libnyanpasu/clash-nyanpasu/pull/5799)

- Refresh README presentation and project overview [skip ci] by @keiko233

- Design the nyanpasu-tsdb storage layer (#5816) by @greenhat616 in [#5816](https://github.com/libnyanpasu/clash-nyanpasu/pull/5816)

### 🔨 Refactor

- **config:** Share config redaction and redact log text (#5959) by @greenhat616 in [#5959](https://github.com/libnyanpasu/clash-nyanpasu/pull/5959)

- **profiles:** Redesign the profile detail page (#5952) by @keiko233 in [#5952](https://github.com/libnyanpasu/clash-nyanpasu/pull/5952)

- **runtime:** Split post-processing into observable steps (#5906) by @greenhat616 in [#5906](https://github.com/libnyanpasu/clash-nyanpasu/pull/5906)

- **runtime:** Describe post-processing writes as ownership trees (#5905) by @greenhat616 in [#5905](https://github.com/libnyanpasu/clash-nyanpasu/pull/5905)

- **tsdb:** Charge every data-sized allocation to a memory account (#5849) by @greenhat616 in [#5849](https://github.com/libnyanpasu/clash-nyanpasu/pull/5849)

- **ui:** Move the M3E navigation list into @nyanpasu/ui (#5949) by @keiko233 in [#5949](https://github.com/libnyanpasu/clash-nyanpasu/pull/5949)

- **widget:** Carry the widget IPC over nyanpasu-local-channel (#5804) by @greenhat616 in [#5804](https://github.com/libnyanpasu/clash-nyanpasu/pull/5804)

- **widget:** Remove the egui statistic widget (#5798) by @greenhat616 in [#5798](https://github.com/libnyanpasu/clash-nyanpasu/pull/5798)

- **widget:** Extract render-free widget protocol crate (#5794) by @greenhat616 in [#5794](https://github.com/libnyanpasu/clash-nyanpasu/pull/5794)

- Extract glass variants for toolbar controls (#5872) by @keiko233 in [#5872](https://github.com/libnyanpasu/clash-nyanpasu/pull/5872)

- Unify empty states with contextual contrast (#5871) by @keiko233 in [#5871](https://github.com/libnyanpasu/clash-nyanpasu/pull/5871)

### 🧹 Miscellaneous Tasks

- **deps:** Bump enable-loopback to v0.1.4 (#5947) by @greenhat616 in [#5947](https://github.com/libnyanpasu/clash-nyanpasu/pull/5947)

- **deps:** Update nyanpasu-runtime submodule to v2.0.0 by @keiko233

---

## New Contributors

- @gengjiawen made their first contribution in [#5934](https://github.com/libnyanpasu/clash-nyanpasu/pull/5934)

**Full Changelog**: https://github.com/libnyanpasu/clash-nyanpasu/compare/v2.0.0-rc.2...v2.0.0-rc.3

## [2.0.0-rc.2] - 2026-10-14

### 💥 Breaking Changes

- **runtime:** Let the guard own port and socks-port like mixed-port (#5720) by @greenhat616 in [#5720](https://github.com/libnyanpasu/clash-nyanpasu/pull/5720)

### ✨ Features

- **frontend:** Unify data pages with Material You navigation (#5738) by @keiko233 in [#5738](https://github.com/libnyanpasu/clash-nyanpasu/pull/5738)

- **i18n:** Add Japanese UI language (#5768) by @keiko233 in [#5768](https://github.com/libnyanpasu/clash-nyanpasu/pull/5768)

- **profile:** Improve profile transform chain editing (#5765) by @keiko233 in [#5765](https://github.com/libnyanpasu/clash-nyanpasu/pull/5765)

- **profiles:** Refresh profile cards and quick import flow (#5732) by @keiko233 in [#5732](https://github.com/libnyanpasu/clash-nyanpasu/pull/5732)

- **settings:** Refine clash and nyanpasu settings controls (#5774) by @keiko233 in [#5774](https://github.com/libnyanpasu/clash-nyanpasu/pull/5774)

- **traffic:** Organize traffic analysis with sidebar routes (#5763) by @keiko233 in [#5763](https://github.com/libnyanpasu/clash-nyanpasu/pull/5763)

- **tray:** Add tray icon presets and custom icon editor (#5711) by @keiko233 in [#5711](https://github.com/libnyanpasu/clash-nyanpasu/pull/5711)

- Redesign context menu and widget configuration (#5705) by @keiko233 in [#5705](https://github.com/libnyanpasu/clash-nyanpasu/pull/5705)

### 🐛 Bug Fixes

- **dashboard:** Preserve layouts across resize (#5698) by @keiko233 in [#5698](https://github.com/libnyanpasu/clash-nyanpasu/pull/5698)

- **group-summary:** Improve delay display logic and formatting by @keiko233

- **i18n:** Resolve hardcoded 'en' lang attribute on document root (#5753) by @3gf8jv4dv in [#5753](https://github.com/libnyanpasu/clash-nyanpasu/pull/5753)

- **l10n:** Refine english and chinese translations (#5757) by @3gf8jv4dv in [#5757](https://github.com/libnyanpasu/clash-nyanpasu/pull/5757)

- **l10n:** Refine english and chinese translations (#5734) by @3gf8jv4dv in [#5734](https://github.com/libnyanpasu/clash-nyanpasu/pull/5734)

- **scripts:** Refresh alpha sidecars when manifest lags by @keiko233

- **service:** Fit the service detail dialog to the window (#5721) by @greenhat616 in [#5721](https://github.com/libnyanpasu/clash-nyanpasu/pull/5721)

- **tray:** Allow same-origin and data: fetches in the CSP (#5750) by @greenhat616 in [#5750](https://github.com/libnyanpasu/clash-nyanpasu/pull/5750)

- **tray-menu:** Round the webview tray menu corners on macOS (#5770) by @keiko233 in [#5770](https://github.com/libnyanpasu/clash-nyanpasu/pull/5770)

- **ui:** Give virtual lists the scroll viewport once it is attached (#5752) by @greenhat616 in [#5752](https://github.com/libnyanpasu/clash-nyanpasu/pull/5752)

### 📚 Documentation

- Define native CoW target reuse for worktrees by @keiko233

- Improve README formatting for clarityore [skip ci] by @keiko233

### 🔨 Refactor

- **profiles:** Type the sync interval and collapse it when auto update is off (#5767) by @greenhat616 in [#5767](https://github.com/libnyanpasu/clash-nyanpasu/pull/5767)

---

**Full Changelog**: https://github.com/libnyanpasu/clash-nyanpasu/compare/v2.0.0-rc.1...v2.0.0-rc.2

## [2.0.0-rc.1] - 2026-10-09

### 🐛 Bug Fixes

- **ci:** Restore Telegram release notifications (#5692) by @greenhat616 in [#5692](https://github.com/libnyanpasu/clash-nyanpasu/pull/5692)

- **deps:** Restore Windows lockfile resolution after tauri update (#5696) by @greenhat616 in [#5696](https://github.com/libnyanpasu/clash-nyanpasu/pull/5696)

- **window:** Keep the tray menu from taking focus while it is built (#5685) by @greenhat616 in [#5685](https://github.com/libnyanpasu/clash-nyanpasu/pull/5685)

### 📚 Documentation

- Defer OpenWrt MVP until core and shell extraction settles by @keiko233

### 🔨 Refactor

- Consolidate shared runtime into nyanpasu-core by @keiko233

---

**Full Changelog**: https://github.com/libnyanpasu/clash-nyanpasu/compare/v2.0.0-beta.3...v2.0.0-rc.1

## [2.0.0-beta.3] - 2026-10-07

### ⚡ Performance Improvements

- **logging:** Limit the appender buffer to 4096 lines (#5636) by @greenhat616 in [#5636](https://github.com/libnyanpasu/clash-nyanpasu/pull/5636)

- **proxies:** Send only the delay records the frontend reads (#5643) by @greenhat616 in [#5643](https://github.com/libnyanpasu/clash-nyanpasu/pull/5643)

- **proxies:** Hash cache fingerprints without retaining JSON (#5640) by @greenhat616 in [#5640](https://github.com/libnyanpasu/clash-nyanpasu/pull/5640)

- **runtime:** Serialize the runtime document once and share it (#5663) by @greenhat616 in [#5663](https://github.com/libnyanpasu/clash-nyanpasu/pull/5663)

- **runtime:** Keep runtime builds as stored snapshot graphs (#5662) by @greenhat616 in [#5662](https://github.com/libnyanpasu/clash-nyanpasu/pull/5662)

- **runtime:** Share nyanpasu-utils runtime with Tauri (#5638) by @greenhat616 in [#5638](https://github.com/libnyanpasu/clash-nyanpasu/pull/5638)

- **storage:** Increase SourceForge and Telegram upload concurrency by @keiko233

### ✨ Features

- **config:** Record changed fields and restore single snapshot nodes (#5661) by @greenhat616 in [#5661](https://github.com/libnyanpasu/clash-nyanpasu/pull/5661)

- **core:** Add Meow Alpha and fix core version detection (#5612) by @keiko233 in [#5612](https://github.com/libnyanpasu/clash-nyanpasu/pull/5612)

- **migration:** Stamp the app config with the schema revision it holds (#5646) by @greenhat616 in [#5646](https://github.com/libnyanpasu/clash-nyanpasu/pull/5646)

- **profiling:** Add trace-chrome and dhat-heap profiling features (#5628) by @greenhat616 in [#5628](https://github.com/libnyanpasu/clash-nyanpasu/pull/5628)

- **proxies:** Show group availability and chains, add P2 settings and provider health check (#5644) by @greenhat616 in [#5644](https://github.com/libnyanpasu/clash-nyanpasu/pull/5644)

- **proxies:** Search, sort and filter a group's nodes (#5642) by @greenhat616 in [#5642](https://github.com/libnyanpasu/clash-nyanpasu/pull/5642)

- **proxies:** Test latency against each group's URL and a configurable timeout (#5641) by @greenhat616 in [#5641](https://github.com/libnyanpasu/clash-nyanpasu/pull/5641)

- **proxies:** Gate selection on capabilities and manage pinned groups (#5631) by @greenhat616 in [#5631](https://github.com/libnyanpasu/clash-nyanpasu/pull/5631)

- **storage:** Publish Telegram archives independently of updater by @keiko233

- **window:** Let each window close by destroying or hiding (#5649) by @greenhat616 in [#5649](https://github.com/libnyanpasu/clash-nyanpasu/pull/5649)

- Update theme color presets and set default color to #3e64a7 by @keiko233

### 🐛 Bug Fixes

- **ci:** Repair the unit tests failing on main (#5626) by @greenhat616 in [#5626](https://github.com/libnyanpasu/clash-nyanpasu/pull/5626)

- **ci:** Keep updater tests independent of pnpm install by @keiko233

- **proxies:** Keep a group card's chips whole beside a long leaf (#5670) by @greenhat616 in [#5670](https://github.com/libnyanpasu/clash-nyanpasu/pull/5670)

- **proxies:** Close proxies API unification follow-ups (#5627) by @greenhat616 in [#5627](https://github.com/libnyanpasu/clash-nyanpasu/pull/5627)

- **proxies:** Keep the proxy snapshot while a selection runs (#5624) by @greenhat616 in [#5624](https://github.com/libnyanpasu/clash-nyanpasu/pull/5624)

- **runtime:** Let profiles own unified-delay and tcp-concurrent on request (#5669) by @greenhat616 in [#5669](https://github.com/libnyanpasu/clash-nyanpasu/pull/5669)

- **storage:** Use valid SourceForge reservation directories by @keiko233

- **updater:** Sample app update speed over a fixed window (#5667) by @greenhat616 in [#5667](https://github.com/libnyanpasu/clash-nyanpasu/pull/5667)

- **updater:** Match SourceForge asset name variants by @keiko233

### 📚 Documentation

- Specify updater speed and download fallback [skip ci] (#5637) by @greenhat616 in [#5637](https://github.com/libnyanpasu/clash-nyanpasu/pull/5637)

- Record the runtime snapshot memory spec and its dhat comparison (#5664) by @greenhat616 in [#5664](https://github.com/libnyanpasu/clash-nyanpasu/pull/5664)

- Specify a window manager that owns window lifecycle [skip ci] (#5634) by @greenhat616 in [#5634](https://github.com/libnyanpasu/clash-nyanpasu/pull/5634)

- Fix a mistake in new issue template [skip ci] (#5635) by @3gf8jv4dv in [#5635](https://github.com/libnyanpasu/clash-nyanpasu/pull/5635)

- Report the Windows memory profile [skip ci] (#5633) by @greenhat616 in [#5633](https://github.com/libnyanpasu/clash-nyanpasu/pull/5633)

- Specify memory and startup profiling infrastructure [skip ci] (#5620) by @greenhat616 in [#5620](https://github.com/libnyanpasu/clash-nyanpasu/pull/5620)

- Point references at the flattened docs directories [skip ci] by @greenhat616

- Flatten design records into per-kind directories [skip ci] by @greenhat616

- Route agent rules to guides and set the core/GUI split goal (#5618) by @greenhat616 in [#5618](https://github.com/libnyanpasu/clash-nyanpasu/pull/5618)

- Update toolchain requirements in contributing guide by @greenhat616

- Revise contributions in README to clarify requirements [skip ci] (#5615) by @3gf8jv4dv in [#5615](https://github.com/libnyanpasu/clash-nyanpasu/pull/5615)

- Refactor issue template to improve writing experience [skip ci] (#5614) by @3gf8jv4dv in [#5614](https://github.com/libnyanpasu/clash-nyanpasu/pull/5614)

### 🔨 Refactor

- **paths:** Extract frontend-independent path resolution (#5645) by @4o3F in [#5645](https://github.com/libnyanpasu/clash-nyanpasu/pull/5645)

- **settings:** Stop waiting for the core after patching the controller (#5648) by @greenhat616 in [#5648](https://github.com/libnyanpasu/clash-nyanpasu/pull/5648)

- **tray:** Identify proxy menu items by group and node (#5623) by @greenhat616 in [#5623](https://github.com/libnyanpasu/clash-nyanpasu/pull/5623)

- **window:** Position the macOS traffic lights through Tauri (#5651) by @greenhat616 in [#5651](https://github.com/libnyanpasu/clash-nyanpasu/pull/5651)

- **window:** Let a window manager own the window lifecycle (#5647) by @greenhat616 in [#5647](https://github.com/libnyanpasu/clash-nyanpasu/pull/5647)

- Extract neutral runtime application and platform crates (#5652) by @keiko233 in [#5652](https://github.com/libnyanpasu/clash-nyanpasu/pull/5652)

---

**Full Changelog**: https://github.com/libnyanpasu/clash-nyanpasu/compare/v2.0.0-beta.2...v2.0.0-beta.3

## [2.0.0-beta.2] - 2026-10-05

### ⚡ Performance Improvements

- **ci:** Publish storage backends and artifacts concurrently by @keiko233

- **logs:** Drive the viewer from status events (#5588) by @greenhat616 in [#5588](https://github.com/libnyanpasu/clash-nyanpasu/pull/5588)

### ✅ Testing

- **dashboard:** Wait for measured connection rows by @greenhat616

- **http:** Preserve acknowledged server status during polling by @greenhat616

- **ui:** Budget the repeated drawer reopen scenario by @greenhat616

- **ui:** Observe drawer dismissal without timing races by @greenhat616

- **widgets:** Observe real control animations across renders by @greenhat616

### ✨ Features

- **ci:** Publish build archives to Internet Archive and add SourceForge mirrors (#5595) by @keiko233 in [#5595](https://github.com/libnyanpasu/clash-nyanpasu/pull/5595)

- **logs:** Configure Core log rotation and compression live (#5599) by @greenhat616 in [#5599](https://github.com/libnyanpasu/clash-nyanpasu/pull/5599)

- **logs:** Compress records with preset and trained dictionaries (#5587) by @greenhat616 in [#5587](https://github.com/libnyanpasu/clash-nyanpasu/pull/5587)

- **logs:** Rotate session storage within a disk budget (#5586) by @greenhat616 in [#5586](https://github.com/libnyanpasu/clash-nyanpasu/pull/5586)

- Route frontend platform capabilities through RPC (#5608) by @keiko233 in [#5608](https://github.com/libnyanpasu/clash-nyanpasu/pull/5608)

- Report frontend warnings and errors to the application log (#5604) by @greenhat616 in [#5604](https://github.com/libnyanpasu/clash-nyanpasu/pull/5604)

### 🐛 Bug Fixes

- **ci:** Bound and report Internet Archive upload retries by @keiko233

- **ci:** Preserve content length for Internet Archive uploads by @keiko233

- **ci:** Publish updater feeds after successful mirror verification by @keiko233

- **ci:** Diagnose and recover storage uploads without rebuilding by @keiko233

- **ci:** Accept NSIS setup installers in central publication by @keiko233

- **ci:** Resolve the installed Tauri signing CLI by @greenhat616

- **ci:** Validate pull requests targeting stacked branches by @greenhat616

- **ci:** Stop retrying archive quota failures by @greenhat616

- **ci:** Separate archive uploads from release notifications by @keiko233

- **fake-core:** Clear inherited nonblocking mode on HTTP sockets by @greenhat616

- **fake-core:** Consume request bodies before closing HTTP sockets by @greenhat616

- **logs:** Follow the committed core log level (#5585) by @greenhat616 in [#5585](https://github.com/libnyanpasu/clash-nyanpasu/pull/5585)

- **proxy:** Prefer group lists and infer all groups as fallback (#5596) by @greenhat616 in [#5596](https://github.com/libnyanpasu/clash-nyanpasu/pull/5596)

- **proxy:** Preserve inline provider node metadata (#5592) by @4o3F in [#5592](https://github.com/libnyanpasu/clash-nyanpasu/pull/5592)

- **tests:** Observe modal layout animation before it settles by @greenhat616

- **traffic:** Center the merged node's text like the other nodes (#5597) by @greenhat616 in [#5597](https://github.com/libnyanpasu/clash-nyanpasu/pull/5597)

- Preserve selected text in context menus by @keiko233

### 🔨 Refactor

- **proxies:** Serve clash-api records and derive group capabilities (#5605) by @greenhat616 in [#5605](https://github.com/libnyanpasu/clash-nyanpasu/pull/5605)

---

**Full Changelog**: https://github.com/libnyanpasu/clash-nyanpasu/compare/v2.0.0-beta.1...v2.0.0-beta.2

## [2.0.0-beta.1] - 2026-10-03

### 💥 Breaking Changes

- **core:** Move the core lifecycle onto CoreControl (PR-5 C/D-switch) (#5182) by @greenhat616 in [#5182](https://github.com/libnyanpasu/clash-nyanpasu/pull/5182)

- **portable:** Correct app placeholder to clash nyanpasu by @greenhat616

- **profile:** Align specta bindings with serde wire format and PR3 regressions (#4915, #4916, #4917) (#4920) by @greenhat616 in [#4920](https://github.com/libnyanpasu/clash-nyanpasu/pull/4920)

- **profile:** Use serde internal newtype to avoid int type mismatch (#3364) by @greenhat616 in [#3364](https://github.com/libnyanpasu/clash-nyanpasu/pull/3364)

- **tauri:** Runtime derivation cleanup — checked promote pipeline and RebuildOutcome (PR-4) (#4932) by @greenhat616 in [#4932](https://github.com/libnyanpasu/clash-nyanpasu/pull/4932)

- **tauri:** Remove legacy profiles config module and enhance pipeline (#4890) by @greenhat616 in [#4890](https://github.com/libnyanpasu/clash-nyanpasu/pull/4890)

- **tauri:** Switch profiles to domain pipeline (composition root, IPC surface, frontend) (#4889) by @greenhat616 in [#4889](https://github.com/libnyanpasu/clash-nyanpasu/pull/4889)

- Hardware-AES baseline on all distributed targets (introduce gxhash) (#4903) by @greenhat616 in [#4903](https://github.com/libnyanpasu/clash-nyanpasu/pull/4903)

- Replace all domain by @keiko233

- Use reqwest instead of isahc to make sure fully remove openssl by @greenhat616

- Use xcode 16 toolchain because xcode 15 was removed by @greenhat616

- Use generic transaction container impl and use builder based profile item instead (#1663) by @greenhat616 in [#1663](https://github.com/libnyanpasu/clash-nyanpasu/pull/1663)

- Migrate to tauri v2 and do a lot of tweaks (#1510) by @greenhat616 in [#1510](https://github.com/libnyanpasu/clash-nyanpasu/pull/1510)

### ⚡ Performance Improvements

- **connections:** Cut the render cost of connection samples (#5496) by @greenhat616 in [#5496](https://github.com/libnyanpasu/clash-nyanpasu/pull/5496)

- **dashboard:** Render the widgets once the grid is measured (#5507) by @greenhat616 in [#5507](https://github.com/libnyanpasu/clash-nyanpasu/pull/5507)

- **dashboard:** Stop samples and KV reads re-rendering whole widgets (#5499) by @greenhat616 in [#5499](https://github.com/libnyanpasu/clash-nyanpasu/pull/5499)

- **hooks:** Stop setting, system and ws hooks re-rendering needlessly (#5495) by @greenhat616 in [#5495](https://github.com/libnyanpasu/clash-nyanpasu/pull/5495)

- **inspect:** Show a large step diff a thousand lines at a time (#5505) by @greenhat616 in [#5505](https://github.com/libnyanpasu/clash-nyanpasu/pull/5505)

- **inspect:** Cache the highlighted step YAML and skip huge documents (#5511) by @greenhat616 in [#5511](https://github.com/libnyanpasu/clash-nyanpasu/pull/5511)

- **interface:** Split clash ws context per history kind (#5413) by @greenhat616 in [#5413](https://github.com/libnyanpasu/clash-nyanpasu/pull/5413)

- **logging:** Benchmark owned and interned index storage (#5253) by @greenhat616 in [#5253](https://github.com/libnyanpasu/clash-nyanpasu/pull/5253)

- **logs:** Make log rows cheap to mount and skip unchanged rows (#5513) by @greenhat616 in [#5513](https://github.com/libnyanpasu/clash-nyanpasu/pull/5513)

- **logs:** Cut the render cost of the logs pages (#5510) by @greenhat616 in [#5510](https://github.com/libnyanpasu/clash-nyanpasu/pull/5510)

- **profiles:** Animate the active card's gradient only on hover (#5512) by @greenhat616 in [#5512](https://github.com/libnyanpasu/clash-nyanpasu/pull/5512)

- **profiles:** Keep profile and provider data stable on refetch (#5494) by @greenhat616 in [#5494](https://github.com/libnyanpasu/clash-nyanpasu/pull/5494)

- **proxies:** Stop dropping frames when switching proxy groups (#5445) by @greenhat616 in [#5445](https://github.com/libnyanpasu/clash-nyanpasu/pull/5445)

- **proxies:** Send each proxy node once and reference it by name (#5430) by @greenhat616 in [#5430](https://github.com/libnyanpasu/clash-nyanpasu/pull/5430)

- **rules:** Cut the render cost of the rules page (#5497) by @greenhat616 in [#5497](https://github.com/libnyanpasu/clash-nyanpasu/pull/5497)

- **settings:** Mount the lower settings sections in a deferred render (#5508) by @greenhat616 in [#5508](https://github.com/libnyanpasu/clash-nyanpasu/pull/5508)

- **settings:** Load shiki lazily and read core versions once (#5498) by @greenhat616 in [#5498](https://github.com/libnyanpasu/clash-nyanpasu/pull/5498)

- **streams:** Send connection details only to pages that subscribe (#5429) by @greenhat616 in [#5429](https://github.com/libnyanpasu/clash-nyanpasu/pull/5429)

- **styles:** Keep Tailwind's space and divide selectors off WebKit's slow path (#5448) by @greenhat616 in [#5448](https://github.com/libnyanpasu/clash-nyanpasu/pull/5448)

- **theme:** Derive the theme from the color instead of on every render (#5506) by @greenhat616 in [#5506](https://github.com/libnyanpasu/clash-nyanpasu/pull/5506)

- **ui:** Measure modal layout only when it opens or closes (#5502) by @greenhat616 in [#5502](https://github.com/libnyanpasu/clash-nyanpasu/pull/5502)

- **ui:** Scroll the sparkline on the compositor (#5504) by @greenhat616 in [#5504](https://github.com/libnyanpasu/clash-nyanpasu/pull/5504)

- **ui:** Measure the tab indicator only when it moves (#5503) by @greenhat616 in [#5503](https://github.com/libnyanpasu/clash-nyanpasu/pull/5503)

- **ui:** Run the logo's idle sway on the compositor (#5501) by @greenhat616 in [#5501](https://github.com/libnyanpasu/clash-nyanpasu/pull/5501)

- **ui:** Run the text marquee on the compositor (#5500) by @greenhat616 in [#5500](https://github.com/libnyanpasu/clash-nyanpasu/pull/5500)

- **ui:** Cut shell re-renders on scroll, resize and proxies updates (#5492) by @greenhat616 in [#5492](https://github.com/libnyanpasu/clash-nyanpasu/pull/5492)

- **ui:** Reduce jank on route transitions, proxies and connections (#5423) by @greenhat616 in [#5423](https://github.com/libnyanpasu/clash-nyanpasu/pull/5423)

### ✅ Testing

- **boa:** Add js engine text module test by @keiko233

- **effects:** Inject a nightly build for rejected channel changes by @keiko233

- **enhance:** Add mapping reorder sample by @greenhat616

- **frontend:** Make two render-count browser tests wait for real events by @greenhat616

- **frontend:** Migrate regression suites to Vitest (#5311) by @greenhat616 in [#5311](https://github.com/libnyanpasu/clash-nyanpasu/pull/5311)

- **logging:** Record viewer performance against the released runtime (#5256) by @greenhat616 in [#5256](https://github.com/libnyanpasu/clash-nyanpasu/pull/5256)

- **perf:** Add on-demand page benchmarks (#5491) by @greenhat616 in [#5491](https://github.com/libnyanpasu/clash-nyanpasu/pull/5491)

- **ui:** Wait for the marquee's first resize before counting reads (#5575) by @greenhat616 in [#5575](https://github.com/libnyanpasu/clash-nyanpasu/pull/5575)

- **ui:** Wait for the marquee's mount measurements before spying (#5535) by @greenhat616 in [#5535](https://github.com/libnyanpasu/clash-nyanpasu/pull/5535)

- **ui:** Control time in scroll and tab animation assertions by @keiko233

- Add enhance order test by @greenhat616

- Disable lib doctest by @greenhat616

- Ignore dir placeholder test by @greenhat616

### ✨ Features

- **action-card:** Improve layout and conditional rendering for action buttons by @keiko233

- **animated-item:** Add AnimatedItem component and integrate into SettingsCard by @keiko233

- **animated-outlet:** Add AnimatedOutlet and AnimatedOutletPreset components by @keiko233

- **block-task:** Implement BlockTaskProvider and related hooks for managing asynchronous tasks by @keiko233

- **breakpoint:** Add container breakpoint hooks for responsive design by @keiko233

- **build-cache:** Add Deno build cache management for workflows by @keiko233

- **button:** Enhance button variants with new styles and disabled states by @keiko233

- **cache-client:** Download progress logging by @keiko233

- **cache-client:** Include chunkMultiplier in cache upload session initialization by @keiko233

- **card:** Implement Card component with variants for header, footer, and content by @keiko233

- **chain-editor:** Draft impl proxy profile chain manager by @keiko233

- **ci:** Publish beta prereleases and fix the beta update path (#5532) by @greenhat616 in [#5532](https://github.com/libnyanpasu/clash-nyanpasu/pull/5532)

- **clash-api:** Migrate WebSocket callers to owned stream actors (#5220) by @greenhat616 in [#5220](https://github.com/libnyanpasu/clash-nyanpasu/pull/5220)

- **clash-settings:** Add UI components for Clash settings by @keiko233

- **color:** Draft impl get system accent color for theme (#4632) by @keiko233 in [#4632](https://github.com/libnyanpasu/clash-nyanpasu/pull/4632)

- **config:** Converge source mutations through selective TCC (#5367) by @greenhat616 in [#5367](https://github.com/libnyanpasu/clash-nyanpasu/pull/5367)

- **connections:** Filter connections by traffic dimensions and range (#5564) by @greenhat616 in [#5564](https://github.com/libnyanpasu/clash-nyanpasu/pull/5564)

- **connections:** Add closed connections tab and column settings (#5486) by @greenhat616 in [#5486](https://github.com/libnyanpasu/clash-nyanpasu/pull/5486)

- **connections:** Draft impl connection page by @keiko233

- **connections:** Implement deleteConnections mutation and update usage in components by @keiko233

- **connections:** Add conection detail dialog by @greenhat616

- **connections:** Add conection total info by @greenhat616

- **connections:** Add a column filter dialog by @greenhat616

- **consts:** Add OS detection constants for Windows, macOS, and Linux by @keiko233

- **context-menu:** Update MotionContent animetion by @keiko233

- **context-menu:** Implement context menu registration and trigger components by @keiko233

- **context-menu:** Implement context menu by @keiko233

- **core:** Enforce profile and proxy interruption policies (#5250) by @greenhat616 in [#5250](https://github.com/libnyanpasu/clash-nyanpasu/pull/5250)

- **core:** Configure channels and recover applied snapshot inspection (#5227) by @greenhat616 in [#5227](https://github.com/libnyanpasu/clash-nyanpasu/pull/5227)

- **core:** Introduce revocable instance-bound Clash API access (#5203) by @greenhat616 in [#5203](https://github.com/libnyanpasu/clash-nyanpasu/pull/5203)

- **core:** Add CoreActor v2, the ServiceActor, and the DNS wiring (PR-C/D-new, unwired) (#5116) by @greenhat616 in [#5116](https://github.com/libnyanpasu/clash-nyanpasu/pull/5116)

- **css:** Add CSS worker support and improve CSS completion logic by @keiko233

- **custom-css:** Draft impl custom spec (#4574) by @keiko233 in [#4574](https://github.com/libnyanpasu/clash-nyanpasu/pull/4574)

- **dashboard:** Add daily widgets and improve widget library (#5566) by @keiko233 in [#5566](https://github.com/libnyanpasu/clash-nyanpasu/pull/5566)

- **dashboard:** Add per-widget configuration by @keiko233

- **dashboard:** Draft impl dashboard page widgets (#4353) by @keiko233 in [#4353](https://github.com/libnyanpasu/clash-nyanpasu/pull/4353)

- **debug:** Add Block Task Viewer component to settings debug page by @keiko233

- **dropdown-menu:** Add width prop to DropdownMenu and MotionContent for better layout control by @keiko233

- **dropdown-menu:** Implement DropdownMenu component with context and animations by @keiko233

- **effects:** Reconcile application effects after typed config commits (#5267) by @greenhat616 in [#5267](https://github.com/libnyanpasu/clash-nyanpasu/pull/5267)

- **effects:** Add pure application effect plan and status protocol (#5266) by @greenhat616 in [#5266](https://github.com/libnyanpasu/clash-nyanpasu/pull/5266)

- **enhance:** Add include-all field support for proxy groups (#3665) by @Slinetrac in [#3665](https://github.com/libnyanpasu/clash-nyanpasu/pull/3665)

- **error-page:** Add reload & close button by @keiko233

- **event:** Add React app mounted event and integrate into IPC by @keiko233

- **frontend:** Open filter chips that do not fit in a dialog (#5573) by @greenhat616 in [#5573](https://github.com/libnyanpasu/clash-nyanpasu/pull/5573)

- **frontend:** Jump between rules, connections and traffic (#5565) by @greenhat616 in [#5565](https://github.com/libnyanpasu/clash-nyanpasu/pull/5565)

- **frontend:** Offer a way back after jumping between pages (#5563) by @greenhat616 in [#5563](https://github.com/libnyanpasu/clash-nyanpasu/pull/5563)

- **geodata:** Add lookups over the core's geo databases (#5537) by @greenhat616 in [#5537](https://github.com/libnyanpasu/clash-nyanpasu/pull/5537)

- **header:** Implement platform-specific headers with new DefaultHeader and MacOSHeader components by @keiko233

- **header:** Draft impl header actions (#4076) by @keiko233 in [#4076](https://github.com/libnyanpasu/clash-nyanpasu/pull/4076)

- **hooks:** Add useIsMobile hook to determine mobile view based on breakpoint by @keiko233

- **hooks:** Add useLockFn hook to prevent concurrent execution of async functions by @keiko233

- **hotkey:** Own shortcut registration in a typed actor (#5269) by @greenhat616 in [#5269](https://github.com/libnyanpasu/clash-nyanpasu/pull/5269)

- **hotlkey:** Add hotkey management functionality and localization support by @keiko233

- **i18n:** Select UI language from the system locale (#5228) by @greenhat616 in [#5228](https://github.com/libnyanpasu/clash-nyanpasu/pull/5228)

- **i18n:** Add Korean (ko) translation (#4923) by @moduvoice in [#4923](https://github.com/libnyanpasu/clash-nyanpasu/pull/4923)

- **i18n:** Integrate @inlang/paraglide-js for internationalization support by @keiko233

- **i18n:** Translate time units and dialog buttons (#2530) by @3gf8jv4dv in [#2530](https://github.com/libnyanpasu/clash-nyanpasu/pull/2530)

- **i18n:** Impl zh-tw language (#2431) by @3gf8jv4dv in [#2431](https://github.com/libnyanpasu/clash-nyanpasu/pull/2431)

- **image:** Add Image component for optimized icon rendering in ProxiesNavigate by @keiko233

- **input:** Implement Input component with NumericInput support by @keiko233

- **ipc:** Send command errors as a structured payload with copyable details (#5431) by @greenhat616 in [#5431](https://github.com/libnyanpasu/clash-nyanpasu/pull/5431)

- **ipc:** Migrate hooks to tauri-specta-query by @keiko233

- **ipc:** Refactor clash ws hooks by @keiko233

- **ipc:** Add useClashCores hook for managing Clash core details and versions by @keiko233

- **ipc:** Add useProxyMode hook for managing proxy modes in Clash configuration by @keiko233

- **ipc:** Add useSystemService hook with upsert mutation for service management by @keiko233

- **ipc:** Add useClashInfo hook for retrieving clash information by @keiko233

- **ipc:** Add useClashConfig hook for managing Clash configuration by @keiko233

- **ipc:** Update profile types and refactor hook useage by @keiko233

- **ipc:** Enhance useProfile hook optional helper functions by @keiko233

- **ipc:** Export new hooks by @keiko233

- **ipc:** Add settings hook by @keiko233

- **ipc:** Add useRuntimeProfile hook by @keiko233

- **ipc:** Enhance useProfile hook to include helper functions by @keiko233

- **ipc:** Add profile hooks via tanstack query by @keiko233

- **jobs:** Add task events rotate job by @greenhat616

- **js-runner:** Support json, raw text module import by @greenhat616

- **l10n:** Translate connection table columns and detail fields by @greenhat616

- **language-provider:** Sync dayjs locale with selected language by @keiko233

- **language-provider:** Add LanguageProvider component for managing application language context by @keiko233

- **layout:** Fixed navigation bar style by @keiko233

- **lifecycle:** Reconcile at startup and shut down in one ordered run (#5387) by @greenhat616 in [#5387](https://github.com/libnyanpasu/clash-nyanpasu/pull/5387)

- **log-file-config:** Add log file configuration component by @keiko233

- **logging:** Rotate app logs by size and local day (#5409) by @greenhat616 in [#5409](https://github.com/libnyanpasu/clash-nyanpasu/pull/5409)

- **logging:** Expose app and service logs through the client facade (#5254) by @greenhat616 in [#5254](https://github.com/libnyanpasu/clash-nyanpasu/pull/5254)

- **logging:** Draft a logging indexer for nyanpasu logs by @greenhat616

- **logo:** Tweaks AnimatedLogo animations with indeterminate state by @keiko233

- **logo:** Add animated logo component with shake effect and update header to use it by @keiko233

- **logs:** Style date filters with Material You controls (#5261) by @greenhat616 in [#5261](https://github.com/libnyanpasu/clash-nyanpasu/pull/5261)

- **logs:** Highlight JSON and clarify live following (#5260) by @greenhat616 in [#5260](https://github.com/libnyanpasu/clash-nyanpasu/pull/5260)

- **logs:** Compact controls and load history on scroll (#5259) by @greenhat616 in [#5259](https://github.com/libnyanpasu/clash-nyanpasu/pull/5259)

- **logs:** Unify viewer presentation with Material You components (#5258) by @greenhat616 in [#5258](https://github.com/libnyanpasu/clash-nyanpasu/pull/5258)

- **logs:** Show application and service files in the viewer (#5255) by @greenhat616 in [#5255](https://github.com/libnyanpasu/clash-nyanpasu/pull/5255)

- **logs:** Add clear logs action by @keiko233

- **logs:** Draft impl new logs viewer by @keiko233

- **logs:** Implement search functionality with highlighted text in logs by @keiko233

- **logs:** Implement virtualized log display by @keiko233

- **migration:** Stamp profiles.yaml with the schema revision it holds (#5364) by @greenhat616 in [#5364](https://github.com/libnyanpasu/clash-nyanpasu/pull/5364)

- **migration:** Add profiles revision 3 clean-schema migration (#4899) by @greenhat616 in [#4899](https://github.com/libnyanpasu/clash-nyanpasu/pull/4899)

- **migration:** Split legacy config into typed state files by @4o3F

- **migration:** Open migration log file while migrate failed by @greenhat616

- **modal:** Implement modal component using @radix-ui/react-dialog by @keiko233

- **navbar:** Add "More" label and enhance mobile navbar with dropdown menu by @keiko233

- **navbar:** Refactor navbar component to use dynamic button rendering by @keiko233

- **navbar:** Integrate Tooltip component into Navbar by @keiko233

- **network-statistic-widget:** Add configuration ui by @keiko233

- **nsis:** Add zh-tw language for installer (#2517) by @3gf8jv4dv in [#2517](https://github.com/libnyanpasu/clash-nyanpasu/pull/2517)

- **pac:** Add PAC proxy support with download, validation, and fallback (#3724) by @Slinetrac in [#3724](https://github.com/libnyanpasu/clash-nyanpasu/pull/3724)

- **profile:** Draft impl edtior window (#4036) by @keiko233 in [#4036](https://github.com/libnyanpasu/clash-nyanpasu/pull/4036)

- **profiles:** Show sync status and persisted run history (#5459) by @greenhat616 in [#5459](https://github.com/libnyanpasu/clash-nyanpasu/pull/5459)

- **profiles:** Display runtime snapshots in Profile Inspect (#5222) by @greenhat616 in [#5222](https://github.com/libnyanpasu/clash-nyanpasu/pull/5222)

- **profiles:** Implement drag-and-drop functionality for profile sorting by @keiko233

- **profiles:** Integrate @paper-design/shaders-react for visual effect by @keiko233

- **profiles:** Implement profiles context menu by @keiko233

- **profiles:** Draft implement experimental profiles page (#3987) by @keiko233 in [#3987](https://github.com/libnyanpasu/clash-nyanpasu/pull/3987)

- **profiles:** Global update button by @greenhat616

- **progress:** Add linear progress component with indeterminate animation by @keiko233

- **providers:** New providers page (#4329) by @keiko233 in [#4329](https://github.com/libnyanpasu/clash-nyanpasu/pull/4329)

- **proxies:** Show group type and selected node latency (#5302) by @greenhat616 in [#5302](https://github.com/libnyanpasu/clash-nyanpasu/pull/5302)

- **proxies:** Show recent latency history in node cards (#5247) by @greenhat616 in [#5247](https://github.com/libnyanpasu/clash-nyanpasu/pull/5247)

- **proxies:** Add no proxies found message by @keiko233

- **proxies:** Add connection speeds for group header component by @keiko233

- **proxies:** Add proxies group delay test button by @keiko233

- **proxies:** Draft implement proxies routes and components for proxy management by @keiko233

- **proxies:** Add useClashProxiesProvider hook and related query key for managing proxies by @keiko233

- **proxies:** Add useClashProxies hook and related query keys by @keiko233

- **proxy:** Reset system proxy bypass to default when empty (#2858) (#3597) by @Slinetrac in [#3597](https://github.com/libnyanpasu/clash-nyanpasu/pull/3597)

- **proxy:** Enhance useSystemProxy hook with auto-refetching based on settings by @keiko233

- **proxy:** Add useSystemProxy hook for managing system proxy settings by @keiko233

- **proxy-mode:** Draft impl proxy mode management by @keiko233

- **route:** Enhance RouteComponent styling with background color support for light and dark modes by @keiko233

- **router:** Integrate AnimatedOutlet directional slide animations based on navigation history by @keiko233

- **router:** Draft implement experimental routing structure by @keiko233

- **rules:** Show live connections and traffic per rule (#5487) by @greenhat616 in [#5487](https://github.com/libnyanpasu/clash-nyanpasu/pull/5487)

- **rules:** Draft impl new rules page with proxy filtering by @keiko233

- **rules:** Rules page use virtualized table (#4044) by @keiko233 in [#4044](https://github.com/libnyanpasu/clash-nyanpasu/pull/4044)

- **rules:** Draft implement virtualized display for rules by @keiko233

- **rules:** Add useClashRulesProvider hook and related query key for managing clash rules by @keiko233

- **rules:** Add useClashRules hook and related query key for fetching clash rules by @keiko233

- **runtime:** Expand include-all groups as a mihomo-compatible step (#5406) by @greenhat616 in [#5406](https://github.com/libnyanpasu/clash-nyanpasu/pull/5406)

- **runtime:** Bind apply evidence to fixed runtime preconditions (#5324) by @greenhat616 in [#5324](https://github.com/libnyanpasu/clash-nyanpasu/pull/5324)

- **script:** Give Lua scripts a console that matches the JS one (#5455) by @greenhat616 in [#5455](https://github.com/libnyanpasu/clash-nyanpasu/pull/5455)

- **script:** Use Deno version manifest generation scripts by @keiko233

- **scripts:** Better check script by @keiko233

- **scripts:** Embeded js modules and helper tools (#3639) by @greenhat616 in [#3639](https://github.com/libnyanpasu/clash-nyanpasu/pull/3639)

- **scroll-area:** Impl scroll tracking with offset calculations by @keiko233

- **scroll-area:** Add both scrollbar orientation by @keiko233

- **scroll-area:** Add scrolling state tracking to scroll area component by @keiko233

- **scroll-area:** Implement scroll tracking context and enhance ScrollArea component by @keiko233

- **scroll-area:** Enhance ScrollArea component with scroll direction detection & top state by @keiko233

- **scroll-area:** Add AppContentScrollArea component and refactor usage in dashboard route by @keiko233

- **select:** Add Select component with styling and context management by @keiko233

- **separator:** Add Separator component using @radix-ui/react-separator by @keiko233

- **server-port:** Add custom hook for fetching server port and update IPC exports by @keiko233

- **service-prompt:** Add a copy button, and make text selectable by @greenhat616

- **setting:** Refactor ClashFieldSwitch to use useSetting hook for state management by @keiko233

- **setting:** Refactor SettingClashExternal component and improve external controller handling by @keiko233

- **setting:** Refactor clash base card by @keiko233

- **setting:** Update network statistic widget to use variant selection (#2575) by @keiko233 in [#2575](https://github.com/libnyanpasu/clash-nyanpasu/pull/2575)

- **setting:** Refactor theme and language settings using new hooks by @keiko233

- **setting:** Refactor system proxy settings with new hooks by @keiko233

- **settings:** Configure mixed, SOCKS5 and HTTP port strategies (#5400) by @greenhat616 in [#5400](https://github.com/libnyanpasu/clash-nyanpasu/pull/5400)

- **settings:** Add DNS cache management functionality by @keiko233

- **settings:** Add enhancement options for proxy, profile, and mode changes by @keiko233

- **settings:** Add log level selector by @keiko233

- **settings:** Path config debug settings (#4153) by @keiko233 in [#4153](https://github.com/libnyanpasu/clash-nyanpasu/pull/4153)

- **settings:** Clash field filter settings (#4152) by @keiko233 in [#4152](https://github.com/libnyanpasu/clash-nyanpasu/pull/4152)

- **settings:** Add UWP Loopback Tools button by @keiko233

- **settings:** Draft impl external controll page (#4147) by @keiko233 in [#4147](https://github.com/libnyanpasu/clash-nyanpasu/pull/4147)

- **settings:** New core manager (#4140) by @keiko233 in [#4140](https://github.com/libnyanpasu/clash-nyanpasu/pull/4140)

- **settings:** Add auto launch and silent start options in system settings by @keiko233

- **settings:** Impl system proxy settings input by @keiko233

- **settings:** Add mixed port configuration and random port toggle in clash settings by @keiko233

- **settings:** Draft implement ui settings by @keiko233

- **settings:** Add ExperimentalSwitch component to navigate to the experimental UI by @keiko233

- **settings:** Draft implement settings page by @keiko233

- **sidebar:** Impl sidebar mobile variants by @keiko233

- **sidebar:** Implement Sidebar and SidebarContent components for responsive layout by @keiko233

- **slider:** Impl slider component by @keiko233

- **state:** Settle resources before publishing source decisions (#5322) by @greenhat616 in [#5322](https://github.com/libnyanpasu/clash-nyanpasu/pull/5322)

- **state:** Serve committed config reads from snapshots and relocate workflow writes (#5321) by @greenhat616 in [#5321](https://github.com/libnyanpasu/clash-nyanpasu/pull/5321)

- **state:** Migrate typed config state actors (#4869) by @greenhat616 in [#4869](https://github.com/libnyanpasu/clash-nyanpasu/pull/4869)

- **state:** Route legacy writes through typed actors by @4o3F

- **state:** Read legacy config from typed actors by @4o3F

- **state:** Wire typed config actors into setup by @4o3F

- **state:** Add typed config actor infrastructure by @4o3F

- **storage:** Implement StorageValueChangedEvent by @keiko233

- **styles:** Replace backdrop-blur with bg-mixed-background in multiple components by @keiko233

- **styles:** Add custom mixed background color by @keiko233

- **switch:** Add switch component with loading state support by @keiko233

- **system-proxy:** Own proxy, PAC and auto-launch in a typed actor (#5268) by @greenhat616 in [#5268](https://github.com/libnyanpasu/clash-nyanpasu/pull/5268)

- **tailwind:** Add font settings for code elements in utilities by @keiko233

- **tauri:** Add RuntimeBuilder with golden snapshot baseline (#4888) by @greenhat616 in [#4888](https://github.com/libnyanpasu/clash-nyanpasu/pull/4888)

- **tauri:** Add profiles domain layer (ports, file service, actor, refresh scheduler) (#4910) by @greenhat616 in [#4910](https://github.com/libnyanpasu/clash-nyanpasu/pull/4910)

- **telegram-notify:** Refactor upload logic and switch to grammy for Telegram bot integration by @keiko233

- **theme:** Pick the theme color in an HCT panel with live preview (#5561) by @greenhat616 in [#5561](https://github.com/libnyanpasu/clash-nyanpasu/pull/5561)

- **theme:** Add currentThemeMode to ExperimentalThemeProvider by @keiko233

- **theme:** Update custom mixed colors for light and dark themes by @keiko233

- **theme:** Draft implement experimental theme provider & add Material Design color variables by @keiko233

- **theme:** Rename theme_setting to theme_color and implement migration by @keiko233

- **tooltip:** Add Tooltip component by @keiko233

- **topology:** Add animated connection and geographic views (#5308) by @greenhat616 in [#5308](https://github.com/libnyanpasu/clash-nyanpasu/pull/5308)

- **tracing:** Span transform application and script execution by @greenhat616

- **traffic:** Locate local sources through opt-in public IP probes (#5580) by @greenhat616 in [#5580](https://github.com/libnyanpasu/clash-nyanpasu/pull/5580)

- **traffic:** List closed connections and live ids by usage filters (#5562) by @greenhat616 in [#5562](https://github.com/libnyanpasu/clash-nyanpasu/pull/5562)

- **traffic:** Weigh the whole traffic page by traffic or connections (#5557) by @greenhat616 in [#5557](https://github.com/libnyanpasu/clash-nyanpasu/pull/5557)

- **traffic:** Animate the ranking lists as their order changes (#5555) by @greenhat616 in [#5555](https://github.com/libnyanpasu/clash-nyanpasu/pull/5555)

- **traffic:** Preview the traffic page with generated usage in dev builds (#5553) by @greenhat616 in [#5553](https://github.com/libnyanpasu/clash-nyanpasu/pull/5553)

- **traffic:** Shade the map by traffic share and how regions were located (#5552) by @greenhat616 in [#5552](https://github.com/libnyanpasu/clash-nyanpasu/pull/5552)

- **traffic:** Locate DIRECT destinations by their peer and record how (#5551) by @greenhat616 in [#5551](https://github.com/libnyanpasu/clash-nyanpasu/pull/5551)

- **traffic:** Locate regions with the app's own country index (#5550) by @greenhat616 in [#5550](https://github.com/libnyanpasu/clash-nyanpasu/pull/5550)

- **traffic:** Probe the public address DIRECT traffic leaves from (#5547) by @greenhat616 in [#5547](https://github.com/libnyanpasu/clash-nyanpasu/pull/5547)

- **traffic:** Rebuild the topology page as traffic analytics (#5519) by @greenhat616 in [#5519](https://github.com/libnyanpasu/clash-nyanpasu/pull/5519)

- **traffic:** Record usage per time bucket with retention (#5518) by @greenhat616 in [#5518](https://github.com/libnyanpasu/clash-nyanpasu/pull/5518)

- **traffic:** Expose session traffic through the client facade (#5479) by @greenhat616 in [#5479](https://github.com/libnyanpasu/clash-nyanpasu/pull/5479)

- **traffic:** Record session traffic in an application actor (#5478) by @greenhat616 in [#5478](https://github.com/libnyanpasu/clash-nyanpasu/pull/5478)

- **traffic:** Add profile-scoped traffic accounting with a redb store (#5477) by @greenhat616 in [#5477](https://github.com/libnyanpasu/clash-nyanpasu/pull/5477)

- **tray:** Add tray menu close behavior options and UI integration by @keiko233

- **tray:** Implement default behavior for TrayMenuMode based on OS by @keiko233

- **tray:** Draft impl web ui base tray menu (#4527) by @keiko233 in [#4527](https://github.com/libnyanpasu/clash-nyanpasu/pull/4527)

- **tray:** Add traffic display and macOS colored icons (#3653) by @Slinetrac in [#3653](https://github.com/libnyanpasu/clash-nyanpasu/pull/3653)

- **tray-settings:** Add tray icon configuration by @keiko233

- **ui:** Fit profile inspection to the viewport, tab step logs, highlight diff YAML (#5422) by @greenhat616 in [#5422](https://github.com/libnyanpasu/clash-nyanpasu/pull/5422)

- **ui:** Add a process chain view and profile names to profile inspection (#5421) by @greenhat616 in [#5421](https://github.com/libnyanpasu/clash-nyanpasu/pull/5421)

- **ui:** Refresh profile inspection automatically and render step logs (#5417) by @greenhat616 in [#5417](https://github.com/libnyanpasu/clash-nyanpasu/pull/5417)

- **ui:** Align release channel selector with settings rows by @greenhat616

- **ui:** Add a clash setting for include-all expansion (#5407) by @greenhat616 in [#5407](https://github.com/libnyanpasu/clash-nyanpasu/pull/5407)

- **ui:** Show configuration convergence and unconfirmed operations (#5368) by @greenhat616 in [#5368](https://github.com/libnyanpasu/clash-nyanpasu/pull/5368)

- **ui:** Surface the daemon compat gate in system settings (#5181) by @greenhat616 in [#5181](https://github.com/libnyanpasu/clash-nyanpasu/pull/5181)

- **ui:** Add segmented button component by @keiko233

- **ui:** Add proxy settings to header and implement proxy management hooks by @keiko233

- **ui:** Update external controller and core secret configurations by @keiko233

- **ui:** Add mixed port configuration with modal for editing by @keiko233

- **ui:** Replace Select components with DropdownMenu by @keiko233

- **ui:** Replace Select component with DropdownMenu by @keiko233

- **ui:** Replace Select component with DropdownMenu in LanguageSelector by @keiko233

- **ui:** Replace Select component with DropdownMenu for log level selection by @keiko233

- **ui:** Dropdown menu with checkbox items and alignment options by @keiko233

- **ui:** Add ScrollArea component with customizable viewport and scrollbar by @keiko233

- **ui:** Draft impl new button, progress, and ripple components by @keiko233

- **ui-effects:** Move locale, tray, logger and widget behind narrow adapters (#5270) by @greenhat616 in [#5270](https://github.com/libnyanpasu/clash-nyanpasu/pull/5270)

- **updater:** Add ghfast download source by @keiko233

- **updater:** Allow ordered package download sources by @keiko233

- **updater:** Add persistent release channels and UI switching (#5332) by @greenhat616 in [#5332](https://github.com/libnyanpasu/clash-nyanpasu/pull/5332)

- **updater:** Own core updates in an injected actor (#5249) by @greenhat616 in [#5249](https://github.com/libnyanpasu/clash-nyanpasu/pull/5249)

- **updater:** New updater UI (#4091) by @keiko233 in [#4091](https://github.com/libnyanpasu/clash-nyanpasu/pull/4091)

- **updater:** Add nightly release link handling in updater dialog by @keiko233

- **updater:** Upload updater to surge by @keiko233

- **updater:** Support windows nightly updater by @greenhat616

- **updater:** Support more targets by @greenhat616

- **utils:** Add PathResolver for centralized path resolution (#4828) by @greenhat616 in [#4828](https://github.com/libnyanpasu/clash-nyanpasu/pull/4828)

- **version:** Update clash-rs alpha binary names and download URL structure by @keiko233

- **widget:** Add network statistic widget with enable/variant settings by @keiko233

- **widget:** Draft a small network widget by @greenhat616

- **widget:** Draft a network widget based on egui by @greenhat616

- **window:** Add support for window decorations configuration by @keiko233

- **workflow:** Settle TCC mutations from authoritative evidence (#5320) by @greenhat616 in [#5320](https://github.com/libnyanpasu/clash-nyanpasu/pull/5320)

- **workflow:** Identify complete runtime targets and apply policy (#5323) by @greenhat616 in [#5323](https://github.com/libnyanpasu/clash-nyanpasu/pull/5323)

- **ws:** Draft impl ractor ClashConnectionsActor for websocket management by @keiko233

- Add app update functionality with hooks and tests (#5579) by @keiko233 in [#5579](https://github.com/libnyanpasu/clash-nyanpasu/pull/5579)

- Back up the config before migrations and from settings (#5529) by @greenhat616 in [#5529](https://github.com/libnyanpasu/clash-nyanpasu/pull/5529)

- Reflect the config/transform hierarchy on the profiles page (#5416) by @greenhat616 in [#5416](https://github.com/libnyanpasu/clash-nyanpasu/pull/5416)

- Bundle Inter Variable and set html lang from the app language (#5286) by @greenhat616 in [#5286](https://github.com/libnyanpasu/clash-nyanpasu/pull/5286)

- Enable Windows service named-pipe control (#5282) by @greenhat616 in [#5282](https://github.com/libnyanpasu/clash-nyanpasu/pull/5282)

- Add meow-rs core support (v0.18.0) (#4987) by @xmh0511 in [#4987](https://github.com/libnyanpasu/clash-nyanpasu/pull/4987)

- Add Tooltip component and integrate with existing UI structure by @keiko233

- Add LESS support to the CSS editor and compiler by @keiko233

- Replace SegmentedButton with AnimatedTabs in ProxyModeSelector by @keiko233

- Implement WindowReadyEvent for improved window management by @keiko233

- Add update notification button and localization for new version by @keiko233

- Refactor UI configuration to use window_type enum for window management by @keiko233

- Add HWID integration for Remnawave subscriptions (#4361) by @kolya13245 in [#4361](https://github.com/libnyanpasu/clash-nyanpasu/pull/4361)

- Settings design tweaks (#4072) by @keiko233 in [#4072](https://github.com/libnyanpasu/clash-nyanpasu/pull/4072)

- Local profile update (#3674) by @Slinetrac in [#3674](https://github.com/libnyanpasu/clash-nyanpasu/pull/3674)

- Draft new state manager (#3656) by @greenhat616 in [#3656](https://github.com/libnyanpasu/clash-nyanpasu/pull/3656)

- Add enable_tray_text option and make lint happy (#3650) by @Slinetrac in [#3650](https://github.com/libnyanpasu/clash-nyanpasu/pull/3650)

- Add enable_tray_text option (#3648) by @Slinetrac in [#3648](https://github.com/libnyanpasu/clash-nyanpasu/pull/3648)

- Support SAFE_PATHS by @greenhat616

- Add cache icon ttl by @greenhat616

- Mutation provider use refetchQueries fn by @keiko233

- Add mutation provider and enhance IPC event handling by @keiko233

- Optimize data handling in DataPanel by removing state management by @keiko233

- Add maximum history constants for connections, memory, traffic, and logs by @keiko233

- Add dayjs dependency and enhance ClashWSProvider for logging and state management by @keiko233

- Add clash version query and related hooks by @keiko233

- Try to add a windows shutdown hook to gracefully shutdown by @greenhat616

- Add QueryLoaderProvider to manage loading state in App component by @keiko233

- Add useSystemService hook and integrate it into ServiceShortcuts component by @keiko233

- Try to add deno deploy manifest mirror by @greenhat616

- Add deno deploy updater server file by @greenhat616

- Widget support (#1771) by @greenhat616 in [#1771](https://github.com/libnyanpasu/clash-nyanpasu/pull/1771)

- Add react-scan for performance analysis by @greenhat616

- Add element based breakpoint hook by @greenhat616

- Support `hidden` display of node attributes ✅ (#1819) by @MNDIA in [#1819](https://github.com/libnyanpasu/clash-nyanpasu/pull/1819)

- Add tun premission warning dialog and auto-granted premission in unix os by @greenhat616

- Draft a reorder for chains by @greenhat616

- Support fixed webview variants by @greenhat616

- Ui support clash rs alpha by @greenhat616

- Support clash rs alpha and windows arm64 by @greenhat616

- Enable clash internal selector cache by @greenhat616

- Impl nyanpasu storage bridge for jotai by @greenhat616

- Experimental memorized route path by @greenhat616

### 🐛 Bug Fixes

- **NavbarButton:** Update class names and add data-slot attribute by @keiko233

- **WindowReveal:** Prevent default action for Ctrl+F keydown event by @keiko233

- **animated-outlet:** Handling to prevent potential errors when nextMatch is undefined by @keiko233

- **animation:** Adjust spring animation duration for improved transition timing by @keiko233

- **bridge:** Map empty system proxy bypass to none by @4o3F

- **build:** Reuse Windows binaries with shared updater feeds (#5313) by @4o3F in [#5313](https://github.com/libnyanpasu/clash-nyanpasu/pull/5313)

- **build:** Embed app manifest into test binaries on Windows (#4830) by @greenhat616 in [#4830](https://github.com/libnyanpasu/clash-nyanpasu/pull/4830)

- **bundler:** Linux aarch64 build by @greenhat616

- **button:** Update dark mode background variant for button styles by @keiko233

- **button:** Button component with Slottable for improved child rendering by @keiko233

- **ci:** Increase archive upload chunks to 50 MiB by @keiko233

- **ci:** Prevent updater executables from replacing NSIS installers (#5420) by @4o3F in [#5420](https://github.com/libnyanpasu/clash-nyanpasu/pull/5420)

- **ci:** Unblock the lint gate and the aarch64 Linux build (#5273) by @greenhat616 in [#5273](https://github.com/libnyanpasu/clash-nyanpasu/pull/5273)

- **ci:** Check out the runtime submodule with LF on Windows (#5193) by @greenhat616 in [#5193](https://github.com/libnyanpasu/clash-nyanpasu/pull/5193)

- **ci:** Restore Windows sidecar bundling on nightly by @4o3F

- **ci:** Serialize legacy config bridge tests by @4o3F

- **ci:** Use tilde-separated version for Linux nightly RPM/DEB packages (#4723) by @latin-1 in [#4723](https://github.com/libnyanpasu/clash-nyanpasu/pull/4723)

- **ci:** Try to add linker debug info by @greenhat616

- **ci:** Bump xcode version to solve link failed by @greenhat616

- **ci:** Reject download non 200 file by @greenhat616

- **ci:** Use macos-latest to build app by @greenhat616

- **ci:** Add SURGE_TOKEN secret and inherit secrets in workflows (#2602) by @keiko233 in [#2602](https://github.com/libnyanpasu/clash-nyanpasu/pull/2602)

- **ci:** Updater manifest missing files by @greenhat616

- **ci:** Linux cross build by @greenhat616

- **ci:** Macos overflow heap by @greenhat616

- **ci:** Enable armel target build due to upstream fixes by @greenhat616

- **circle, progress:** Adjust positioning and layout styles for CircleSVG and CircularProgress by @keiko233

- **clash-ws-provider:** Persist log panel pause state across sessions (#3631) by @Slinetrac in [#3631](https://github.com/libnyanpasu/clash-nyanpasu/pull/3631)

- **config:** Default max_log_file_size in existing application configs (#5410) by @greenhat616 in [#5410](https://github.com/libnyanpasu/clash-nyanpasu/pull/5410)

- **config:** Start fresh installs from the typed config defaults (#5399) by @greenhat616 in [#5399](https://github.com/libnyanpasu/clash-nyanpasu/pull/5399)

- **config:** Verify service-host receipts through the confirmed submission (#5396) by @greenhat616 in [#5396](https://github.com/libnyanpasu/clash-nyanpasu/pull/5396)

- **config:** Address convergence review findings and trim the TCC stack (#5374) by @greenhat616 in [#5374](https://github.com/libnyanpasu/clash-nyanpasu/pull/5374)

- **config:** Persist allow-lan runtime patch (#4718) by @yokinanya in [#4718](https://github.com/libnyanpasu/clash-nyanpasu/pull/4718)

- **config:** Persist LAN connection setting with smart recovery (#3633) by @Slinetrac in [#3633](https://github.com/libnyanpasu/clash-nyanpasu/pull/3633)

- **connections:** Start the table scrollbar below the sticky header (#5509) by @greenhat616 in [#5509](https://github.com/libnyanpasu/clash-nyanpasu/pull/5509)

- **connections:** Close button trigger, and close #1770 by @greenhat616

- **connections:** Table i18n by @greenhat616

- **connections:** Table sort order by @greenhat616

- **connections:** Table header rendering, and close #1744 by @greenhat616

- **connections:** Number sorting, and close #1637 by @greenhat616

- **context-menu:** Custom items missing on first right-click by @keiko233

- **core:** Run the core locally when service mode cannot use the service (#5528) by @greenhat616 in [#5528](https://github.com/libnyanpasu/clash-nyanpasu/pull/5528)

- **core:** Preserve native state across desktop and service modes (#5425) by @4o3F in [#5425](https://github.com/libnyanpasu/clash-nyanpasu/pull/5425)

- **core:** Keep the source mapping order in the core's runtime config (#5404) by @greenhat616 in [#5404](https://github.com/libnyanpasu/clash-nyanpasu/pull/5404)

- **core:** Leave another instance's service core alone by @greenhat616

- **core:** Recover disconnected service endpoints (#5248) by @greenhat616 in [#5248](https://github.com/libnyanpasu/clash-nyanpasu/pull/5248)

- **core:** Find core binaries in the externalBin dir during dev (#5200) by @greenhat616 in [#5200](https://github.com/libnyanpasu/clash-nyanpasu/pull/5200)

- **core:** Prevent race conditions in core execution with run_lock by @keiko233

- **core:** Check config with window by @greenhat616

- **core:** Use nyanpasu utils' internal check config by @greenhat616

- **dashboard:** Prevent freeze when page is hidden (#3654) by @Slinetrac in [#3654](https://github.com/libnyanpasu/clash-nyanpasu/pull/3654)

- **dashboard:** Adjust grid sizes when clash core not support memory api by @keiko233

- **dashboard:** Update data handling to use new ClashMemory and ClashTraffic hooks by @keiko233

- **dashboard:** Ipsb missing country code issue, and close #1742 by @greenhat616

- **deep-link:** Import profiles from install-config deep links & sync profile name from subscription (#4913) by @kolya13245 in [#4913](https://github.com/libnyanpasu/clash-nyanpasu/pull/4913)

- **deps:** Align windows-core and webview2-com with wry by @greenhat616

- **deps:** Clean up deps by @keiko233

- **deps:** Replace bincode with postcard for cache file serialization (#4198) by @keiko233 in [#4198](https://github.com/libnyanpasu/clash-nyanpasu/pull/4198)

- **deps:** Bump framer-motion by @greenhat616

- **deps:** Update pnpm version by @keiko233

- **download:** Repair broken/flaky tests in core::download (#4740) by @latin-1 in [#4740](https://github.com/libnyanpasu/clash-nyanpasu/pull/4740)

- **editor:** Keep model path stable and Monaco out of main bundle (#5493) by @greenhat616 in [#5493](https://github.com/libnyanpasu/clash-nyanpasu/pull/5493)

- **enhance:** Js wrap failed after upgraded oxc by @greenhat616

- **enhance:** A panic while apply changes to a remote profile by @greenhat616

- **enhance:** Try to correct the original mapping order for lua runner by @greenhat616

- **eslint:** Clean up eslint config by removing unnecessary comments and projectService option by @keiko233

- **experimental-switch:** Update navigation path to dashboard and set memorized route by @keiko233

- **extensions:** Remove 'bungcip.better-toml' and add 'tamasfe.even-better-toml' to recommendations by @keiko233

- **fake-core:** Prevent macOS ConnectionReset in HTTP protocol tests (#5022) by @xmh0511 in [#5022](https://github.com/libnyanpasu/clash-nyanpasu/pull/5022)

- **fmt:** Format code by @keiko233

- **frontend:** Never print traffic amounts in exponent notation (#5554) by @greenhat616 in [#5554](https://github.com/libnyanpasu/clash-nyanpasu/pull/5554)

- **group-header:** Adjust BackButton visibility based on sidebar state by @keiko233

- **i18n:** Update locale mapping for zh-TW and zh-CN by @keiko233

- **i18n:** Copy feat i18n for service manual prompt (#2532) by @3gf8jv4dv in [#2532](https://github.com/libnyanpasu/clash-nyanpasu/pull/2532)

- **i18n:** Apply button for theme colors not translated (#2531) by @3gf8jv4dv in [#2531](https://github.com/libnyanpasu/clash-nyanpasu/pull/2531)

- **i18n:** Fix missing tray menu locale & wrong i18n key (#2514) by @3gf8jv4dv in [#2514](https://github.com/libnyanpasu/clash-nyanpasu/pull/2514)

- **i18n:** Rename locale file to fix broken tray menu text (#2489) by @3gf8jv4dv in [#2489](https://github.com/libnyanpasu/clash-nyanpasu/pull/2489)

- **i18n:** Refine zh-tw i18n code logic (#2485) by @3gf8jv4dv in [#2485](https://github.com/libnyanpasu/clash-nyanpasu/pull/2485)

- **i18n:** Connections table i18n for zh-tw (#2471) by @3gf8jv4dv in [#2471](https://github.com/libnyanpasu/clash-nyanpasu/pull/2471)

- **i18n:** System service setting prompt button i18n (#2432) by @3gf8jv4dv in [#2432](https://github.com/libnyanpasu/clash-nyanpasu/pull/2432)

- **input, select:** Styling for filled variants by @keiko233

- **ipc:** Update bindings generate by @keiko233

- **ipc:** Add default queryFn to clash ws hooks by @keiko233

- **ipc:** Update query key format for profile content by @keiko233

- **ipc:** Update useSetting upsert function to support async operations by @keiko233

- **ipc:** Update mutation function to accept partial profiles builder by @keiko233

- **ipc:** Disable useProfileContent auto fetch by @keiko233

- **jobs:** Keep maintenance timers with their actor owners (#5465) by @greenhat616 in [#5465](https://github.com/libnyanpasu/clash-nyanpasu/pull/5465)

- **jobs:** Events rotate cron by @greenhat616

- **js-runner:** Logger thread safe by @greenhat616

- **js-runner:** Make http module fetcher works by @greenhat616

- **js-runner:** Use CombineLoader to make it works, however, im a joker by @greenhat616

- **l10n:** Overhaul english and chinese translations (#5583) by @3gf8jv4dv in [#5583](https://github.com/libnyanpasu/clash-nyanpasu/pull/5583)

- **l10n:** Refine en and zh-cn translations (#5531) by @3gf8jv4dv in [#5531](https://github.com/libnyanpasu/clash-nyanpasu/pull/5531)

- **l10n:** Refine zh-tw translations (#5530) by @3gf8jv4dv in [#5530](https://github.com/libnyanpasu/clash-nyanpasu/pull/5530)

- **l10n:** Refine zh-tw translations (#5485) by @3gf8jv4dv in [#5485](https://github.com/libnyanpasu/clash-nyanpasu/pull/5485)

- **l10n:** Refine tray menu text 2 (#5460) by @3gf8jv4dv in [#5460](https://github.com/libnyanpasu/clash-nyanpasu/pull/5460)

- **l10n:** Unify word in tradchinese (#5461) by @3gf8jv4dv in [#5461](https://github.com/libnyanpasu/clash-nyanpasu/pull/5461)

- **l10n:** Add missing quote in update channel button (#5343) by @3gf8jv4dv in [#5343](https://github.com/libnyanpasu/clash-nyanpasu/pull/5343)

- **l10n:** Refine tray menu text (#5333) by @3gf8jv4dv in [#5333](https://github.com/libnyanpasu/clash-nyanpasu/pull/5333)

- **l10n:** Refine search placeholder text on various pages (#5316) by @3gf8jv4dv in [#5316](https://github.com/libnyanpasu/clash-nyanpasu/pull/5316)

- **l10n:** Refine profile inspect page text (#5317) by @3gf8jv4dv in [#5317](https://github.com/libnyanpasu/clash-nyanpasu/pull/5317)

- **l10n:** Update remote profile time description text (#4810) by @3gf8jv4dv in [#4810](https://github.com/libnyanpasu/clash-nyanpasu/pull/4810)

- **l10n:** Add missing english text (#4591) by @3gf8jv4dv in [#4591](https://github.com/libnyanpasu/clash-nyanpasu/pull/4591)

- **language:** Ensure locale is set in lowercase for consistency by @keiko233

- **language:** Improve cached language retrieval logic by @keiko233

- **language:** Use custom strategy for paraglide & streamline language context management by @keiko233

- **language:** Manually import language utils & inject paraglide custom strategy by @keiko233

- **language:** Add caching for selected language by @keiko233

- **lifecycle:** Stop the core even when the workflow died (#5540) by @greenhat616 in [#5540](https://github.com/libnyanpasu/clash-nyanpasu/pull/5540)

- **lint:** Lint-staged configuration by using direct eslint command to fix windows issues by @keiko233

- **lint:** Crlf by @keiko233

- **lint:** Missing eslint deps by @keiko233

- **lint-staged:** Add changed files while format backend codes by @greenhat616

- **linting:** Configuration and scripts for better file handling by @keiko233

- **lintstaged:** Prevent submission of untracked files by @keiko233

- **linux:** Rename GTK icons to avoid cache update failure (#3610) by @Slinetrac in [#3610](https://github.com/libnyanpasu/clash-nyanpasu/pull/3610)

- **linux:** Vendored openssl for compatiable with customized os, uos etc by @greenhat616

- **logging:** Stop the silent app log level from crashing the app (#5533) by @greenhat616 in [#5533](https://github.com/libnyanpasu/clash-nyanpasu/pull/5533)

- **logging:** Warn on service IPC and API monitor failures by @greenhat616

- **logging:** Replace hyphens with double underscores in index creation by @keiko233

- **logs:** Load core history on scroll and align footer selects (#5559) by @greenhat616 in [#5559](https://github.com/libnyanpasu/clash-nyanpasu/pull/5559)

- **logs:** Capture Debug logs and persist the current session (#5490) by @4o3F in [#5490](https://github.com/libnyanpasu/clash-nyanpasu/pull/5490)

- **logs:** Bottom toolbar layout and copy button reset (#5523) by @greenhat616 in [#5523](https://github.com/libnyanpasu/clash-nyanpasu/pull/5523)

- **macos:** Reopen window while dock icon is buggy show on the panel by @greenhat616

- **macos:** Set/reset dns while toggle tun mode by @greenhat616

- **macos:** Traffic lights flicking while resize window by @greenhat616

- **macos:** Service install issue by @greenhat616

- **macos:** Cleanup codes and remove round area while window is maximized by @greenhat616

- **macos:** Resize panic by @greenhat616

- **macos:** Try to use do shell start service by @greenhat616

- **macos:** Require minimumSystemVersion to 12.6 by @greenhat616

- **migration:** Refuse to start when migration state and files disagree (#5359) by @greenhat616 in [#5359](https://github.com/libnyanpasu/clash-nyanpasu/pull/5359)

- **migration:** Reconcile dangling profile transforms by @4o3F

- **migration:** Repair typed clash config path by @keiko233

- **migration:** Merge legacy clash guard overrides by @4o3F

- **migration:** Preserve typed clash config during migration (#4880) by @4o3F in [#4880](https://github.com/libnyanpasu/clash-nyanpasu/pull/4880)

- **migration:** Detect fresh install and do not commit lockfile for read op by @greenhat616

- **migration:** Enable v2.0.0 migrations by @greenhat616

- **migration:** Add a migration for chain by @greenhat616

- **modal:** Update modal trigger styles for improved responsiveness by @keiko233

- **navbar:** Error occurs when the name cannot be retrieved by @keiko233

- **nsis:** Default cross-version upgrade to in-place update instead of uninstall-first (#4807) by @greenhat616 in [#4807](https://github.com/libnyanpasu/clash-nyanpasu/pull/4807)

- **nyanpasu-config:** Fill NyanpasuAppConfig default with real values (#4762) by @greenhat616 in [#4762](https://github.com/libnyanpasu/clash-nyanpasu/pull/4762)

- **page-transition:** Replace local AnimatedOutlet with shared implementation by @keiko233

- **panic:** Shut the app down when the main thread panics (#5542) by @greenhat616 in [#5542](https://github.com/libnyanpasu/clash-nyanpasu/pull/5542)

- **panic:** Let panic force capture backtrace by @greenhat616

- **postcss:** Twcss intellisense build err by @keiko233

- **profile:** Local profile button import by @keiko233

- **profile:** Remote profile update helper fn by @keiko233

- **profile:** Use a workaround to parse taggedvalue type by @greenhat616

- **profiles:** Accept IP hosts in subscription URLs by @keiko233

- **profiles:** Stop opening a profile file from crashing the app (#5524) by @greenhat616 in [#5524](https://github.com/libnyanpasu/clash-nyanpasu/pull/5524)

- **profiles:** Show transform diagnostics and profile names (#5466) by @greenhat616 in [#5466](https://github.com/libnyanpasu/clash-nyanpasu/pull/5466)

- **profiles:** Keep a failed connection drop from failing activation by @greenhat616

- **profiles:** Keep chain editor columns in sync with drag state (#5277) by @greenhat616 in [#5277](https://github.com/libnyanpasu/clash-nyanpasu/pull/5277)

- **profiles:** Improve item reordering to preserve unmatched items by @keiko233

- **profiles:** Hidden proxy groups and add profile-name from remote profile headers (#3434) by @kolya13245 in [#3434](https://github.com/libnyanpasu/clash-nyanpasu/pull/3434)

- **profiles:** Abstract profile builder, and fixes #2495 by @greenhat616

- **profiles:** Update all subscriptions button in dark mode, and close #1767 by @greenhat616

- **profiles:** Touch new profile and process scoped chain by @greenhat616

- **profiles:** Add a migration for v1.6.0 to migrate script profile type by @greenhat616

- **profiles-header:** Increase z-index for sticky header by @keiko233

- **proxies:** Re-render useClashProxies when the query updates (#5427) by @greenhat616 in [#5427](https://github.com/libnyanpasu/clash-nyanpasu/pull/5427)

- **proxy:** Resolve provider-owned proxies (#4954) by @Jimie165 in [#4954](https://github.com/libnyanpasu/clash-nyanpasu/pull/4954)

- **root:** Adjust background opacity for improved visual consistency by @keiko233

- **route-memory:** Fix Index Page redirect by ensuring storage init (#3687) by @Slinetrac in [#3687](https://github.com/libnyanpasu/clash-nyanpasu/pull/3687)

- **router:** Patch frozen router with current tanstack store names by @keiko233

- **router:** Fix animated outlet showing stale route on navigation by @keiko233

- **script:** Keep mapping key order through Lua scripts (#5454) by @greenhat616 in [#5454](https://github.com/libnyanpasu/clash-nyanpasu/pull/5454)

- **script:** Name Lua chunks after the script instead of Rust source (#5453) by @greenhat616 in [#5453](https://github.com/libnyanpasu/clash-nyanpasu/pull/5453)

- **script:** Export the right main function when wrapping JS scripts (#5452) by @greenhat616 in [#5452](https://github.com/libnyanpasu/clash-nyanpasu/pull/5452)

- **script-ui:** Enable draggable window and proper URL opening in Monaco (#3612) by @Slinetrac in [#3612](https://github.com/libnyanpasu/clash-nyanpasu/pull/3612)

- **scripts:** Recognize the app package on Windows in boundary checks by @greenhat616

- **scripts:** Stop counting other crates' Config types as legacy calls by @greenhat616

- **scripts:** Make `generate-git-info.ts` retrieve commit authors (#4835) by @3gf8jv4dv in [#4835](https://github.com/libnyanpasu/clash-nyanpasu/pull/4835)

- **select:** Adjust shadow styles and transform origin for animations by @keiko233

- **sepcta:** Prettier formatting command by @keiko233

- **server:** Stop the internal HTTP server on shutdown (#5541) by @greenhat616 in [#5541](https://github.com/libnyanpasu/clash-nyanpasu/pull/5541)

- **service:** Uninstall a stopped service without stopping it first (#5526) by @greenhat616 in [#5526](https://github.com/libnyanpasu/clash-nyanpasu/pull/5526)

- **service:** Dark theme and install failed dir prompt by @greenhat616

- **service:** Restart core while service ipc state changed by @greenhat616

- **setting:** Update Auto Check Updates switch to use nullish coalescing for default value by @keiko233

- **setting:** Update service mode switch to use checked prop by @keiko233

- **setting:** Ensure default color is considered in theme expansion logic by @keiko233

- **settings:** Nest port dialog trigger inside the card content slot by @greenhat616

- **settings:** Layout structure for better responsiveness by @keiko233

- **settings:** Mobile page text marquee by @keiko233

- **settings:** Mui masonry style by @keiko233

- **settings:** Expand element display err by @keiko233

- **settings-title:** Adjust padding and gap for settings title layout by @keiko233

- **settings-title:** Adjust threshold for showing top title from 60 to 40 by @keiko233

- **settings-title:** Update padding classes for responsive design adjustments by @keiko233

- **setup:** Comment out logging setup to prevent IPC state issues by @keiko233

- **shiki:** Update getShikiSingleton to use getWasm for loading engine by @keiko233

- **shutdown:** Windows panic (#2629) by @keiko233

- **singleton:** Isolate Windows singleton per user/config and unify cross-platform behavior (#3622) by @Slinetrac in [#3622](https://github.com/libnyanpasu/clash-nyanpasu/pull/3622)

- **startup:** Remove fixed delays before CLI exit (#5570) by @4o3F in [#5570](https://github.com/libnyanpasu/clash-nyanpasu/pull/5570)

- **state:** Sync loaded typed config into legacy mirrors by @4o3F

- **state:** Prevent panic when unwrapping None values in ManagedState (#3706) by @Sn0wo2 in [#3706](https://github.com/libnyanpasu/clash-nyanpasu/pull/3706)

- **styles:** Correct background color fallback for .bg-inherit-allow-fallback class by @keiko233

- **styles:** Update body background color for light and dark themes by @keiko233

- **styles:** Connection chip by @greenhat616

- **tasks:** Hold the state for event dispatcher as a workaround towards redb internal bug by @greenhat616

- **tauri:** Try to force use tray-icon 0.21.0 by @greenhat616

- **tests:** Prepare dirs while first touch dir path by @greenhat616

- **tests:** Do not use nyanpasu proxy while testing by @greenhat616

- **tests:** Create a axum server for local testing by @greenhat616

- **theme:** System theme detection by @keiko233

- **theme:** Proxy toggle button group styles by @greenhat616

- **title-bar:** Implement custom hook for window maximize state management by @keiko233

- **traffic:** Keep the stat cards one height (#5556) by @greenhat616 in [#5556](https://github.com/libnyanpasu/clash-nyanpasu/pull/5556)

- **traffic:** Read the one country among a GeoIP record's tags (#5549) by @greenhat616 in [#5549](https://github.com/libnyanpasu/clash-nyanpasu/pull/5549)

- **translations:** Update consistency and clarity by @keiko233

- **tray:** Prevent multiple checked proxies in tray menu (#3624) by @Slinetrac in [#3624](https://github.com/libnyanpasu/clash-nyanpasu/pull/3624)

- **tray:** Wrong iter in select proxy by @greenhat616

- **tun:** Show failed message by @greenhat616

- **typo:** Fix the typo (#3893) by @lmmqxyx404 in [#3893](https://github.com/libnyanpasu/clash-nyanpasu/pull/3893)

- **ui:** Keep navbar button highlighted across its section's subpages (#5441) by @greenhat616 in [#5441](https://github.com/libnyanpasu/clash-nyanpasu/pull/5441)

- **ui:** Stop proxy node hover from dimming its chips and delay button by @greenhat616

- **ui:** Keep chain step headers in place under WebKit by @greenhat616

- **ui:** Keep sidebar widths stable across routes (#5419) by @greenhat616 in [#5419](https://github.com/libnyanpasu/clash-nyanpasu/pull/5419)

- **ui:** Outlined input placeholder overlap and idle border color (#5418) by @greenhat616 in [#5418](https://github.com/libnyanpasu/clash-nyanpasu/pull/5418)

- **ui:** Stop dashboard sparkline jitter (#5412) by @greenhat616 in [#5412](https://github.com/libnyanpasu/clash-nyanpasu/pull/5412)

- **ui:** Drop the border notch of outlined select without placeholder by @greenhat616

- **ui:** Memory usage display and subscription usage display bugs (#4436) by @Bihrys in [#4436](https://github.com/libnyanpasu/clash-nyanpasu/pull/4436)

- **ui:** Show correctly message while switching profiles by @greenhat616

- **ui:** Deadlock navigate call by @greenhat616

- **ui:** Mui v7 bc (#3006) by @keiko233 in [#3006](https://github.com/libnyanpasu/clash-nyanpasu/pull/3006)

- **updater:** Shut the app down before the update installer runs (#5543) by @greenhat616 in [#5543](https://github.com/libnyanpasu/clash-nyanpasu/pull/5543)

- **updater:** Use a fixed-length hash for nightly versions by @greenhat616

- **updater:** Sign nightly updates with the release version (#5464) by @greenhat616 in [#5464](https://github.com/libnyanpasu/clash-nyanpasu/pull/5464)

- **updater:** Update proxy URLs to use nyanpasu-script by @keiko233

- **updater:** Correct pre-release URL in updater dialog by @keiko233

- **updater:** Nightly channel build info prepare issue by @greenhat616

- **updater:** Version compare issue by @greenhat616

- **updater:** Use internal improved cmd to check updates by @greenhat616

- **updater:** Replace current core by @greenhat616

- **upload:** Add diagnostics upload and verification for nightly builds by @keiko233

- **v6:** Update sendMessage type definition in WebSocketHookResult by @keiko233

- **validation:** Improve input validation and config repair (#3657) by @Slinetrac in [#3657](https://github.com/libnyanpasu/clash-nyanpasu/pull/3657)

- **vite:** Bump safari>=13 to safari>=15 in the legacy plugin targets by @keiko233

- **whitelist:** Just detect valid fields and protected fields by @greenhat616

- **widget:** Use dup pipe to make piped correctly by @greenhat616

- **widget:** Hidden dock icon on macOs by @greenhat616

- **widget:** Theme and cleanup processes by @greenhat616

- **widget:** Deadlock on widget exit by @greenhat616

- **widget:** Instant repaint after status updated by @greenhat616

- **widget:** Start blocking and failture by @greenhat616

- **window:** Preserve state when exiting minimized (#4943) by @Jimie165 in [#4943](https://github.com/libnyanpasu/clash-nyanpasu/pull/4943)

- **window:** Update singleton window handling to focus instead of show by @keiko233

- **window:** Skip window state save when size is 0x0 during shutdown by @keiko233

- **window:** Clamp restored window size to minimum dimensions to prevent 0x0 windows by @keiko233

- **window:** Adjust traffic lights position for macOS window by @keiko233

- **window:** Adjust minimum inner size for window creation by @keiko233

- **window:** Handle window movement across monitors with different scaling (#3675) by @Slinetrac in [#3675](https://github.com/libnyanpasu/clash-nyanpasu/pull/3675)

- **window-control:** Correct icon rendering order in maximize button by @keiko233

- **windows:** Fix windows service mode issue by @4o3F

- **windows:** Auto launch issue by @greenhat616

- **workflow:** Update file patterns for artifact uploads in Windows NSIS build by @keiko233

- **workflow:** Improve file handling in release asset upload step by @keiko233

- Specify architecture for Windows and Linux builds by @keiko233

- Reorder restart options on system menu (#5581) by @3gf8jv4dv in [#5581](https://github.com/libnyanpasu/clash-nyanpasu/pull/5581)

- Avoid startup panic before tray is ready by @keiko233

- Tcc review loop (#5366) by @greenhat616 in [#5366](https://github.com/libnyanpasu/clash-nyanpasu/pull/5366)

- Update styling for GroupSummary and ProxyNodeButton components by @keiko233

- Update clash-rs upstream references (#5357) by @greenhat616 in [#5357](https://github.com/libnyanpasu/clash-nyanpasu/pull/5357)

- Adopt named-pipe IPC connection retries (#5281) by @greenhat616 in [#5281](https://github.com/libnyanpasu/clash-nyanpasu/pull/5281)

- Adapt to oxc_parser 0.148 ParserReturn field rename by @greenhat616

- Upgrade Boa to 0.22 and preserve YAML template nesting (#5202) by @greenhat616 in [#5202](https://github.com/libnyanpasu/clash-nyanpasu/pull/5202)

- Resolve all clippy warnings across the workspace (#5021) by @xmh0511 in [#5021](https://github.com/libnyanpasu/clash-nyanpasu/pull/5021)

- Remove tauri-icon patch by @greenhat616 in [#4955](https://github.com/libnyanpasu/clash-nyanpasu/pull/4955)

- Renormalize serena config by @greenhat616

- Fmt by @greenhat616

- Use valid monospace font stack in Monaco by @keiko233

- Handle errors for service installation, uninstallation, and control operations by @keiko233

- Remove support for windows-i686 platform in updater scripts by @keiko233

- Unify all units of information to ice (#4823) by @3gf8jv4dv in [#4823](https://github.com/libnyanpasu/clash-nyanpasu/pull/4823)

- Remote profile traffic and local file size display (#4809) by @3gf8jv4dv in [#4809](https://github.com/libnyanpasu/clash-nyanpasu/pull/4809)

- Color of more button on mobile navbar (#4811) by @3gf8jv4dv in [#4811](https://github.com/libnyanpasu/clash-nyanpasu/pull/4811)

- Button width on desktop navbar (#4812) by @3gf8jv4dv in [#4812](https://github.com/libnyanpasu/clash-nyanpasu/pull/4812)

- Fmt by @greenhat616

- Replace relaunch with commands.restartApplication in RestartButton by @keiko233

- Update version string format for Linux to comply with semver by @keiko233

- Unify PatchRuntimeConfig field naming to kebab-case (#4715) by @C4a15Wh in [#4715](https://github.com/libnyanpasu/clash-nyanpasu/pull/4715)

- Regenerate lockfile with cross-platform rolldown native bindings by @keiko233

- Deno fmt by @keiko233

- Remove unnecessary logging in copy_clash_env and show_dock_icon functions by @keiko233

- Resolve macOS clippy lifetime errors in DNS client and type inference by @keiko233

- Specta-typescript serde serialize err by @keiko233

- Specta bc & lint by @keiko233

- Update mihomo_alpha version handling and add asset extraction logic by @keiko233

- Import monaco service in editor route by @keiko233

- Vite 8 build by @keiko233

- Update URLs to use the new domain for resources and documentation by @keiko233

- Ci build by @keiko233

- Clean up by @keiko233

- Lint by @keiko233

- Lint by @keiko233

- Update artifact upload path to include only NSIS executables by @keiko233

- Light theme color err by @keiko233

- Update component styles to use SxProps for better type safety by @keiko233

- Reload clash config from file on core restart to persist user mode preference (#3919) by @temp-maker-temp in [#3919](https://github.com/libnyanpasu/clash-nyanpasu/pull/3919)

- Fix the errored codes (#3864) by @lmmqxyx404 in [#3864](https://github.com/libnyanpasu/clash-nyanpasu/pull/3864)

- Clash-rs core version format mismatch (#3826) by @ArakawaHenri in [#3826](https://github.com/libnyanpasu/clash-nyanpasu/pull/3826)

- Connections ui (#3688) by @Slinetrac in [#3688](https://github.com/libnyanpasu/clash-nyanpasu/pull/3688)

- Hide powershell window (#3686) by @Slinetrac in [#3686](https://github.com/libnyanpasu/clash-nyanpasu/pull/3686)

- Window manager on linux (#3676) by @Slinetrac in [#3676](https://github.com/libnyanpasu/clash-nyanpasu/pull/3676)

- Proxy chain ui (#3663) by @Slinetrac in [#3663](https://github.com/libnyanpasu/clash-nyanpasu/pull/3663)

- Proxy ui memory (#3668) by @Slinetrac in [#3668](https://github.com/libnyanpasu/clash-nyanpasu/pull/3668)

- Core name display (#3666) by @Slinetrac in [#3666](https://github.com/libnyanpasu/clash-nyanpasu/pull/3666)

- Menu description (#3662) by @Slinetrac in [#3662](https://github.com/libnyanpasu/clash-nyanpasu/pull/3662)

- Kernel download write display (#3661) by @Slinetrac in [#3661](https://github.com/libnyanpasu/clash-nyanpasu/pull/3661)

- Prevent NaN% in progress display (#3651) by @Slinetrac in [#3651](https://github.com/libnyanpasu/clash-nyanpasu/pull/3651)

- Tooltips display (#3644) by @Slinetrac in [#3644](https://github.com/libnyanpasu/clash-nyanpasu/pull/3644)

- Rm openssl deps by @greenhat616

- Unit tests by @greenhat616

- Enhance Linux auto-launch with improved logging and KDE compatibility #3466 (#3609) by @Slinetrac in [#3609](https://github.com/libnyanpasu/clash-nyanpasu/pull/3609)

- Lint by @greenhat616

- Delay chip color error (#3192) by @SherkeyXD in [#3192](https://github.com/libnyanpasu/clash-nyanpasu/pull/3192)

- Show TUN permission warning only when TUN is enabled (#3160) by @Rbqwow in [#3160](https://github.com/libnyanpasu/clash-nyanpasu/pull/3160)

- Build by @greenhat616

- Mui v7 tansatck table and themes by @greenhat616

- Mui v7 theme palette issue, migrating to color-mix and css vars by @greenhat616

- Cache icon panic from upgrading to bincode v2 by @greenhat616

- Mui and framer-motion runtime panic by @greenhat616

- Build issue by @greenhat616

- Bump mui to v7 by @greenhat616

- Bump objc2 to 0.6.1 (#2984) by @greenhat616 in [#2984](https://github.com/libnyanpasu/clash-nyanpasu/pull/2984)

- Bump bincode to v2, and avoid failure while cache corrupt by @greenhat616

- Solve the navigating to the definition in vscode (#2860) by @lmmqxyx404 in [#2860](https://github.com/libnyanpasu/clash-nyanpasu/pull/2860)

- Update script-dialog to handle query results correctly in Monaco viewer by @keiko233

- Avoid crash while using app (#2795) by @lmmqxyx404 in [#2795](https://github.com/libnyanpasu/clash-nyanpasu/pull/2795)

- Avoid crash while using clash-rs (#2793) by @lmmqxyx404

- Workspace location by @greenhat616

- Macos bundle uploader by @greenhat616

- Macos linker issue by pinned rustc version by @greenhat616

- Try to use old linker by @greenhat616

- Macos utils issue by @greenhat616

- Handle undefined connections in DataPanel by @keiko233

- Remove empty query functions from Clash hooks by @keiko233

- Potential shutdown race condition by @greenhat616

- Include default font family in theme creation by @keiko233

- Theme var context undefined by @keiko233

- Add block shutdown reason to blocking shutdown by @greenhat616

- Format win32 error by @greenhat616

- Shutdown hook setup panic by @greenhat616

- Mui styled engine theme load by @keiko233

- Clash core update by @keiko233

- Lint by @keiko233

- Updater check and styles by @greenhat616

- Lint and logs page emoji flags by @greenhat616

- Update surge command to use env variable for token by @keiko233

- Lint by @greenhat616

- Tailwind v4 styles by @greenhat616

- Ci by @greenhat616

- Nightly updater channel by @greenhat616

- Updater by @keiko233

- Remove mirror.ghproy.com by @greenhat616

- Try to fix events panic by @greenhat616

- Fmt by @greenhat616

- Migrate to shiki v2 by @greenhat616

- Migrate to tailwindcss v4 by @greenhat616

- Stylelint by @greenhat616

- Try to fix state corrupt again by @greenhat616

- Lint by @greenhat616

- Clippy by @greenhat616

- Downgrade objc2 until tauri got updated by @greenhat616

- Bump dirs to v6 by @greenhat616

- Bump framer-motion to 12 stable by @greenhat616

- Lint by @greenhat616

- Should remove \\? prefix for core dir path by @greenhat616

- Try to use accquire/release to solve state manager panic by @greenhat616

- Comp with deps by @greenhat616

- Lint by @greenhat616

- Linux build by @greenhat616

- Tun toggle logic by @greenhat616

- Revert bump window-core by @greenhat616

- Bump crates by @greenhat616

- Panic dialog styles by @greenhat616

- Lua decreption by @greenhat616

- Bump virtua to 0.39.1 by @greenhat616

- Bump react to v19 stable by @greenhat616

- Bump sysinfo to 0.33 by @greenhat616

- Bump crates by @greenhat616

- Lint by @greenhat616

- I18n (#1964) by @3gf8jv4dv in [#1964](https://github.com/libnyanpasu/clash-nyanpasu/pull/1964)

- Lint by @greenhat616

- Lint by @greenhat616

- Add polyfill for safari by @greenhat616

- Service install failure on windows, and lua runner misorder due to mihomo limitation (#1866) by @MNDIA in [#1866](https://github.com/libnyanpasu/clash-nyanpasu/pull/1866)

- Port mismatch (#1782) by @trhao99 in [#1782](https://github.com/libnyanpasu/clash-nyanpasu/pull/1782)

- Auto activate first profile, and close #1752 by @greenhat616

- Build non blocking to fix start a runtime in async runtime issue by @greenhat616

- Comp with profile chain by @greenhat616

- Comp with remote profile chain by @greenhat616

- Merge by @greenhat616

- Compatible with missing remote option by @greenhat616

- Bump delay-timer deps to fix linux i686 build issue by @greenhat616

- Clippy warning, and maybe address clash rs alpha updater issue by @greenhat616

- Change hide dock icon event to requestCloseWindow by @greenhat616

- Lint by @greenhat616

- Status dispatch issue by @greenhat616

- Fmt by @greenhat616

- Try to avoid the sendMessage panic and just log it by @greenhat616

- Styles by @greenhat616

- Protable and tg uploader issue by @greenhat616

- Clash-rs-alpha pack issue by @greenhat616

- Prfole file open in windows by @greenhat616

- Nightly notify telegram ci channel only by @greenhat616

- Build by @greenhat616

- Build by @greenhat616

- Clippy by @greenhat616

- Mark tray menu on left click false by @greenhat616

- Lint by @greenhat616

- Lint by @greenhat616

- Lint by @greenhat616

- Bump tauri version by @greenhat616

- Use product name for bundler main binary name by @greenhat616

- Mui color scheme by @greenhat616

- Dialog by @greenhat616

- Patch clash config issue by @greenhat616

- Mode switch issue, and close #1525 by @greenhat616

- Service tips by @greenhat616

- Acl by @greenhat616

- Speedtest delayed update by @greenhat616

- Shell no window by @greenhat616

- Bump boa to fix nightly build issue by @greenhat616

- Linux cross build by @greenhat616

- Try to fix windows resolver by @greenhat616

- Validate hex color value while theme color input on blur by @greenhat616

- Premission issue by @greenhat616

- Use latest auto-launch to allow running as admin by @greenhat616

### 💅 Styling

- **button:** Update button shadow styles for raised and fab variants by @keiko233

- **tooltip:** Update tooltip styles to use variant classes by @keiko233

- Update UI components for improved aesthetics and consistency by @keiko233

- Update background styles for app content & settings sidebar by @keiko233

### 📚 Documentation

- **actor-migration:** Add v2 roadmap and PR-2b/PR-3 design notes (#4872) by @greenhat616 in [#4872](https://github.com/libnyanpasu/clash-nyanpasu/pull/4872)

- **architecture:** Define PR-4S stabilization gate by @4o3F

- **jobs:** Define P0 contracts and add executable probes (#5221) by @greenhat616 in [#5221](https://github.com/libnyanpasu/clash-nyanpasu/pull/5221)

- **reviews:** Record WebKit style invalidation from positional selectors (#5447) by @greenhat616 in [#5447](https://github.com/libnyanpasu/clash-nyanpasu/pull/5447)

- **state:** Add IVerge migration mapping baseline by @4o3F

- **streams:** Design on-demand connection details and normalized proxies (#5424) by @greenhat616 in [#5424](https://github.com/libnyanpasu/clash-nyanpasu/pull/5424)

- **traffic:** Design locating connections with the app's geo index (#5548) by @greenhat616 in [#5548](https://github.com/libnyanpasu/clash-nyanpasu/pull/5548)

- **worktree:** Document setup and resource-reuse policy (#4875) by @greenhat616 in [#4875](https://github.com/libnyanpasu/clash-nyanpasu/pull/4875)

- Publish shared development and language standards by @keiko233

- Require unified RPC and clarify worktree selection by @keiko233

- Record the T0 integration baseline for the selective TCC plan (#5294) by @greenhat616 in [#5294](https://github.com/libnyanpasu/clash-nyanpasu/pull/5294)

- Record service pipe contention and probe race analysis by @greenhat616

- Update Code of Conduct document [skip ci] (#5176) by @3gf8jv4dv in [#5176](https://github.com/libnyanpasu/clash-nyanpasu/pull/5176)

- Make CLAUDE.md import AGENTS.md instead of duplicating it by @greenhat616

- Add git commit rules section to AGENTS.md and CLAUDE.md by @greenhat616

- Sync AGENTS.md and CLAUDE.md with actor/DI migration guidelines (#4833) by @greenhat616 in [#4833](https://github.com/libnyanpasu/clash-nyanpasu/pull/4833)

- Add agent guidelines and ignore AI tooling state (#4829) by @greenhat616 in [#4829](https://github.com/libnyanpasu/clash-nyanpasu/pull/4829)

- Lint fmt (#3693) by @Slinetrac in [#3693](https://github.com/libnyanpasu/clash-nyanpasu/pull/3693)

- Create CODE_OF_CONDUCT.md by @Slinetrac

- Add CONTRIBUTING.md (#3628) by @Slinetrac in [#3628](https://github.com/libnyanpasu/clash-nyanpasu/pull/3628)

- Revise readme [skip ci] (#1791) by @3gf8jv4dv in [#1791](https://github.com/libnyanpasu/clash-nyanpasu/pull/1791)

- Revise issue template [skip ci] (#1785) by @3gf8jv4dv in [#1785](https://github.com/libnyanpasu/clash-nyanpasu/pull/1785)

### 🔨 Refactor

- **app:** Inject windows, tray, logging and infrastructure (#5388) by @greenhat616 in [#5388](https://github.com/libnyanpasu/clash-nyanpasu/pull/5388)

- **backend:** Route verge config through ractor StateActor (#4778) by @greenhat616 in [#4778](https://github.com/libnyanpasu/clash-nyanpasu/pull/4778)

- **bundle:** Resolve metadata and configure startup directly (#5327) by @greenhat616 in [#5327](https://github.com/libnyanpasu/clash-nyanpasu/pull/5327)

- **chain:** Simplify chains function by removing useCallback by @keiko233

- **clash:** Remove useClashCore hook and related exports; update imports to use useClashProxies by @keiko233

- **clash-api:** Draft migrate to backend by @keiko233

- **clash-api:** Logging error message while request failed by @greenhat616

- **client:** Remove dead lifecycle routes and record the delivery (#5389) by @greenhat616 in [#5389](https://github.com/libnyanpasu/clash-nyanpasu/pull/5389)

- **config:** Return typed configuration and storage errors (#5435) by @greenhat616 in [#5435](https://github.com/libnyanpasu/clash-nyanpasu/pull/5435)

- **config:** Remove the legacy Config global and its mirrors (#5386) by @greenhat616 in [#5386](https://github.com/libnyanpasu/clash-nyanpasu/pull/5386)

- **core:** Group core lifecycle tests under a tests directory by @greenhat616

- **core:** Bind mode interruption to the lifecycle workflow (#5219) by @greenhat616 in [#5219](https://github.com/libnyanpasu/clash-nyanpasu/pull/5219)

- **core:** Serialize configuration writes through the facade (#5217) by @greenhat616 in [#5217](https://github.com/libnyanpasu/clash-nyanpasu/pull/5217)

- **core:** Move proxy callers into an injected actor (#5215) by @greenhat616 in [#5215](https://github.com/libnyanpasu/clash-nyanpasu/pull/5215)

- **core:** Read configs through the bound Clash API (#5208) by @greenhat616 in [#5208](https://github.com/libnyanpasu/clash-nyanpasu/pull/5208)

- **core:** Read rule providers through the bound Clash API (#5206) by @greenhat616 in [#5206](https://github.com/libnyanpasu/clash-nyanpasu/pull/5206)

- **core:** Route rule operations through the bound Clash API (#5204) by @greenhat616 in [#5204](https://github.com/libnyanpasu/clash-nyanpasu/pull/5204)

- **core:** Serialize lifecycle operations in an actor (#5201) by @greenhat616 in [#5201](https://github.com/libnyanpasu/clash-nyanpasu/pull/5201)

- **core:** Standardize boxed futures and async trait ports (#5199) by @greenhat616 in [#5199](https://github.com/libnyanpasu/clash-nyanpasu/pull/5199)

- **delay-history:** Update tooltip implementation and remove unused DelayHistoryBar by @keiko233

- **effects:** Type system proxy, DNS, hotkey and widget failures (#5436) by @greenhat616 in [#5436](https://github.com/libnyanpasu/clash-nyanpasu/pull/5436)

- **frontend:** Split workspace packages and unify entrypoints by @keiko233

- **frontend:** Replace Sass with Tailwind and plain CSS by @keiko233

- **frontend:** Replace Day.js with date-fns by @keiko233

- **frontend:** Replace lodash-es with es-toolkit (#5426) by @greenhat616 in [#5426](https://github.com/libnyanpasu/clash-nyanpasu/pull/5426)

- **hook:** Update useClashProxies to return structured proxies data by @keiko233

- **hooks:** Replace useNyanpasu with useSetting for core type management in useCoreType hook by @keiko233

- **hotkeys:** Use event based save logic by @greenhat616

- **input:** Remove asChild prop and set default input element by @keiko233

- **ipc:** Move configuration IPC onto typed app and clash config (#5385) by @greenhat616 in [#5385](https://github.com/libnyanpasu/clash-nyanpasu/pull/5385)

- **ipc:** Drop useNyanpasu hook and clean up imports by @keiko233

- **ipc:** Replace useNyanpasu with useProxyMode for proxy mode management in ProxyPage by @keiko233

- **ipc:** Clash configuration management with useClashAPI by @keiko233

- **ipc:** Generate command bindings use tauri sepcta (#2486) by @keiko233 in [#2486](https://github.com/libnyanpasu/clash-nyanpasu/pull/2486)

- **jobs:** Move business timers into the shared job owner (#5458) by @greenhat616 in [#5458](https://github.com/libnyanpasu/clash-nyanpasu/pull/5458)

- **jobs:** Use builder inject to avoid global singleton by @greenhat616

- **lifecycle:** Let owners shut themselves down and narrow the workflow (#5394) by @greenhat616 in [#5394](https://github.com/libnyanpasu/clash-nyanpasu/pull/5394)

- **logging:** Use hand-write indexer instead by @greenhat616

- **macos:** Use objc2 instead of objc and cocoa by @greenhat616

- **migration:** Let steps declare whether they are needed (#5360) by @greenhat616 in [#5360](https://github.com/libnyanpasu/clash-nyanpasu/pull/5360)

- **migration:** Rewrite migration subsystem to V2 on a new PathResolver (#4824) by @greenhat616 in [#4824](https://github.com/libnyanpasu/clash-nyanpasu/pull/4824)

- **modal:** Modal trigger layout and styling for better responsiveness by @keiko233

- **navbar:** Improve layout and styling of navbar components by @keiko233

- **nyanpasu-config:** Pure runtime pipeline executor (PR-3-pre②) (#4877) by @greenhat616 in [#4877](https://github.com/libnyanpasu/clash-nyanpasu/pull/4877)

- **nyanpasu-config:** Redesign runtime snapshot store around the new profile domain model (#4868) by @greenhat616 in [#4868](https://github.com/libnyanpasu/clash-nyanpasu/pull/4868)

- **nyanpasu-config:** Migrate profiles to clean composition type model + migration docs (#4840) by @greenhat616 in [#4840](https://github.com/libnyanpasu/clash-nyanpasu/pull/4840)

- **oxlint:** Draft migrate eslint to oxlint (#4075) by @keiko233 in [#4075](https://github.com/libnyanpasu/clash-nyanpasu/pull/4075)

- **profile:** Remove unused profile-normalize module and clean up use-profile by @keiko233

- **profiles:** Return typed profile errors with context (#5432) by @greenhat616 in [#5432](https://github.com/libnyanpasu/clash-nyanpasu/pull/5432)

- **profiles:** Replace parseTraffic with filesize for better file size formatting by @keiko233

- **router:** Migrate old routes to legacy group by @keiko233

- **rpc:** Unify connection detail subscription commands by @keiko233

- **runtime:** Return typed runtime and core errors (#5433) by @greenhat616 in [#5433](https://github.com/libnyanpasu/clash-nyanpasu/pull/5433)

- **runtime:** Stop reordering top-level keys when finalizing (#5405) by @greenhat616 in [#5405](https://github.com/libnyanpasu/clash-nyanpasu/pull/5405)

- **runtime:** Stabilize actor migration lifecycle (#4956) by @4o3F in [#4956](https://github.com/libnyanpasu/clash-nyanpasu/pull/4956)

- **script:** Run JS transforms from memory and keep the JS stack in errors (#5450) by @greenhat616 in [#5450](https://github.com/libnyanpasu/clash-nyanpasu/pull/5450)

- **script:** Use boa's console and pass the log sink into script runners (#5449) by @greenhat616 in [#5449](https://github.com/libnyanpasu/clash-nyanpasu/pull/5449)

- **script:** Migrate notification script to Deno and remove old by @keiko233

- **scripts:** Organize tooling behind Deno tasks by @keiko233

- **scripts:** Migrate to deno impl (#4538) by @keiko233 in [#4538](https://github.com/libnyanpasu/clash-nyanpasu/pull/4538)

- **setting:** Use useSetting for app log level and tray proxies selector management by @keiko233

- **settings:** Move configuration status into the debug page (#5401) by @greenhat616 in [#5401](https://github.com/libnyanpasu/clash-nyanpasu/pull/5401)

- **settings:** Move network statistic widget variant to nyanpasu misc by @keiko233

- **slider:** Simplify value handling in Slider component by @keiko233

- **state:** Type subscriber acks and degradation reasons (#5434) by @greenhat616 in [#5434](https://github.com/libnyanpasu/clash-nyanpasu/pull/5434)

- **state:** Move legacy verge facade into bridge by @4o3F

- **state:** Make verge reads use typed config clients by @4o3F

- **state:** Migrate state manager to nyanpasu-core with ACK pattern (#4594) by @greenhat616 in [#4594](https://github.com/libnyanpasu/clash-nyanpasu/pull/4594)

- **tauri:** Export profile domain types via specta with CI freshness gate (#4885) by @greenhat616 in [#4885](https://github.com/libnyanpasu/clash-nyanpasu/pull/4885)

- **tauri:** Introduce NyanpasuClient facade (ACL) [PR-1] (#4764) by @greenhat616 in [#4764](https://github.com/libnyanpasu/clash-nyanpasu/pull/4764)

- **theme:** Remove theme_blur option from IVerge struct by @keiko233

- **tooltip:** Enhance tooltip component with context and animation support by @keiko233

- **ui:** Update layout and styling for scroll areas and components by @keiko233

- **updater:** Simplify fetch_latest method and remove unused version fetching functions by @keiko233

- **window:** Multi window support (#4030) by @keiko233 in [#4030](https://github.com/libnyanpasu/clash-nyanpasu/pull/4030)

- **workflow:** Remove the residue of the old scheduler (#5395) by @greenhat616 in [#5395](https://github.com/libnyanpasu/clash-nyanpasu/pull/5395)

- Impl cross transport rpc (#5484) by @keiko233 in [#5484](https://github.com/libnyanpasu/clash-nyanpasu/pull/5484)

- Remove in-process deadlines and panic catching below the workflow (#5393) by @greenhat616 in [#5393](https://github.com/libnyanpasu/clash-nyanpasu/pull/5393)

- Draft nyanpasu config metadata (#4739) by @greenhat616 in [#4739](https://github.com/libnyanpasu/clash-nyanpasu/pull/4739)

- Implement bolt-load download engine by @keiko233

- Simplify useIsAppImage and remove unused updater support logic by @keiko233

- Remove GitHub token handling from portable script and workflow by @keiko233

- Draft drop legacy ui (#4508) by @keiko233 in [#4508](https://github.com/libnyanpasu/clash-nyanpasu/pull/4508)

- Draft impl material you expressive ui (#4229) by @keiko233 in [#4229](https://github.com/libnyanpasu/clash-nyanpasu/pull/4229)

- Draft implement main window structure (#3982) by @keiko233 in [#3982](https://github.com/libnyanpasu/clash-nyanpasu/pull/3982)

- Simplify error boundary component in __root.tsx by @keiko233

- Layout and index page components by @keiko233

- Remove unused useClash imports and update profile handling in components by @keiko233

- Drop old hooks & refactor logs with provider by @keiko233

- Remove websocket handling from individual hooks and centralize in ClashWSProvider by @keiko233

- Drop create-props & refactor SettingNyanpasuMisc components by @keiko233

- Replace useNyanpasu with useSetting for configuration management by @keiko233

- Remove simd-json, and use serde-json instead for indexmap support by @greenhat616

- Show panic dialog via a new process, and exit immediately by @greenhat616

- Use tauri internal cleanup process by @greenhat616

- Try to make table use deferred data and do unmount before page transition by @greenhat616

- Migrate react-dom-router to @tanstack/router by @greenhat616

---

## New Contributors

- @3gf8jv4dv made their first contribution in [#5581](https://github.com/libnyanpasu/clash-nyanpasu/pull/5581)
- @xmh0511 made their first contribution in [#5022](https://github.com/libnyanpasu/clash-nyanpasu/pull/5022)
- @kolya13245 made their first contribution in [#4913](https://github.com/libnyanpasu/clash-nyanpasu/pull/4913)
- @Jimie165 made their first contribution in [#4954](https://github.com/libnyanpasu/clash-nyanpasu/pull/4954)
- @moduvoice made their first contribution in [#4923](https://github.com/libnyanpasu/clash-nyanpasu/pull/4923)
- @latin-1 made their first contribution in [#4740](https://github.com/libnyanpasu/clash-nyanpasu/pull/4740)
- @yokinanya made their first contribution in [#4718](https://github.com/libnyanpasu/clash-nyanpasu/pull/4718)
- @C4a15Wh made their first contribution in [#4715](https://github.com/libnyanpasu/clash-nyanpasu/pull/4715)
- @Bihrys made their first contribution in [#4436](https://github.com/libnyanpasu/clash-nyanpasu/pull/4436)
- @temp-maker-temp made their first contribution in [#3919](https://github.com/libnyanpasu/clash-nyanpasu/pull/3919)
- @lmmqxyx404 made their first contribution in [#3893](https://github.com/libnyanpasu/clash-nyanpasu/pull/3893)
- @ArakawaHenri made their first contribution in [#3826](https://github.com/libnyanpasu/clash-nyanpasu/pull/3826)
- @Slinetrac made their first contribution
- @Sn0wo2 made their first contribution in [#3706](https://github.com/libnyanpasu/clash-nyanpasu/pull/3706)
- @SherkeyXD made their first contribution in [#3192](https://github.com/libnyanpasu/clash-nyanpasu/pull/3192)
- @Rbqwow made their first contribution in [#3160](https://github.com/libnyanpasu/clash-nyanpasu/pull/3160)
- @legiz-ru made their first contribution in [#2170](https://github.com/libnyanpasu/clash-nyanpasu/pull/2170)
- @MNDIA made their first contribution in [#1866](https://github.com/libnyanpasu/clash-nyanpasu/pull/1866)
- @trhao99 made their first contribution in [#1782](https://github.com/libnyanpasu/clash-nyanpasu/pull/1782)

**Full Changelog**: https://github.com/libnyanpasu/clash-nyanpasu/compare/v1.6.1...v2.0.0-beta.1

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
