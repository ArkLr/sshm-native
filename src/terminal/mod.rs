//! A terminal tab: alacritty_terminal's `Term` plus a PTY running a program.
//!
//! alacritty_terminal's `EventLoop` runs on its own thread. It reads the PTY,
//! feeds the VT parser, and updates the shared `Term`. On a `Wakeup` event we
//! ask egui for a repaint, so an idle terminal costs no CPU and there's no
//! per-chunk IPC hop like the CEF build's `ExecuteJavaScript` calls.

mod input;
mod view;

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use alacritty_terminal::event::{Event, EventListener, Notify, OnResize, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg, Notifier};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::tty;

use crate::theme::{self, Theme};

pub use view::TerminalView;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TermSize {
    pub columns: usize,
    pub lines: usize,
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.lines
    }
    fn screen_lines(&self) -> usize {
        self.lines
    }
    fn columns(&self) -> usize {
        self.columns
    }
}

/// State the PTY thread reports back to the UI.
#[derive(Default)]
pub struct Status {
    pub title: Option<String>,
    pub exited: bool,
}

/// Receives events from the terminal thread.
#[derive(Clone)]
pub struct Listener {
    ctx: egui::Context,
    status: Arc<Mutex<Status>>,
    /// Set right after the event loop is created (it needs the listener first).
    sender: Arc<OnceLock<EventLoopSender>>,
    theme: Theme,
}

impl Listener {
    fn write(&self, text: String) {
        if let Some(sender) = self.sender.get() {
            let _ = sender.send(Msg::Input(Cow::Owned(text.into_bytes())));
        }
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::Wakeup | Event::Bell | Event::CursorBlinkingChange | Event::MouseCursorDirty => {
                self.ctx.request_repaint();
            }
            Event::Title(title) => {
                self.status.lock().unwrap().title = Some(title);
                self.ctx.request_repaint();
            }
            Event::ResetTitle => self.status.lock().unwrap().title = None,
            // Replies the terminal must send back, e.g. device attributes.
            Event::PtyWrite(text) => self.write(text),
            Event::ColorRequest(index, format) => {
                let overrides = alacritty_terminal::term::color::Colors::default();
                let color = match index {
                    256 => self.theme.foreground,
                    257 => self.theme.background,
                    258 => self.theme.cursor,
                    i if i < 16 => self.theme.ansi[i],
                    _ => self.theme.resolve(
                        alacritty_terminal::vte::ansi::Color::Indexed(index as u8),
                        &overrides,
                    ),
                };
                self.write(format(theme::to_rgb(color)));
            }
            // OSC 52 copy from the remote side.
            Event::ClipboardStore(_, text) => self.ctx.copy_text(text),
            Event::ChildExit(_) | Event::Exit => {
                self.status.lock().unwrap().exited = true;
                self.ctx.request_repaint();
            }
            // ClipboardLoad (remote reading your clipboard) is refused on
            // purpose. TextAreaSizeRequest is rare; add it when needed.
            _ => {}
        }
    }
}

pub struct TerminalSession {
    pub term: Arc<FairMutex<Term<Listener>>>,
    pub status: Arc<Mutex<Status>>,
    pub size: TermSize,
    cell_px: (u16, u16),
    notifier: Notifier,
}

impl TerminalSession {
    /// `command = None` runs the user's login shell.
    pub fn spawn(
        ctx: &egui::Context,
        theme: Theme,
        command: Option<(String, Vec<String>)>,
    ) -> anyhow::Result<Self> {
        let size = TermSize { columns: 80, lines: 24 };
        let status = Arc::new(Mutex::new(Status::default()));
        let sender = Arc::new(OnceLock::new());
        let listener = Listener { ctx: ctx.clone(), status: status.clone(), sender: sender.clone(), theme };

        let config = Config { scrolling_history: 10_000, ..Default::default() };
        let term = Arc::new(FairMutex::new(Term::new(config, &size, listener.clone())));

        #[allow(clippy::needless_update)] // fills the Windows-only `escape_args`
        let options = tty::Options {
            shell: command.map(|(program, args)| tty::Shell::new(program, args)),
            working_directory: dirs::home_dir(),
            drain_on_exit: true,
            // xterm-256color, not "alacritty": most servers lack that terminfo.
            env: HashMap::from([
                ("TERM".to_string(), "xterm-256color".to_string()),
                ("COLORTERM".to_string(), "truecolor".to_string()),
            ]),
            ..Default::default()
        };
        let pty = tty::new(&options, window_size(size, (8, 16)), 0)?;

        let event_loop = EventLoop::new(term.clone(), listener, pty, options.drain_on_exit, false)?;
        let channel = event_loop.channel();
        let _ = sender.set(channel.clone());
        event_loop.spawn();

        Ok(Self { term, status, size, cell_px: (8, 16), notifier: Notifier(channel) })
    }

    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        self.notifier.notify(bytes);
    }

    pub fn resize(&mut self, size: TermSize, cell_px: (u16, u16)) {
        if size == self.size && cell_px == self.cell_px {
            return;
        }
        self.size = size;
        self.cell_px = cell_px;
        self.notifier.on_resize(window_size(size, cell_px));
        self.term.lock().resize(size);
    }

    pub fn title(&self) -> Option<String> {
        self.status.lock().unwrap().title.clone()
    }

    pub fn exited(&self) -> bool {
        self.status.lock().unwrap().exited
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        // Stops the PTY thread; dropping the PTY hangs up the child (SIGHUP).
        let _ = self.notifier.0.send(Msg::Shutdown);
    }
}

fn window_size(size: TermSize, cell_px: (u16, u16)) -> WindowSize {
    WindowSize {
        num_lines: size.lines as u16,
        num_cols: size.columns as u16,
        cell_width: cell_px.0,
        cell_height: cell_px.1,
    }
}
