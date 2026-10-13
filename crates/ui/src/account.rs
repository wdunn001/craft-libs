//! Who is signed in, which account is in use, and how to stop being signed in.
//!
//! Every office suite puts this in the same place, for the same reason. A
//! document is about to be saved somewhere and shared with somebody. The
//! account decides both the destination and the identity. An application that
//! never shows it leaves the user guessing which of their identities is in
//! play.
//!
//! A person commonly holds more than one. A work account and a personal one
//! are two different places to save, with two different sets of colleagues to
//! share with, and confusing them is the expensive mistake this control exists
//! to prevent. So the control holds a list and marks which one is in use.
//!
//! This holds no credential. It asks the service who the caller is and shows
//! the answer. A sign-out makes the service forget the session.

use std::sync::mpsc::{Receiver, Sender, channel};

use egui::{Align, Context, Layout, RichText, Ui};

/// One account the user is signed in to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Account {
    /// How the service names this account. Never shown.
    pub id: String,
    /// What to show. A display name, never an identity.
    pub display: String,
    pub email: Option<String>,
    /// Where they are signed in, such as the service's host. Shown because
    /// somebody with accounts on two deployments needs to tell them apart.
    pub service: String,
    /// Whether storage is linked. Without it Open and Save have nowhere to go,
    /// and saying so here is kinder than a failure at the moment of saving.
    pub linked: bool,
}

impl Account {
    pub fn new(id: impl Into<String>, display: impl Into<String>) -> Account {
        Account { id: id.into(), display: display.into(), ..Account::default() }
    }

    /// The letter on the badge. The first character of the display name, which
    /// is what every suite uses and what people recognise at that size.
    pub fn initial(&self) -> String {
        self.display.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".to_owned())
    }
}

/// What the menu asks the application to do.
///
/// Each one names the account it is about. A control that said only "sign out"
/// would be ambiguous the moment somebody held two.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccountAction {
    /// Begin a sign-in with nobody signed in yet.
    SignIn,
    /// Add another account alongside the ones already here.
    AddAccount,
    /// Work as this account from now on.
    Use(String),
    /// End this account's session, at the service and here.
    SignOut(String),
    /// Connect storage for an account that has none.
    Link(String),
    /// Open the service's own page for this account.
    Manage(String),
}

/// Strings, so an application can translate them.
#[derive(Clone, Debug)]
pub struct AccountLabels {
    pub signed_out: String,
    pub sign_in: String,
    pub sign_out: String,
    pub add_account: String,
    pub other_accounts: String,
    pub link: String,
    pub linked: String,
    pub not_linked: String,
    pub manage: String,
    pub working: String,
}

impl Default for AccountLabels {
    fn default() -> AccountLabels {
        AccountLabels {
            signed_out: "Not signed in".into(),
            sign_in: "Sign in".into(),
            sign_out: "Sign out".into(),
            add_account: "Add an account".into(),
            other_accounts: "Other accounts".into(),
            link: "Connect storage".into(),
            linked: "Storage connected".into(),
            not_linked: "No storage connected".into(),
            manage: "Account settings".into(),
            working: "Working\u{2026}".into(),
        }
    }
}

/// The account badge and its menu.
pub struct AccountMenu {
    accounts: Vec<Account>,
    /// Which account the application is working as. Held by id rather than by
    /// index, since the list is replaced wholesale whenever the service answers.
    active: Option<String>,
    labels: AccountLabels,
    /// Set while a sign-in or sign-out is in flight, so the control says so
    /// rather than looking unresponsive.
    busy: bool,
    actions: (Sender<AccountAction>, Receiver<AccountAction>),
}

impl Default for AccountMenu {
    fn default() -> AccountMenu {
        AccountMenu::new()
    }
}

impl AccountMenu {
    pub fn new() -> AccountMenu {
        AccountMenu { accounts: Vec::new(), active: None, labels: AccountLabels::default(), busy: false, actions: channel() }
    }

