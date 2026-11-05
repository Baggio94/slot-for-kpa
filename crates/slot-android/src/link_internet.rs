//! The Internet tab of the Multiplayer menu: what it shows and the room code
//! being typed. Pure state, no GL or sockets, so it can be tested on Linux;
//! runtime.rs turns `heading()` into a face and routes keys here.
//!
//!   L1 / R1     Local / Internet tab (on the Host/Join picker)
//!   Host, A     the relay opens a room; its code is shown, Y shares it
//!   Join, A     type the six digits: Up/Down change, Left/Right move, A joins

use std::path::Path;

use crate::app::{GameMenu, LinkRow};
use crate::link_relay::{display_code, normalize_code, CODE_LEN};

/// The relay this build talks to, from `SLOT_RELAY_ADDR` at build time.
/// A `multiplayer-relay.txt` in slot.'s private files folder overrides it
/// (developers, over `adb shell run-as`) without a rebuild.
pub fn relay_address(storage: Option<&Path>) -> Option<String> {
    let from_file = storage
        .and_then(|dir| std::fs::read_to_string(dir.join("multiplayer-relay.txt")).ok())
        .and_then(|text| text.lines().map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with('#')).map(str::to_owned));
    from_file.or_else(|| option_env!("SLOT_RELAY_ADDR")
        .map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned))
}

/// What the host sends a friend, by WhatsApp or anything else.
pub fn share_text(code: &str, game: &str) -> String {
    let game = game.trim();
    let what = if game.is_empty() { String::new() } else { format!(" {game}") };
    format!("Play{what} with me on slot.! Open the game, Multiplayer, R1 Internet, \
             Join, and enter the code {}", display_code(code))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeEntry {
    digits: [u8; CODE_LEN],
    cursor: usize,
}

impl CodeEntry {
    /// Starts from the last code used, if any, so a retry is one press.
    pub fn new(previous: Option<&str>) -> Self {
        let mut digits = [0u8; CODE_LEN];
        if let Some(code) = previous.and_then(normalize_code) {
            for (d, b) in digits.iter_mut().zip(code.bytes()) { *d = b - b'0'; }
        }
        CodeEntry { digits, cursor: 0 }
    }

    pub fn up(&mut self) { self.digits[self.cursor] = (self.digits[self.cursor] + 1) % 10; }
    pub fn down(&mut self) { self.digits[self.cursor] = (self.digits[self.cursor] + 9) % 10; }
    pub fn left(&mut self) { self.cursor = (self.cursor + CODE_LEN - 1) % CODE_LEN; }
    pub fn right(&mut self) { self.cursor = (self.cursor + 1) % CODE_LEN; }

    /// A keyboard digit: set it and move on.
    pub fn type_digit(&mut self, digit: u8) {
        self.digits[self.cursor] = digit % 10;
        if self.cursor + 1 < CODE_LEN { self.cursor += 1; }
    }

    pub fn code(&self) -> String {
        self.digits.iter().map(|d| char::from(b'0' + d)).collect()
    }

    /// "4 8 [2] 9 1 3": the digit being changed is in brackets.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for (i, d) in self.digits.iter().enumerate() {
            if i > 0 { out.push(' '); }
            if i == 3 { out.push(' '); }
            if i == self.cursor { out.push_str(&format!("[{d}]")); }
            else { out.push(char::from(b'0' + d)); }
        }
        out
    }
}

/// Android keycodes the code entry listens to.
pub const KEY_UP: i32 = 19;
pub const KEY_DOWN: i32 = 20;
pub const KEY_LEFT: i32 = 21;
pub const KEY_RIGHT: i32 = 22;
pub const KEY_0: i32 = 7;
pub const KEY_9: i32 = 16;
pub const KEY_L1: i32 = 102;
pub const KEY_R1: i32 = 103;
pub const KEY_Y: i32 = 100;

#[derive(Clone, Debug, Default)]
pub struct InternetUi {
    /// The Internet tab is selected (L1/R1).
    pub internet: bool,
    /// Join is typing a room code.
    pub entry: Option<CodeEntry>,
    /// The current connection attempt goes through the relay.
    pub session: bool,
    /// Host: the code the relay gave this room.
    pub room: Option<String>,
    /// Join: the code being joined.
    pub joining: Option<String>,
    /// The last code typed, offered again next time.
    pub last_code: Option<String>,
}

