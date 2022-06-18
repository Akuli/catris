// This module contains pure game logic. IO and async are done elsewhere.
pub mod blocks;
pub mod game;
pub mod player;

// PlayerPoint numbers must be big, they don't wrap around in ring mode
pub type PlayerPoint = (i32, i32); // player-specific in ring mode, (0,1) = downwards
pub type WorldPoint = (i16, i16); // the same for all players, differs from PlayerPoint only in ring mode
pub type BlockRelativeCoords = (i8, i8); // (0,0) = center of falling block
