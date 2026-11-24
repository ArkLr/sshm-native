# SSHM (native)

Native, GPU-rendered rewrite of SSHM. No Chromium.

| Layer | Crate | Replaces (CEF build) |
|---|---|---|
| Window + UI (wgpu: Vulkan/Metal/DX12) | `eframe` / `egui` | CEF + HTML/CSS/`app.js` |
| VT parser, grid, scrollback, PTY | `alacritty_terminal` | xterm.js + `pty_session.cc` / `local_shell_session.cc` |
| SSH transport | system `ssh` in the PTY (for now) | libssh2 |
| Saved hosts | `serde_json` → `~/.config/sshm/connections.json` (same file) | `connection_store.cc` |

## Run

```bash
cargo run --release
```

Shortcuts: `Ctrl+Shift+T` local shell · `Ctrl+Shift+W` close tab · `Ctrl+Tab` next tab ·
`Ctrl+Shift+C / V` copy / paste · `Shift+PgUp/PgDn` scrollback · `Ctrl+Shift+=` / `Ctrl+-` font size.

## Layout

```
src/
  main.rs            window setup
  app.rs             sidebar, tabs, connection dialog
  config.rs          connections.json load/save (keeps unknown fields)
  theme.rs           palette + ANSI/256/truecolor resolution
  terminal/mod.rs    TerminalSession: Term + PTY thread, events → repaint
  terminal/view.rs   grid painting, selection, wheel, keyboard dispatch
  terminal/input.rs  key → xterm byte sequences
```

## Roadmap

1. **Mouse reporting** (`TermMode::MOUSE_*`) so tmux/vim get clicks; Shift to override, like the CEF build.
2. **Native SSH with `russh`**: implement `alacritty_terminal::tty::EventedPty` on top of an SSH channel, then use the stored password/key passphrase from `secrets_store` (system keyring via the `keyring` crate).
3. **Stats panel**: port `stats_session.cc` onto a second `russh` exec channel.
4. **Settings + themes**: fonts (load a Nerd Font via `egui::FontDefinitions`), light theme, cursor blink.
5. **Assistant panel**: async HTTP (`reqwest` + `tokio`) streaming into an egui side panel.
6. **Drag-and-drop reordering** for the sidebar (replaces Sortable.js).
7. If profiling shows painting is the bottleneck: move the grid to a custom wgpu glyph-atlas pass (`egui_wgpu::CallbackTrait`), the way Alacritty and Zed draw text.
