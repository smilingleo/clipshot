# ClipShot

## Build & Install

```bash
make bundle    # Build release, generate icons, create .app, codesign
make install   # Copy to /Applications/
```

The app is ad-hoc code signed with `ClipShot.entitlements` (screen capture + audio input). This ensures macOS TCC permissions persist across rebuilds. Without signing, every rebuild resets screen recording permission.

## Global Hotkeys

- **Ctrl+Cmd+A** — Screenshot capture
- **Ctrl+Cmd+Z** — Screen recording (start/stop)
- **Ctrl+Cmd+S** — Scroll capture (start/stop)

## Architecture

- Rust + objc2 bindings to AppKit/CoreGraphics
- Menu bar app (`LSUIElement = true`), no Dock icon
- Bundle ID: `com.clipshot.app`

### Key Modules

- `src/app.rs` — AppDelegate, hotkey polling, mode management, toolbar/editor orchestration
- `src/editor/window.rs` — Editor window creation, sizing (80% screen cap), border, minibar
- `src/editor/view.rs` — Editor drawing, annotation tools, mouse/key handling
- `src/editor/model.rs` — EditorState, TimedAnnotation (default end = start + 1s)
- `src/editor/minibar.rs` — Per-annotation timeline bar with start/end handles
- `src/editor/export.rs` — Video export with baked-in annotations
- `src/toolbar/` — Floating NSPanel toolbar (attached as child window to editor)
- `src/overlay/` — Full-screen overlay for region selection
- `src/hotkey.rs` — Global hotkey registration (global-hotkey crate)
- `src/recording.rs` — Screen recording state/encoder
- `src/scroll_capture.rs` — Scroll capture state
- `src/stitch.rs` — Scroll capture frame stitching

### Cancel Behavior

Cancel (Esc or toolbar button) discards everything without saving or copying to clipboard across all modes. The editor uses an `editor_cancelled` flag to distinguish cancel from window close button (which offers save dialog).
