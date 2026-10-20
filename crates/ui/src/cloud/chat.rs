//! The in-app chat for a document's conversation.
//!
//! The thin UI's own host of the conversation the provider keys on a document.
//! It holds the messages, polls for new ones on its own clock, and sends.
//! Nothing here opens a socket: it states [`Job::Messages`] and
//! [`Job::PostMessage`] and the host answers through the same channel the rest
//! of the cloud pages use. A provider that only offers a call keeps the
//! pop-out, which hands a URL to the provider's own app.

use egui::{Align, Key, Layout, RichText, ScrollArea, TextEdit, Ui};

use super::{CloudApp, Fault, Job, submit, work_together};
use craft_cloud::ChatMessage;

/// How long an open chat waits between asking for new messages.
const POLL_SECONDS: f64 = 4.0;

/// The document whose conversation is open.
struct Open {
    uid: u64,
    /// The provider's handle for the document, which the chat is keyed on.
    id: String,
    /// What to show at the top of the panel.
    title: String,
}

/// One document's conversation, as the thin UI shows it.
#[derive(Default)]
pub struct ChatPanel {
    open: Option<Open>,
    messages: Vec<ChatMessage>,
    draft: String,
    /// The newest id held, asked for more after on the next poll.
    last_id: Option<String>,
    /// When to poll next, on the cloud clock (seconds).
    next_poll: f64,
    /// The first page is still on the way.
    loading: bool,
    /// A send is on the wire; the composer waits for its echo.
    sending: bool,
    /// The last thing that went wrong, for the line under the composer.
    fault: Option<String>,
}

impl ChatPanel {
    /// Whether a conversation is open.
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Open the conversation for a document, starting fresh for a new one.
    pub fn open_document(&mut self, uid: u64, id: String, title: String) {
        if self.open.as_ref().map(|o| o.uid) != Some(uid) {
            self.messages.clear();
            self.last_id = None;
            self.draft.clear();
            self.fault = None;
        }
        self.open = Some(Open { uid, id, title });
        self.loading = true;
        self.next_poll = 0.0;
    }

    /// Close the panel. The messages are kept for the next open of the same
    /// document, which a poll refreshes.
    pub fn close(&mut self) {
        self.open = None;
    }

    /// Fold in a page of messages (from [`super::Event::Messages`]).
    pub fn received(&mut self, result: Result<Vec<ChatMessage>, Fault>, noun: &str) {
        self.loading = false;
        match result {
            Ok(page) => {
                self.fault = None;
                for m in page {
                    self.absorb(m);
                }
            }
            Err(fault) => self.fault = Some(fault.text(noun)),
        }
    }

    /// Fold in a sent message (from [`super::Event::Posted`]).
    pub fn posted(&mut self, result: Result<ChatMessage, Fault>, noun: &str) {
        self.sending = false;
        match result {
            Ok(m) => {
                self.fault = None;
                self.absorb(m);
            }
            Err(fault) => self.fault = Some(fault.text(noun)),
        }
    }

    /// Add a message unless its id is already held, keeping `last_id` current.
    fn absorb(&mut self, m: ChatMessage) {
        if self.messages.iter().any(|x| x.id == m.id) {
            return;
        }
        self.last_id = Some(m.id.clone());
        self.messages.push(m);
    }
}

/// Open the chat for the active document, if there is a conversation to have.
///
/// A document that has never been in the cloud has no handle to key a room on,
/// so this says to save it first, exactly as the pop-out does.
pub fn open_chat<A: CloudApp>(app: &mut A) {
    if !super::available(app) {
        return;
    }
    let Some((id, title)) = super::origin(app).map(|o| (o.entry.id.clone(), o.entry.name.clone())) else {
        let noun = app.kind().noun;
        app.cloud_mut().status = Some(app.tr(&format!("Save this {noun} to the cloud first")));
        return;
    };
    let Some(uid) = app.active() else { return };
    app.cloud_mut().chat.open_document(uid, id, title);
}

/// Ask for new messages when the open chat is due a poll.
///
/// Driven from [`super::pump`], so an open chat follows along without the host
/// scheduling anything. A chat that is loading or mid-send waits its turn.
pub fn poll<A: CloudApp>(app: &mut A, now: f64) {
    let job = {
        let chat = &app.cloud().chat;
        match &chat.open {
            Some(open) if now >= chat.next_poll && !chat.sending => Some((open.id.clone(), chat.last_id.clone())),
            _ => None,
        }
    };
    if let Some((id, after)) = job {
        app.cloud_mut().chat.next_poll = now + POLL_SECONDS;
        submit(app, Job::Messages { id, after });
    }
}

/// Draw the chat in `ui`, a panel the host places where it likes.
pub fn show_in<A: CloudApp>(app: &mut A, ui: &mut Ui) {
    if app.cloud().chat.open.is_none() {
        return;
    }
    let title = app.cloud().chat.open.as_ref().map(|o| o.title.clone()).unwrap_or_default();
    let send_label = app.tr("Send");
    let hint = app.tr("Write a message");
    let close_label = app.tr("Close");
    let popout_label = app.tr("Open in the provider");
    let empty = app.tr("No messages yet.");
    let loading = app.tr("Loading…");

    let mut send: Option<String> = None;
    let mut close = false;
    let mut popout = false;
    {
        let chat = &mut app.cloud_mut().chat;
        let dim = ui.visuals().weak_text_color();
        ui.horizontal(|ui| {
            ui.label(RichText::new(&title).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("✕").on_hover_text(&close_label).clicked() {
                    close = true;
                }
                if ui.small_button("↗").on_hover_text(&popout_label).clicked() {
                    popout = true;
                }
            });
        });
        ui.separator();
        ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).max_height(ui.available_height() - 40.0).show(ui, |ui| {
            if chat.messages.is_empty() {
                ui.label(RichText::new(if chat.loading { &loading } else { &empty }).color(dim));
            }
            for m in &chat.messages {
                let layout = if m.mine { Layout::right_to_left(Align::Min) } else { Layout::left_to_right(Align::Min) };
                ui.with_layout(layout, |ui| {
                    ui.allocate_ui(egui::vec2(ui.available_width() * 0.82, 0.0), |ui| {
                        ui.vertical(|ui| {
                            if !m.mine {
                                ui.label(RichText::new(&m.author).small().color(dim));
                            }
                            ui.label(&m.text);
                        });
                    });
                });
            }
        });
        if let Some(fault) = &chat.fault {
            ui.label(RichText::new(fault).small().color(ui.visuals().error_fg_color));
        }
        ui.separator();
        ui.horizontal(|ui| {
            let resp = ui.add(TextEdit::singleline(&mut chat.draft).hint_text(&hint).desired_width(ui.available_width() - 64.0));
            let entered = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            let clicked = ui.add_enabled(!chat.sending, egui::Button::new(&send_label)).clicked();
            if (entered || clicked) && !chat.draft.trim().is_empty() && !chat.sending {
                send = Some(std::mem::take(&mut chat.draft).trim().to_owned());
                resp.request_focus();
            }
        });
    }

    if close {
        app.cloud_mut().chat.close();
    }
    if let Some(text) = send {
        if let Some(id) = app.cloud().chat.open.as_ref().map(|o| o.id.clone()) {
            app.cloud_mut().chat.sending = true;
            submit(app, Job::PostMessage { id, text });
        }
    }
    if popout {
        work_together(app);
    }
}
