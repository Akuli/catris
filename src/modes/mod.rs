use crate::lobby::MAX_CLIENTS_PER_LOBBY;

mod traditional;
pub use traditional::TraditionalMode;

#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub enum Mode {
    Traditional,
    Bottle,
    Ring,
}

impl Mode {
    pub const ALL_MODES: &'static [Mode] = &[Mode::Traditional, Mode::Bottle, Mode::Ring];

    pub fn name(self) -> &'static str {
        match self {
            Mode::Traditional => "Traditional game",
            Mode::Bottle => "Bottle game",
            Mode::Ring => "Ring game",
        }
    }

    pub fn max_players(self) -> usize {
        match self {
            Mode::Traditional | Mode::Bottle => MAX_CLIENTS_PER_LOBBY,
            Mode::Ring => 4,
        }
    }
}
