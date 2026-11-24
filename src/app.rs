//! Window layout: connection sidebar, tab bar, terminal area, edit dialog.

use std::collections::BTreeMap;

use egui::{Color32, FontId, RichText};

use crate::config::{self, Connection};
use crate::terminal::{TerminalSession, TerminalView};
use crate::theme::Theme;

struct Tab {
    id: u64,
    label: String,
    session: TerminalSession,
}

pub struct SshmApp {
    connections: Vec<Connection>,
    tabs: Vec<Tab>,
    active: usize,
    next_tab_id: u64,
    theme: Theme,
    font_size: f32,
    /// The connection being added or edited in the dialog.
    editing: Option<Connection>,
    error: Option<String>,
    filter: String,
}

impl SshmApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let theme = Theme::default();
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(0x1b, 0x1e, 0x24);
        visuals.window_fill = Color32::from_rgb(0x20, 0x23, 0x2a);
        visuals.extreme_bg_color = theme.background;
        visuals.selection.bg_fill = theme.cursor.gamma_multiply(0.6);
        cc.egui_ctx.set_visuals(visuals);

        Self {
            connections: config::load(),
            tabs: Vec::new(),
            active: 0,
            next_tab_id: 1,
            theme,
            font_size: 14.0,
            editing: None,
            error: None,
            filter: String::new(),
        }
    }

    fn open(&mut self, ctx: &egui::Context, label: String, command: Option<(String, Vec<String>)>) {
        match TerminalSession::spawn(ctx, self.theme.clone(), command) {
            Ok(session) => {
                self.tabs.push(Tab { id: self.next_tab_id, label, session });
                self.next_tab_id += 1;
                self.active = self.tabs.len() - 1;
            }
            Err(e) => self.error = Some(format!("Could not start terminal: {e}")),
        }
    }

    fn persist(&mut self) {
        if let Err(e) = config::save(&self.connections) {
            self.error = Some(format!("Could not save connections: {e}"));
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.heading(RichText::new("SSHM").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("＋").on_hover_text("New connection").clicked() {
                    self.editing = Some(Connection::new_ssh());
                }
            });
        });
        ui.add_space(4.0);
        ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Search…"));
        ui.add_space(6.0);

        if ui.add_sized([ui.available_width(), 28.0], egui::Button::new("⌨  Local shell")).clicked() {
            self.open(ui.ctx(), "local".into(), None);
        }
        ui.separator();

        // Group by `group`, keeping the stored order inside each group.
        let needle = self.filter.to_lowercase();
        let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (i, c) in self.connections.iter().enumerate() {
            let haystack = format!("{} {} {}", c.display_name(), c.host, c.group).to_lowercase();
            if needle.is_empty() || haystack.contains(&needle) {
                let group = if c.group.is_empty() { "Ungrouped".to_string() } else { c.group.clone() };
                groups.entry(group).or_default().push(i);
            }
        }

        let mut to_open = None;
        let mut to_delete = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (group, indices) in &groups {
                egui::CollapsingHeader::new(RichText::new(group).small().strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        for &i in indices {
                            let c = &self.connections[i];
                            let row = ui
                                .add_sized(
                                    [ui.available_width(), 24.0],
                                    egui::Button::new(c.display_name()).frame(false),
                                )
                                .on_hover_text(format!("{}@{}:{}", c.username, c.host, c.port));
                            if row.double_clicked() || row.clicked() {
                                to_open = Some(i);
                            }
                            row.context_menu(|ui| {
                                if ui.button("Connect").clicked() {
                                    to_open = Some(i);
                                }
                                if ui.button("Edit…").clicked() {
                                    self.editing = Some(self.connections[i].clone());
                                }
                                if ui.button(RichText::new("Delete").color(self.theme.ansi[1])).clicked() {
                                    to_delete = Some(i);
                                }
                            });
                        }
                    });
            }
            if self.connections.is_empty() {
                ui.label(RichText::new("No saved connections yet.").weak());
            }
        });

        if let Some(i) = to_open {
            let c = self.connections[i].clone();
            self.open(ui.ctx(), c.display_name(), c.command());
        }
        if let Some(i) = to_delete {
            self.connections.remove(i);
            self.persist();
        }
    }

    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        let mut close = None;
        ui.horizontal(|ui| {
            for (i, tab) in self.tabs.iter().enumerate() {
                let title = tab.session.title().unwrap_or_else(|| tab.label.clone());
                let title: String = title.chars().take(32).collect();
                let selected = i == self.active;
                let dot = if tab.session.exited() { "○" } else { "●" };
                ui.scope(|ui| {
                    if ui.selectable_label(selected, format!("{dot} {title}")).clicked() {
                        self.active = i;
                    }
                    if ui.small_button("×").clicked() {
                        close = Some(i);
                    }
                });
                ui.separator();
            }
        });
        if let Some(i) = close {
            self.tabs.remove(i); // Drop shuts the session down.
            self.active = self.active.min(self.tabs.len().saturating_sub(1));
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(ctrl_shift, Key::T))) {
            self.open(ctx, "local".into(), None);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(ctrl_shift, Key::W))) && !self.tabs.is_empty() {
            self.tabs.remove(self.active);
            self.active = self.active.min(self.tabs.len().saturating_sub(1));
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::CTRL, Key::Tab))) && !self.tabs.is_empty() {
            self.active = (self.active + 1) % self.tabs.len();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(ctrl_shift, Key::Plus))) {
            self.font_size = (self.font_size + 1.0).min(32.0);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::CTRL, Key::Minus))) {
            self.font_size = (self.font_size - 1.0).max(8.0);
        }
    }

    fn edit_dialog(&mut self, ctx: &egui::Context) {
        let Some(conn) = self.editing.as_mut() else { return };
        let mut save = false;
        let mut cancel = false;

        let modal = egui::Modal::new(egui::Id::unique("edit-connection")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading(if conn.host.is_empty() { "New connection" } else { "Edit connection" });
            ui.add_space(8.0);
            egui::Grid::new("conn-form").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut conn.name);
                ui.end_row();
                ui.label("Host");
                ui.text_edit_singleline(&mut conn.host);
                ui.end_row();
                ui.label("Port");
                ui.add(egui::DragValue::new(&mut conn.port).range(1..=65535));
                ui.end_row();
                ui.label("User");
                ui.text_edit_singleline(&mut conn.username);
                ui.end_row();
                ui.label("Group");
                ui.text_edit_singleline(&mut conn.group);
                ui.end_row();
                ui.label("Auth");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut conn.auth, "password".to_string(), "Password");
                    ui.radio_value(&mut conn.auth, "key".to_string(), "Key file");
                });
                ui.end_row();
                if conn.auth == "key" {
                    ui.label("Key path");
                    ui.add(egui::TextEdit::singleline(&mut conn.key_path).hint_text("~/.ssh/id_ed25519"));
                    ui.end_row();
                }
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let can_save = !conn.host.trim().is_empty();
                if ui.add_enabled(can_save, egui::Button::new("Save")).clicked() {
                    save = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });

        if modal.should_close() {
            cancel = true;
        }
        if save {
            let conn = self.editing.take().unwrap();
            match self.connections.iter_mut().find(|c| c.id == conn.id) {
                Some(existing) => *existing = conn,
                None => self.connections.push(conn),
            }
            self.persist();
        } else if cancel {
            self.editing = None;
        }
    }
}

