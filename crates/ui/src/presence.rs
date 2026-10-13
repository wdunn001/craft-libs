//! Who else is in the document.
//!
//! A person editing alone and a person editing alongside three colleagues are
//! in very different situations. Only one of them has to worry about somebody
//! else changing the cell they are about to type in. An application that shows
//! nothing leaves them to find out by surprise.
//!
//! So this is a row of badges, one per person, with the count folded into a
//! final badge once the row would get long. It is the same control every
//! collaborative editor puts in the same corner, for the same reason.
//!
//! A peer's colour comes from the peer's own identifier. Two things need to
//! agree on it: the badge here, and whatever the document draws at that
//! person's cursor. Deriving it rather than assigning it means they agree with
//! no state passed between them, and the colour survives a reconnection.

use egui::{Align, Color32, Layout, Response, RichText, Ui};

/// How wide a badge is, and how far the next one overlaps it.
const BADGE: f32 = 24.0;
const OVERLAP: f32 = 7.0;

/// Somebody else in the document right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    /// The session's own identifier, stable for as long as they are connected.
    pub id: String,
    /// What to show. A display name, never an identity.
    pub display: String,
    /// Where they are working, when the document can say. A spreadsheet knows
    /// the cell. A word processor knows the page. Shown on hover only.
    pub at: Option<String>,
}

impl Peer {
    pub fn new(id: impl Into<String>, display: impl Into<String>) -> Peer {
        Peer { id: id.into(), display: display.into(), at: None }
    }

    /// Where they are working, for the tooltip.
    pub fn at(mut self, at: impl Into<String>) -> Peer {
        self.at = Some(at.into());
        self
    }

    /// The letter on the badge.
    pub fn initial(&self) -> String {
        self.display.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".to_owned())
    }

    /// This peer's colour.
    pub fn colour(&self) -> Color32 {
        colour_for(&self.id)
    }
}

/// A stable colour for an identifier.
///
/// Public because the badge is not the only thing that needs it. A remote
/// cursor drawn in a different colour from the badge above it reads as two
/// different people.
pub fn colour_for(id: &str) -> Color32 {
    // FNV-1a, which is small, has no dependency and spreads short strings well
    // enough to keep neighbouring session ids apart.
    let mut hash: u32 = 0x811c_9dc5;
    for byte in id.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    // A fixed saturation and value, so every badge carries the same weight and
    // no peer ends up with a colour that cannot hold white text.
    hsv(f32::from(u16::try_from(hash % 360).unwrap_or(0)), 0.58, 0.80)
}

fn hsv(hue: f32, saturation: f32, value: f32) -> Color32 {
    let c = value * saturation;
    let h = hue / 60.0;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = value - c;
    let to_byte = |f: f32| (f.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(to_byte(r + m), to_byte(g + m), to_byte(b + m))
}

/// Strings, so an application can translate them.
#[derive(Clone, Debug)]
pub struct PresenceLabels {
    /// Said when nobody else is here. An application may leave this empty to
    /// draw nothing at all in that case.
    pub alone: String,
    /// With one other person. `{name}` is replaced.
    pub with_one: String,
    /// With several. `{count}` is replaced.
    pub with_many: String,
    /// The badge that stands for the people who did not fit. `{count}` is
    /// replaced.
    pub and_more: String,
    /// The accessible name of the whole row.
    pub who: String,
}

impl Default for PresenceLabels {
    fn default() -> PresenceLabels {
        PresenceLabels {
            alone: String::new(),
            with_one: "Editing with {name}".into(),
            with_many: "Editing with {count} others".into(),
            and_more: "{count} more".into(),
            who: "Who is here".into(),
        }
    }
}

/// The row of badges.
#[derive(Clone)]
pub struct PresenceBar {
    peers: Vec<Peer>,
    /// How many badges to draw before folding the rest into a count. Four is
    /// what fits beside a title bar without crowding it.
    shown: usize,
    labels: PresenceLabels,
    /// Whether to write the sentence beside the badges. A narrow toolbar wants
    /// the badges alone.
    summary: bool,
}

impl Default for PresenceBar {
    fn default() -> PresenceBar {
        PresenceBar::new()
    }
}

impl PresenceBar {
    pub fn new() -> PresenceBar {
        PresenceBar { peers: Vec::new(), shown: 4, labels: PresenceLabels::default(), summary: true }
    }

    pub fn with_labels(mut self, labels: PresenceLabels) -> PresenceBar {
        self.labels = labels;
        self
    }

    /// How many badges before the row folds into a count.
    pub fn with_shown(mut self, shown: usize) -> PresenceBar {
        self.shown = shown.max(1);
        self
    }

    /// Draw the badges only, with no sentence beside them.
    pub fn badges_only(mut self) -> PresenceBar {
        self.summary = false;
        self
    }

    /// Replace the set of people in the document.
    ///
    /// Takes the whole set rather than a change, because a session that missed
    /// a departure would otherwise show somebody who left until the next
    /// reconnection.
    pub fn set(&mut self, peers: Vec<Peer>) {
        self.peers = peers;
    }

    pub fn peers(&self) -> &[Peer] {
        &self.peers
    }

    /// Whether anybody else is here. An application draws nothing when this is
    /// false, which keeps a solitary document looking like one.
    pub fn shared(&self) -> bool {
        !self.peers.is_empty()
    }

    /// What the row says in words. Also the accessible summary.
    pub fn summary(&self) -> String {
        match self.peers.len() {
            0 => self.labels.alone.clone(),
            1 => self.peers.first().map(|p| self.labels.with_one.replace("{name}", &p.display)).unwrap_or_default(),
            n => self.labels.with_many.replace("{count}", &n.to_string()),
        }
    }

    /// Draw it. Answers with the peer a person clicked, when they clicked one.
    ///
    /// A click is worth answering because a name is where "take me to what
    /// they are editing" belongs. The application decides whether it can.
    pub fn show(&mut self, ui: &mut Ui) -> Option<String> {
        if self.peers.is_empty() {
            if !self.labels.alone.is_empty() {
                ui.label(RichText::new(self.labels.alone.as_str()).small().color(ui.visuals().weak_text_color()));
            }
            return None;
        }

        let mut clicked = None;
        ui.horizontal(|ui| {
            // Badges overlap, the way a stack of cards does. Without it four
            // circles read as four separate controls instead of one group.
            ui.spacing_mut().item_spacing.x = -OVERLAP;
            // Right to left, so the first person sits on top of the stack.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let hidden = self.peers.len().saturating_sub(self.shown);
                if hidden > 0 {
                    let more = self.labels.and_more.replace("{count}", &hidden.to_string());
                    let weak = ui.visuals().widgets.inactive.bg_fill;
                    let badge = badge(ui, &format!("+{hidden}"), weak, ui.visuals().text_color());
                    badge.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, more.as_str()));
                    badge.on_hover_text(self.peers.iter().skip(self.shown).map(|p| p.display.as_str()).collect::<Vec<_>>().join("\n"));
                }
                for peer in self.peers.iter().take(self.shown).rev() {
                    let hit = badge(ui, &peer.initial(), peer.colour(), Color32::WHITE);
                    let name = peer.display.clone();
                    hit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name.as_str()));
                    let tip = match &peer.at {
                        Some(at) => format!("{}\n{at}", peer.display),
                        None => peer.display.clone(),
                    };
                    if hit.on_hover_text(tip).clicked() {
                        clicked = Some(peer.id.clone());
                    }
                }
            });
            if self.summary {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(RichText::new(self.summary()).small().color(ui.visuals().weak_text_color()));
            }
        });
        clicked
    }
}