impl InternetUi {
    /// The connection attempt ended, however it ended.
    pub fn end_attempt(&mut self) {
        self.session = false;
        self.room = None;
        self.joining = None;
    }

    /// The heading to show instead of the stock Host/Join/progress face, or
    /// `None` for the stock one (everything on the Local tab, and any state
    /// the Internet tab does not change).
    pub fn heading(&self, menu: GameMenu) -> Option<String> {
        if let Some(entry) = &self.entry {
            return Some(format!("Code {}", entry.text()));
        }
        match menu {
            GameMenu::Pick(role) if self.internet => Some(match role {
                LinkRow::Host => "Host over Internet".into(),
                LinkRow::Join => "Join over Internet".into(),
            }),
            GameMenu::Working { role, .. } if self.session => Some(match role {
                LinkRow::Host => match &self.room {
                    Some(code) => format!("Code {}", display_code(code)),
                    None => "Opening a room".into(),
                },
                LinkRow::Join => match &self.joining {
                    Some(code) => format!("Joining {}", display_code(code)),
                    None => "Joining".into(),
                },
            }),
            _ => None,
        }
    }

    /// Y shares the code while a host waits for its friend.
    pub fn can_share(&self, menu: GameMenu) -> bool {
        self.session && self.room.is_some()
            && matches!(menu, GameMenu::Working { role: LinkRow::Host, .. })
    }

    /// An Internet host waiting for its friend keeps its room while slot.
    /// is in the background: that is when the code is being sent.
    pub fn keeps_room_in_background(&self, menu: GameMenu) -> bool {
        self.session && matches!(menu, GameMenu::Working { role: LinkRow::Host, .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::LinkStep;

    #[test]
    fn code_entry_wraps_digits_and_cursor() {
        let mut e = CodeEntry::new(None);
        assert_eq!(e.code(), "000000");
        e.down();
        assert_eq!(e.code(), "900000");
        e.up(); e.up();
        assert_eq!(e.code(), "100000");
        e.left();
        e.up();
        assert_eq!(e.code(), "100001");
        e.right(); // back to the first digit
        e.type_digit(4); e.type_digit(8); e.type_digit(2);
        e.type_digit(9); e.type_digit(1); e.type_digit(3); e.type_digit(7);
        assert_eq!(e.code(), "482917", "the last digit is overwritten, never a seventh");
        assert_eq!(CodeEntry::new(Some("482 913")).code(), "482913");
        assert_eq!(CodeEntry::new(Some("12")).code(), "000000");
    }

    #[test]
    fn the_digit_being_changed_is_bracketed() {
        let mut e = CodeEntry::new(Some("482913"));
        assert_eq!(e.text(), "[4] 8 2  9 1 3");
        e.left();
        assert_eq!(e.text(), "4 8 2  9 1 [3]");
    }

    #[test]
    fn headings_follow_the_tab_and_the_attempt() {
        let mut ui = InternetUi::default();
        assert_eq!(ui.heading(GameMenu::Pick(LinkRow::Host)), None, "Local keeps stock faces");
        ui.internet = true;
        assert_eq!(ui.heading(GameMenu::Pick(LinkRow::Join)).as_deref(), Some("Join over Internet"));
        ui.session = true;
        let waiting = GameMenu::Working { role: LinkRow::Host, step: LinkStep::Waiting, since: 0 };
        assert_eq!(ui.heading(waiting).as_deref(), Some("Opening a room"));
        assert!(!ui.can_share(waiting));
        ui.room = Some("482913".into());
        assert_eq!(ui.heading(waiting).as_deref(), Some("Code 482 913"));
        assert!(ui.can_share(waiting));
        assert!(ui.keeps_room_in_background(waiting));
        ui.end_attempt();
        assert_eq!(ui.heading(waiting), None);
        assert!(!ui.keeps_room_in_background(waiting));
    }

    #[test]
    fn share_text_names_the_game_and_the_code() {
        let text = share_text("482913", "Pokémon Emerald");
        assert!(text.contains("Pokémon Emerald"));
        assert!(text.contains("482 913"));
    }

    #[test]
    fn a_relay_file_overrides_the_build() {
        let dir = std::env::temp_dir().join(format!("slot-relay-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("multiplayer-relay.txt"), "# my server\n relay.example:7212 \n").unwrap();
        assert_eq!(relay_address(Some(&dir)).as_deref(), Some("relay.example:7212"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