    pub fn with_labels(mut self, labels: AccountLabels) -> AccountMenu {
        self.labels = labels;
        self
    }

    /// Replace the whole set of accounts.
    ///
    /// Wholesale rather than one at a time. A session that missed a sign-out
    /// elsewhere would otherwise keep offering an account that is gone.
    ///
    /// The active account is kept when it is still in the list. Otherwise the
    /// first becomes active, since a list with nobody marked would leave every
    /// save without a destination.
    pub fn set(&mut self, accounts: Vec<Account>) {
        let kept = self.active.take().filter(|id| accounts.iter().any(|a| a.id == *id));
        self.active = kept.or_else(|| accounts.first().map(|a| a.id.clone()));
        self.accounts = accounts;
        self.busy = false;
    }

    /// Work as this account. Ignored for an account that is not here.
    pub fn set_active(&mut self, id: &str) {
        if self.accounts.iter().any(|a| a.id == id) {
            self.active = Some(id.to_owned());
        }
    }

    pub fn active(&self) -> Option<&Account> {
        let id = self.active.as_deref()?;
        self.accounts.iter().find(|a| a.id == id)
    }

    pub fn accounts(&self) -> &[Account] {
        &self.accounts
    }

    pub fn is_signed_in(&self) -> bool {
        !self.accounts.is_empty()
    }

    /// Mark something as in flight. Cleared by the next [`AccountMenu::set`].
    pub fn working(&mut self) {
        self.busy = true;
    }

    /// What the user asked for since the last call, if anything.
    ///
    /// Drained rather than handed a callback, so the application acts on its
    /// own thread and this stays free of any notion of what signing in means.
    pub fn take_action(&mut self) -> Option<AccountAction> {
        self.actions.1.try_recv().ok()
    }