/// One round badge. Painted as a button so it is focusable and announces
/// itself, which a bare circle would not.
fn badge(ui: &mut Ui, text: &str, fill: Color32, ink: Color32) -> Response {
    let button = egui::Button::new(RichText::new(text).color(ink).size(11.0).strong())
        .fill(fill)
        .corner_radius(BADGE / 2.0)
        .min_size(egui::vec2(BADGE, BADGE))
        .stroke(egui::Stroke::new(1.5, ui.visuals().panel_fill));
    ui.add(button)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_is_the_same_every_time_for_the_same_peer() {
        assert_eq!(colour_for("session-a"), colour_for("session-a"));
    }

    #[test]
    fn two_peers_do_not_share_a_colour() {
        let a = colour_for("session-a");
        let b = colour_for("session-b");
        assert_ne!(a, b, "neighbouring session ids must not land on one colour");
    }

    #[test]
    fn a_colour_is_dark_enough_to_hold_white_text() {
        for id in ["a", "b", "zzz", "session-41", "", "\u{1f600}"] {
            let c = colour_for(id);
            // Rec. 601 luma, which is what readability of white ink tracks.
            let luma = 0.299 * f32::from(c.r()) + 0.587 * f32::from(c.g()) + 0.114 * f32::from(c.b());
            assert!(luma < 200.0, "{id:?} gave a colour too pale for white text, luma {luma}");
        }
    }

    #[test]
    fn an_empty_room_is_not_shared_and_says_nothing() {
        let bar = PresenceBar::new();
        assert!(!bar.shared());
        assert_eq!(bar.summary(), "");
    }

    #[test]
    fn one_other_person_is_named_and_several_are_counted() {
        let mut bar = PresenceBar::new();
        bar.set(vec![Peer::new("1", "Ann Reed")]);
        assert_eq!(bar.summary(), "Editing with Ann Reed");
        bar.set(vec![Peer::new("1", "Ann Reed"), Peer::new("2", "Bob Vine")]);
        assert_eq!(bar.summary(), "Editing with 2 others");
    }

    #[test]
    fn a_departure_leaves_nobody_behind() {
        let mut bar = PresenceBar::new();
        bar.set(vec![Peer::new("1", "Ann Reed"), Peer::new("2", "Bob Vine")]);
        bar.set(vec![Peer::new("1", "Ann Reed")]);
        assert_eq!(bar.peers().len(), 1, "the whole set replaces the old one");
    }

    #[test]
    fn an_initial_survives_a_name_that_starts_oddly() {
        assert_eq!(Peer::new("1", "ann").initial(), "A");
        assert_eq!(Peer::new("1", "").initial(), "?");
        assert_eq!(Peer::new("1", "\u{e9}lodie").initial(), "\u{c9}");
    }
}
