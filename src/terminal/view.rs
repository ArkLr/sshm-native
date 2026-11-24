//! Draws a `Term` with egui's painter and turns UI input into PTY bytes.
//!
//! Rendering works in runs: cells next to each other with the same colour
//! become one text shape placed at its exact column, so a screen is a few
//! hundred shapes, not one per cell. Non-ASCII characters (box drawing, CJK,
//! emoji) get their own shape so fallback-font widths can't push the grid
//! out of line.

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor};
use egui::{Align2, Color32, Event, FontId, Pos2, Rect, Sense, Vec2};

use super::{TermSize, TerminalSession, input};
use crate::theme::Theme;

pub struct TerminalView<'a> {
    pub session: &'a mut TerminalSession,
    pub theme: &'a Theme,
    pub font: FontId,
    pub id: egui::Id,
}

impl TerminalView<'_> {
    pub fn show(self, ui: &mut egui::Ui) -> egui::Response {
        let rect = ui.available_rect_before_wrap();
        let response = ui.interact(rect, self.id, Sense::click_and_drag());
        ui.advance_cursor_after_rect(rect);

        // Snap cells to physical pixels so lines don't blur or leave seams.
        let ppp = ui.ctx().pixels_per_point();
        let (glyph_w, row_h) = ui.ctx().fonts_mut(|f| (f.glyph_width(&self.font, 'M'), f.row_height(&self.font)));
        let cell = Vec2::new((glyph_w * ppp).round() / ppp, (row_h * ppp).round() / ppp);

        let size = TermSize {
            columns: ((rect.width() / cell.x) as usize).max(2),
            lines: ((rect.height() / cell.y) as usize).max(1),
        };
        self.session.resize(size, ((cell.x * ppp) as u16, (cell.y * ppp) as u16));

        if response.clicked() || response.drag_started() {
            response.request_focus();
        }
        let focused = response.has_focus();
        if focused {
            // Keep Tab, arrows and Escape for the terminal instead of egui's
            // focus navigation.
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    self.id,
                    egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
                )
            });
            self.handle_keyboard(ui);
        }
        self.handle_mouse(ui, &response, rect, cell);
        self.paint(ui, rect, cell, focused);
        response
    }

    fn handle_keyboard(&self, ui: &egui::Ui) {
        let (events, mods) = ui.input(|i| (i.events.clone(), i.modifiers));
        let mode = *self.session.term.lock().mode();

        for event in events {
            match event {
                // Alt+letter is already sent as ESC+letter from the Key event.
                Event::Text(text) if !(mods.alt && !mods.ctrl) => {
                    self.scroll_to_bottom();
                    self.session.write(text.into_bytes());
                }
                // egui turns plain Ctrl+C / Ctrl+X into clipboard events; in a
                // terminal they have to be ^C (interrupt) and ^X.
                Event::Copy => self.session.write(&b"\x03"[..]),
                Event::Cut => self.session.write(&b"\x18"[..]),
                Event::Paste(text) => self.paste(&text, mode),
                Event::Key { key, pressed: true, modifiers, .. } => {
                    if modifiers.ctrl && modifiers.shift {
                        match key {
                            egui::Key::C => {
                                if let Some(text) = self.session.term.lock().selection_to_string() {
                                    ui.ctx().copy_text(text);
                                }
                                continue;
                            }
                            egui::Key::V => {
                                if let Some(text) = read_clipboard() {
                                    self.paste(&text, mode);
                                }
                                continue;
                            }
                            _ => {}
                        }
                    }
                    if modifiers.shift && matches!(key, egui::Key::PageUp | egui::Key::PageDown) {
                        let scroll = if key == egui::Key::PageUp { Scroll::PageUp } else { Scroll::PageDown };
                        self.session.term.lock().scroll_display(scroll);
                        continue;
                    }
                    if let Some(bytes) = input::key_to_bytes(key, modifiers, mode) {
                        self.scroll_to_bottom();
                        self.session.write(bytes);
                    }
                }
                _ => {}
            }
        }
    }

    fn handle_mouse(&self, ui: &egui::Ui, response: &egui::Response, rect: Rect, cell: Vec2) {
        // Wheel: scrollback on the normal screen; arrow keys on the alternate
        // screen (less, vim, htop) the way most terminals do it.
        if response.hovered() {
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            let lines = (dy / cell.y).round() as i32;
            if lines != 0 {
                let mut term = self.session.term.lock();
                if term.mode().contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
                    let app_cursor = term.mode().contains(TermMode::APP_CURSOR);
                    drop(term);
                    let seq: &[u8] = match (lines > 0, app_cursor) {
                        (true, true) => b"\x1bOA",
                        (true, false) => b"\x1b[A",
                        (false, true) => b"\x1bOB",
                        (false, false) => b"\x1b[B",
                    };
                    self.session.write(seq.repeat(lines.unsigned_abs() as usize));
                } else {
                    term.scroll_display(Scroll::Delta(lines));
                }
            }
        }

        // Selection: drag to select, double-click for a word, click to clear.
        // TODO: forward mouse events when the program asks for them (tmux,
        // vim), i.e. TermMode::MOUSE_MODE, with Shift to override.
        let Some(pos) = response.interact_pointer_pos() else { return };
        let mut term = self.session.term.lock();
        let offset = term.grid().display_offset() as i32;
        let col = (((pos.x - rect.min.x) / cell.x).max(0.0) as usize).min(term.columns() - 1);
        let row = (((pos.y - rect.min.y) / cell.y).max(0.0) as i32).min(term.screen_lines() as i32 - 1);
        let point = Point::new(Line(row - offset), Column(col));
        let side = if (pos.x - rect.min.x) % cell.x > cell.x / 2.0 { Side::Right } else { Side::Left };

        if response.double_clicked() {
            term.selection = Some(Selection::new(SelectionType::Semantic, point, side));
        } else if response.drag_started() {
            term.selection = Some(Selection::new(SelectionType::Simple, point, side));
        } else if response.dragged() {
            if let Some(sel) = term.selection.as_mut() {
                sel.update(point, side);
            }
        } else if response.clicked() {
            term.selection = None;
        }
    }

    fn paste(&self, text: &str, mode: TermMode) {
        self.scroll_to_bottom();
        if mode.contains(TermMode::BRACKETED_PASTE) {
            // Strip any end marker inside the text so it can't escape the paste.
            let clean = text.replace("\x1b[201~", "");
            self.session.write(format!("\x1b[200~{clean}\x1b[201~").into_bytes());
        } else {
            self.session.write(text.replace("\r\n", "\r").replace('\n', "\r").into_bytes());
        }
    }

    fn scroll_to_bottom(&self) {
        self.session.term.lock().scroll_display(Scroll::Bottom);
    }

    fn paint(&self, ui: &egui::Ui, rect: Rect, cell: Vec2, focused: bool) {
        let painter = ui.painter_at(rect);
        let theme = self.theme;
        painter.rect_filled(rect, 0.0, theme.background);

        let term = self.session.term.lock();
        let content = term.renderable_content();
        let colors = content.colors;
        let offset = content.display_offset as i32;
        let cursor = content.cursor;
        let selection = content.selection;
        let cursor_visible = content.mode.contains(TermMode::SHOW_CURSOR) && cursor.shape != CursorShape::Hidden;

        let origin = |line: i32, col: usize| {
            Pos2::new(rect.min.x + col as f32 * cell.x, rect.min.y + (line + offset) as f32 * cell.y)
        };

        // Pending text run: (line, start column, text, colour)
        let mut run: Option<(i32, usize, String, Color32)> = None;
        let flush = |run: &mut Option<(i32, usize, String, Color32)>| {
            if let Some((line, col, text, color)) = run.take() {
                painter.text(origin(line, col), Align2::LEFT_TOP, text, self.font.clone(), color);
            }
        };

        for indexed in content.display_iter {
            let point = indexed.point;
            let c = indexed.cell;
            if c.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }

            let mut fg = c.fg;
            // Classic terminals show bold text in the bright colour.
            if c.flags.contains(Flags::BOLD)
                && let Color::Named(n) = fg
                && (n as usize) < 8
            {
                fg = Color::Named(bright(n));
            }
            let mut fg = theme.resolve(fg, colors);
            let mut bg = theme.resolve(c.bg, colors);
            if c.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if c.flags.contains(Flags::DIM) {
                fg = fg.gamma_multiply(0.66);
            }

            let is_cursor = cursor_visible && point == cursor.point;
            if is_cursor && focused && cursor.shape == CursorShape::Block {
                bg = theme.cursor;
                fg = theme.background;
            }

            let width = if c.flags.contains(Flags::WIDE_CHAR) { 2.0 } else { 1.0 };
            let cell_rect = Rect::from_min_size(origin(point.line.0, point.column.0), Vec2::new(cell.x * width, cell.y));

            if bg != theme.background {
                painter.rect_filled(cell_rect, 0.0, bg);
            }
            if selection.is_some_and(|s| s.contains(point)) {
                painter.rect_filled(cell_rect, 0.0, theme.selection);
            }
            if is_cursor {
                paint_cursor(&painter, cell_rect, cursor.shape, focused, theme.cursor);
            }
            if c.flags.contains(Flags::ALL_UNDERLINES) {
                let y = cell_rect.max.y - 1.0;
                painter.hline(cell_rect.x_range(), y, egui::Stroke::new(1.0, fg));
            }

            // Grow the run or start a new one.
            let ch = if c.flags.contains(Flags::HIDDEN) { ' ' } else { c.c };
            let line = point.line.0;
            let col = point.column.0;
            let extends = matches!(&run, Some((l, start, text, color))
                if *l == line && *color == fg && ch.is_ascii()
                    && start + text.chars().count() == col);
            if extends {
                run.as_mut().unwrap().2.push(ch);
            } else {
                flush(&mut run);
                if ch != ' ' {
                    run = Some((line, col, ch.to_string(), fg));
                }
            }
            if !ch.is_ascii() {
                flush(&mut run);
            }
        }
        flush(&mut run);

        if self.session.exited() {
            painter.text(
                rect.right_bottom() - Vec2::new(8.0, 6.0),
                Align2::RIGHT_BOTTOM,
                "[process exited]",
                self.font.clone(),
                theme.ansi[8],
            );
        }
    }
}

fn paint_cursor(painter: &egui::Painter, rect: Rect, shape: CursorShape, focused: bool, color: Color32) {
    let stroke = egui::Stroke::new(1.5, color);
    match shape {
        CursorShape::Beam => {
            painter.vline(rect.min.x + 1.0, rect.y_range(), egui::Stroke::new(2.0, color));
        }
        CursorShape::Underline => {
            painter.hline(rect.x_range(), rect.max.y - 1.5, egui::Stroke::new(2.0, color));
        }
        CursorShape::Block if focused => {} // already painted as the cell background
        _ => {
            painter.rect_stroke(rect.shrink(0.75), 0.0, stroke, egui::StrokeKind::Inside);
        }
    }
}

fn bright(n: NamedColor) -> NamedColor {
    use NamedColor::*;
    match n {
        Black => BrightBlack,
        Red => BrightRed,
        Green => BrightGreen,
        Yellow => BrightYellow,
        Blue => BrightBlue,
        Magenta => BrightMagenta,
        Cyan => BrightCyan,
        White => BrightWhite,
        other => other,
    }
}

fn read_clipboard() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}