impl eframe::App for SshmApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.shortcuts(&ctx);

        egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(240.0)
            .size_range(180.0..=420.0)
            .show(ui, |ui| self.sidebar(ui));

        if let Some(err) = self.error.clone() {
            egui::Panel::bottom("error").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(self.theme.ansi[1], err);
                    if ui.small_button("dismiss").clicked() {
                        self.error = None;
                    }
                });
            });
        }

        if !self.tabs.is_empty() {
            egui::Panel::top("tabs").show(ui, |ui| self.tab_bar(ui));
        }

        egui::CentralPanel::no_frame().show(ui, |ui| {
            if let Some(tab) = self.tabs.get_mut(self.active) {
                let id = egui::Id::unique(("terminal", tab.id));
                let response = TerminalView {
                    session: &mut tab.session,
                    theme: &self.theme,
                    font: FontId::monospace(self.font_size),
                    id,
                }
                .show(ui);
                // Focus a freshly opened tab without needing a click.
                if ctx.memory(|m| m.focused().is_none()) && self.editing.is_none() {
                    response.request_focus();
                }
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new("Pick a connection, or press Ctrl+Shift+T for a local shell")
                            .color(self.theme.ansi[8]),
                    );
                });
            }
        });

        self.edit_dialog(&ctx);
    }
}
