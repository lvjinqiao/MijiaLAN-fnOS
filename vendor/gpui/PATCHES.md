# Local Windows packaging and popup patches

Source: crates.io `gpui` 0.2.2; original Apache-2.0 license retained.

`src/platform/windows/directx_renderer.rs`: Windows Debug shaders use embedded
HLSL text with the existing alpha-correction include expanded, then call
`D3DCompile` in the system's `d3dcompiler_47.dll`. Upstream's Debug path calls
`D3DCompileFromFile` with `CARGO_MANIFEST_DIR`, which points at the build machine
and is unavailable in a distributed application.

The HLSL source, shader entry points, shader targets, compile flags, release
shader pipeline, and non-Windows renderers are unchanged.

The application build script separately embeds Common Controls v6 and DPI
metadata when cross-compiling on a non-Windows host. Upstream GPUI already
embeds its manifest when built natively on Windows.

`src/platform/windows/window.rs`: `WindowKind::PopUp` explicitly uses `WS_POPUP`
instead of the zero-valued `WS_OVERLAPPED` style, and uses the default client
rectangle for this borderless style. The custom-titlebar path otherwise
reserves invisible resize-frame insets even though a popup has no caption or
resize border, which crops and offsets the compact panel. Normal desktop
windows and other platform backends keep their existing behavior.

`src/platform/windows/platform.rs`: closing the last visible window no longer
posts `WM_QUIT` unconditionally. The application already owns this decision in
`App::on_window_closed`: it quits when no tray exists, and keeps polling and
HomeKit alive when a tray remains. Explicit Quit still follows the original
platform quit path. The platform message window continues to dispatch tray
events so a dismissed panel can be reopened.