    /// Draw the badge. Put it where the window's own controls are.
    pub fn show(&mut self, ui: &mut Ui) {
        let tx = self.actions.0.clone();
        let labels = self.labels.clone();
        let busy = self.busy;

        let Some(account) = self.active().cloned() else {
            // Signed out. A word, because a bare badge with no name on it says
            // nothing about what pressing it would do.
            let label = if busy { labels.working.as_str() } else { labels.sign_in.as_str() };
            if ui.add_enabled(!busy, egui::Button::new(label).small()).clicked() {
                let _ = tx.send(AccountAction::SignIn);
            }
            return;
        };

        let others: Vec<Account> = self.accounts.iter().filter(|a| a.id != account.id).cloned().collect();
        let badge = egui::Button::new(RichText::new(account.initial()).strong()).min_size(egui::vec2(26.0, 26.0)).corner_radius(13.0);
        let hit = ui.add(badge);
        let name = account.display.clone();
        hit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name.as_str()));
        let hit = hit.on_hover_text(account.display.as_str());

        egui::Popup::menu(&hit).show(|ui| {
            ui.set_min_width(260.0);
            // The deployment first, because somebody with an account on two of
            // them needs to know which one this is before reading anything
            // else.
            ui.horizontal(|ui| {
                ui.label(RichText::new(account.service.as_str()).small().color(ui.visuals().weak_text_color()));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button(labels.sign_out.as_str()).clicked() {
                        let _ = tx.send(AccountAction::SignOut(account.id.clone()));
                        ui.close();
                    }
                });
            });
            ui.separator();

            ui.horizontal(|ui| {
                ui.label(RichText::new(account.initial()).size(26.0).strong());
                ui.vertical(|ui| {
                    ui.label(RichText::new(account.display.as_str()).strong().size(15.0));
                    if let Some(email) = &account.email {
                        ui.label(RichText::new(email.as_str()).small().color(ui.visuals().weak_text_color()));
                    }
                });
            });

            ui.add_space(6.0);

            // Storage, which is the thing that actually stops working when it
            // is missing. Said here rather than discovered at the moment of
            // saving.
            if account.linked {
                ui.label(RichText::new(labels.linked.as_str()).small().color(ui.visuals().weak_text_color()));
            } else {
                ui.label(RichText::new(labels.not_linked.as_str()).small().color(ui.visuals().warn_fg_color));
                if ui.button(labels.link.as_str()).clicked() {
                    let _ = tx.send(AccountAction::Link(account.id.clone()));
                    ui.close();
                }
            }

            if !others.is_empty() {
                ui.add_space(6.0);
                ui.separator();
                ui.label(RichText::new(labels.other_accounts.as_str()).small().color(ui.visuals().weak_text_color()));
                for other in &others {
                    let row = ui.button(other.display.as_str());
                    // The deployment under each one. Two accounts reading
                    // "Will Dunn" tell nobody which is the work one.
                    ui.label(RichText::new(other.service.as_str()).small().color(ui.visuals().weak_text_color()));
                    if row.clicked() {
                        let _ = tx.send(AccountAction::Use(other.id.clone()));
                        ui.close();
                    }
                }
            }

            ui.add_space(6.0);
            if ui.button(labels.add_account.as_str()).clicked() {
                let _ = tx.send(AccountAction::AddAccount);
                ui.close();
            }

            ui.add_space(4.0);
            if ui.link(labels.manage.as_str()).clicked() {
                let _ = tx.send(AccountAction::Manage(account.id.clone()));
                ui.close();
            }
        });
    }

    /// Draw the badge in a context that has no `Ui` to hand.
    pub fn show_in_ctx(&mut self, ctx: &Context) {
        egui::Area::new(egui::Id::new("craft-ui/account")).anchor(egui::Align2::RIGHT_TOP, egui::vec2(-8.0, 8.0)).show(ctx, |ui| {
            self.show(ui);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str, display: &str) -> Account {
        Account { id: id.into(), display: display.into(), service: "one.example.com".into(), ..Account::default() }
    }

    #[test]
    fn nobody_signed_in_has_no_active_account() {
        let menu = AccountMenu::new();
        assert!(!menu.is_signed_in());
        assert_eq!(menu.active(), None);
    }

    #[test]
    fn the_first_account_becomes_the_active_one() {
        let mut menu = AccountMenu::new();
        menu.set(vec![account("a", "Work"), account("b", "Personal")]);
        assert_eq!(menu.active().map(|a| a.id.as_str()), Some("a"));
    }

    #[test]
    fn replacing_the_list_keeps_the_account_in_use() {
        let mut menu = AccountMenu::new();
        menu.set(vec![account("a", "Work"), account("b", "Personal")]);
        menu.set_active("b");
        menu.set(vec![account("a", "Work"), account("b", "Personal"), account("c", "Lab")]);
        assert_eq!(menu.active().map(|a| a.id.as_str()), Some("b"), "an unrelated refresh must not switch accounts");
    }

    #[test]
    fn an_account_that_signed_out_elsewhere_does_not_stay_active() {
        let mut menu = AccountMenu::new();
        menu.set(vec![account("a", "Work"), account("b", "Personal")]);
        menu.set_active("b");
        menu.set(vec![account("a", "Work")]);
        assert_eq!(menu.active().map(|a| a.id.as_str()), Some("a"), "the one that is gone cannot stay in use");
    }

    #[test]
    fn the_last_account_leaving_signs_everybody_out() {
        let mut menu = AccountMenu::new();
        menu.set(vec![account("a", "Work")]);
        menu.set(Vec::new());
        assert!(!menu.is_signed_in());
        assert_eq!(menu.active(), None);
    }

    #[test]
    fn switching_to_an_account_that_is_not_here_changes_nothing() {
        let mut menu = AccountMenu::new();
        menu.set(vec![account("a", "Work")]);
        menu.set_active("nobody");
        assert_eq!(menu.active().map(|a| a.id.as_str()), Some("a"));
    }

    #[test]
    fn an_initial_survives_a_name_that_starts_oddly() {
        assert_eq!(account("1", "ann").initial(), "A");
        assert_eq!(account("1", "").initial(), "?");
        assert_eq!(account("1", "\u{e9}lodie").initial(), "\u{c9}");
    }
}
