# Changelog

## [0.2.0](https://github.com/doomspork/PandaSpy/compare/v0.1.0...v0.2.0) (2026-08-02)


### Features

* **client:** hand-rolled MQTT session with TOFU pinning and a reconnect supervisor ([ed7b2ae](https://github.com/doomspork/PandaSpy/commit/ed7b2ae3d2d824e48d4ea38cb2f440cc4a94dee6))
* **discovery:** implement multi-interface SSDP, subnet probe, and diagnostics ([a09d800](https://github.com/doomspork/PandaSpy/commit/a09d8006e46ea1000d0ea2884111d885efc8656f))
* **i18n:** add shared Fluent locales for Rust and the frontend ([46a7c76](https://github.com/doomspork/PandaSpy/commit/46a7c761eee5e90a8f7c16bc65bda4d625a25585))
* **i18n:** add Simplified Chinese (zh-CN) translation ([8bd070b](https://github.com/doomspork/PandaSpy/commit/8bd070b2a928aa69cf482ed3a060df4b27cb9b97))
* **proto:** implement the protocol core ([ecdc03c](https://github.com/doomspork/PandaSpy/commit/ecdc03cce4e0fd7f13416c49ab2d24a294b29beb))
* scaffold the four domain crates and the fixture corpus ([6ad6f23](https://github.com/doomspork/PandaSpy/commit/6ad6f23bfa699f94971cb88ffdbf98e75798946d))
* **store:** config, secrets, encrypted fallback, and cert-pin persistence ([ef50512](https://github.com/doomspork/PandaSpy/commit/ef50512bf546e3e3cdb8cefdff4d25c9126691fc))
* **tauri:** add the tray shell ([8b13c2c](https://github.com/doomspork/PandaSpy/commit/8b13c2c035e65123475bbfeae29bde05e8f84b92))
* **tauri:** scaffold the platform shell (tray, popover, plugins, Info.plist) ([f724726](https://github.com/doomspork/PandaSpy/commit/f72472674a369be4d3403df88522d7aa57c3e479))
* **tauri:** wire self-update via the updater and process plugins (M8) ([3ea807b](https://github.com/doomspork/PandaSpy/commit/3ea807b3f52ccc44ef6356b532e6b686648f2347))
* **tauri:** wire the domain crates into the app with commands and events ([4dc3934](https://github.com/doomspork/PandaSpy/commit/4dc39349e5ea40aad73a499975c99926498bb146))
* **ui:** add the typed IPC contract for the Rust bridge ([1743b71](https://github.com/doomspork/PandaSpy/commit/1743b717d644556f1e3b70ec90846ab63c2c5d7c))
* **ui:** build the printer-monitoring window and update banner (M6, M8) ([640dabb](https://github.com/doomspork/PandaSpy/commit/640dabb8abfda591b61fbaba96fe996efb8e218a))
* **ui:** scaffold the SvelteKit frontend with shared Fluent locales ([cb84f90](https://github.com/doomspork/PandaSpy/commit/cb84f90c69559934945fec327b5eaceaa57b9e6d))


### Bug fixes

* **ci:** cross-compile the Intel macOS bundle on macos-14 ([30935e6](https://github.com/doomspork/PandaSpy/commit/30935e6762a7e388403536de0de54376ccde0e8a))
* **ci:** give each bundle a unique workflow-artifact name ([03495c1](https://github.com/doomspork/PandaSpy/commit/03495c12acffb211890de6167335ba7cc0c05f8b))
* **client:** verify handshake signature, harden reconnect and reads ([33c6871](https://github.com/doomspork/PandaSpy/commit/33c6871bb4c4c2d8a4a6cfd7f5a9e988dfb4ddda))
* **discovery:** verify the handshake signature in the probe verifier too ([8b7ac76](https://github.com/doomspork/PandaSpy/commit/8b7ac76370a80587ab0ccf3e9ae1b3dd459489c8))
* **proto:** saturate remaining() so a hostile mc_remaining_time can't overflow ([9e419f8](https://github.com/doomspork/PandaSpy/commit/9e419f88a0fc3f0b55bd9c686225d945cd839f31))
* **store:** heal a corrupt pin file on write; install libdbus for the Linux keyring ([77b225a](https://github.com/doomspork/PandaSpy/commit/77b225acf2884e80b3814e0e2a872d652fe52509))
* **tauri:** collapse the machine-id if-let into a let-chain ([7fc0169](https://github.com/doomspork/PandaSpy/commit/7fc0169500f8aeb0fd39dde163dea58b2a48f1fa))
* **tauri:** stop a replaced supervisor's forwarder corrupting its successor ([f5591ac](https://github.com/doomspork/PandaSpy/commit/f5591ac1fe031c23238d4740e7ee1522ede8d8d4))
* **ui:** resolve adversarial-review findings in the window ([7b7a427](https://github.com/doomspork/PandaSpy/commit/7b7a427fa481bbd9c000140ecfce77512fcc7c5c))
