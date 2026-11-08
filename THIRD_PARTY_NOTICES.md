# Third-party notices

- **MijiaLAN** — MIT, copyright iPocket-APP. The protocol, schema data, verified legacy profiles, shared-controller behavior and bridge mapping were ported from the sibling Swift project. Its original checkout is not modified by this project.
- **MenuBarMijia / MijiaCore** — MIT. Xiaomi QR login, RC4 request signing, room parsing and related protocol behavior were reimplemented in Rust from the pinned Swift dependency used by MijiaLAN. See `licenses/MenuBarMijia/LICENSE`.
- **hass-xiaomi-miot** — Apache-2.0, upstream commit `75bc74f67275bde6a8742b6638039819e10eaea5`. `resources/legacy-miio.json` contains converted data from the original project, with the original hand-checked model overrides applied. No Python templates are executed. See `licenses/hass-xiaomi-miot/` for the license and provenance.
- **homebridge-miot** — MIT. Native HomeKit category mapping follows the rules already ported into MijiaLAN, with independent Rust implementation. See `licenses/homebridge-miot/`.

- **GPUI 0.2.2**, **GPUI Component / Assets 0.5.1** — Apache-2.0. GPU rendering, widgets and widget SVG icons. GPUI is vendored with Windows Debug shader packaging, popup-frame and tray-lifecycle patches; see `vendor/gpui/PATCHES.md` and `licenses/gpui/`.
- **xattr 0.2.3** — MIT OR Apache-2.0, copyright Steven Allen. Vendored with a Linux/Android `ENOATTR` compatibility alias to `ENODATA`; the macOS/BSD behavior is unchanged. See `licenses/xattr/` and `vendor/xattr/PATCHES.md`.
- **hap-crypto 1.4.0** — Apache-2.0. The HAP-compatible SRP server helper is used; the application implements the accessory pairing orchestration and transport. The independent controller implementation is also used for interoperability tests.
- **RustCrypto**, **ed25519-dalek**, **x25519-dalek**, **mdns-sd**, **reqwest**, **keyring**, **tray-icon**, **keepawake**, **qrcode**, and other Cargo dependencies retain their individual licenses. Exact resolved versions are recorded in `Cargo.lock`.

Vacuum action admission, model-specific status interpretation, HomeKit cleaning/docking switches and the four explicit legacy Roborock profiles follow the native MijiaLAN implementation. The status semantics and schema fixtures retain the native project's MIoT / homebridge-miot / hass-xiaomi-miot provenance. Actions are independently implemented in Rust; no upstream Python code or template runtime is included.

The project's MIT license does not grant rights to third-party trademarks, protocols or services. Use `cargo metadata --locked` to inspect dependency license metadata before distribution; retain required notices with release packages.
## Windows compact-panel icons

Lucide 0.468.0 SVGs are embedded under `resources/windows/icons/`; the Windows and macOS application icons also derive their house outline from Lucide. Copyright Lucide Contributors and Cole Bemis (Feather portions). The ISC license and attribution are retained at `licenses/lucide/LICENSE`. macOS menu-bar and device symbols remain supplied by AppKit.

