//! Minimal Android mirror of upstream Slot's GameMenu transition model.
//! It exists ONLY to compile the ORIGINAL link_screen.rs renderer unmodified.
//! Network and emulation lifecycle are handled by multiplayer.rs/link_play.rs.
//! Keep index/face ordering and strings in sync with crates/slot/src/app.rs
//! and crates/slot/src/link_start.rs, which supply the original Link UI.

use slot_ui::Millis;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LinkRow { Host, Join }

impl LinkRow {
    pub const ALL: [LinkRow; 2] = [LinkRow::Host, LinkRow::Join];

    pub fn index(self) -> usize { self as usize }

    pub fn text(self) -> &'static str {
        match self {
            LinkRow::Host => "Host",
            LinkRow::Join => "Join",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LinkStep { Radio, Waiting }

impl LinkStep {
    pub const ALL: [LinkStep; 2] = [LinkStep::Radio, LinkStep::Waiting];

    pub fn index(self) -> usize { self as usize }

    pub fn line(self) -> &'static str {
        match self {
            LinkStep::Radio => "Bringing the radio up",
            LinkStep::Waiting => "Looking for the other player",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LinkFail { Radio, NobodyCame, PeerVanished, Cancelled }

impl LinkFail {
    pub const SHOWN: [LinkFail; 3] = [
        LinkFail::Radio, LinkFail::NobodyCame, LinkFail::PeerVanished,
    ];

    pub fn shown(self) -> Option<usize> {
        Self::SHOWN.iter().position(|f| *f == self)
    }

    pub fn line(self) -> &'static str {
        match self {
            LinkFail::Radio => "The radio did not come up",
            LinkFail::NobodyCame => "Nobody arrived",
            LinkFail::PeerVanished => "The other player vanished",
            LinkFail::Cancelled => "Cancelled",
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GameMenu {
    Pick(LinkRow),
    Working { role: LinkRow, step: LinkStep, since: Millis },
    Linked { role: LinkRow, worked: Millis, since: Millis, opened: bool },
    Failed { role: LinkRow, fail: LinkFail, worked: Millis, since: Millis },
    Unplug { role: LinkRow, since: Millis },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_role_faces_are_ordered_host_then_join() {
        assert_eq!(LinkRow::ALL.map(LinkRow::text), ["Host", "Join"]);
        assert_eq!(LinkRow::ALL.map(LinkRow::index), [0, 1]);
    }

    #[test]
    fn original_link_step_faces_keep_exact_original_order_and_text() {
        assert_eq!(LinkStep::ALL.map(LinkStep::index), [0, 1]);
        assert_eq!(LinkStep::ALL.map(LinkStep::line),
            ["Bringing the radio up", "Looking for the other player"]);
    }

    #[test]
    fn link_fail_faces_include_only_original_shown_variants() {
        assert_eq!(LinkFail::SHOWN.map(LinkFail::line),
            ["The radio did not come up", "Nobody arrived",
             "The other player vanished"]);
        assert_eq!(LinkFail::Radio.shown(), Some(0));
        assert_eq!(LinkFail::NobodyCame.shown(), Some(1));
        assert_eq!(LinkFail::PeerVanished.shown(), Some(2));
        assert_eq!(LinkFail::Cancelled.shown(), None);
    }
}
