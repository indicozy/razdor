//! The idle animation of the armies standing on the map (engine.md §7, 0x4c868c).
//!
//! While the map is idle (the hero neither walks nor waits: the world frame's flag 0x68dc62),
//! the world frame (0x4cc17b) sets the sprite of every army on the map that stands in no
//! building (+0x20ec below 1) from the time since its idle stamp: an army on land takes the
//! frame of its figure's 20-step table at 0x4ed6d0, one step per 250 ms; a ship (and the hero
//! aboard his) cycles its 8 frames at 200 ms. The stamp and a `Random(3000)` ms offset are set
//! for every army at the map load (0x4b4b38, 0x4b4b43) and at a save load (0x4b8691), and again
//! for the armies the hero's stop restarts ([`crate::rules::game::Game::armies_snap`],
//! 0x4ad8a0). The facing row is the direction of its next step (+0x74, 5 when it has none).
//!
//! The figure's pixels are drawn with SRCBLEND ONE, DESTBLEND INVSRCALPHA (0x4c9ef6): the
//! sheets hold premultiplied colour, and the Ghost's light where its alpha is 0 is added to
//! the map (its glow).

/// One step of a land army's idle table (0x4c868c: `div 0xfa`).
pub const IDLE_STEP_MS: i64 = 250;
/// One frame of a ship's idle cycle (0x4c868c: `div 200`).
pub const SHIP_STEP_MS: i64 = 200;
/// Steps in a figure's idle table.
pub const IDLE_STEPS: usize = 20;

/// The idle tables at 0x4ed6d0: 20 sheet frames per figure (`ui::world_view::FIGURES` order),
/// 0x50 bytes each. The heroes' figures and the hero's galley (0–3) never take theirs (the
/// pass skips record 0 on land) and stand still; the ships' figures (11, 12) are read by
/// the ship branch instead.
pub const IDLE_FRAMES: [[u8; IDLE_STEPS]; 11] = [
    [0; IDLE_STEPS],
    [0; IDLE_STEPS],
    [0; IDLE_STEPS],
    [0; IDLE_STEPS],
    // Rogue
    [0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 1, 1, 2, 2, 1, 1, 0, 0],
    // Peasant
    [1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2],
    // Knight
    [0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 0, 0, 0, 0, 1, 1, 1, 1],
    // Necromant
    [0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 1, 1, 2, 2, 1, 1, 0, 0],
    // Zombie
    [1, 1, 1, 1, 2, 2, 4, 4, 1, 1, 2, 2, 2, 2, 4, 4, 2, 2, 1, 1],
    // Ghost: it floats through its frames 0–6 and back to 0, a 5-second loop.
    [0, 0, 1, 1, 2, 2, 3, 4, 5, 5, 6, 0, 1, 1, 2, 2, 3, 4, 5, 5],
    // Mage
    [0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 1, 1, 7, 7, 1, 1, 0, 0],
];

/// The sheet frame of a land army of `figure` standing idle, `since_ms` after its idle stamp
/// with its `offset_ms` (0x4c868c): step `(since − offset) div 250`, held at 0 while
/// negative (the first `offset` ms show the table's first frame), modulo 20.
///
/// A figure past the table (the ships' 11 and 12 on land, or an event's opcode 17 setting a
/// larger byte) reads past it in the original, into pointers; Razdor shows frame 0 *(guess)*.
pub fn idle_frame(figure: u8, since_ms: i64, offset_ms: i32) -> u32 {
    let step = ((since_ms - offset_ms as i64) / IDLE_STEP_MS).max(0) as usize % IDLE_STEPS;
    IDLE_FRAMES.get(figure as usize).map_or(0, |t| t[step] as u32)
}

/// The sheet frame of a ship standing idle (0x4c868c): `(since + offset) div 200`, held at 0
/// while negative, `and 7`. The offset is *added* here, unlike on land (the original's).
pub fn ship_idle_frame(since_ms: i64, offset_ms: i32) -> u32 {
    (((since_ms + offset_ms as i64) / SHIP_STEP_MS).max(0) & 7) as u32
}

/// The hero's galley at rest (0x4c868c, its last part): `t div 200 and 7` of the frame clock
/// itself, no stamp, while he is aboard and in no building.
pub fn hero_ship_idle_frame(clock_ms: i64) -> u32 {
    ship_idle_frame(clock_ms, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ghost_floats_through_its_frames_every_250_ms() {
        let ghost = 9;
        let frames: Vec<u32> = (0..20).map(|k| idle_frame(ghost, k * 250, 0)).collect();
        assert_eq!(frames, [0, 0, 1, 1, 2, 2, 3, 4, 5, 5, 6, 0, 1, 1, 2, 2, 3, 4, 5, 5]);
        // A step lasts 250 ms and the table loops after 20 of them (5 s).
        assert_eq!(idle_frame(ghost, 249, 0), 0);
        assert_eq!(idle_frame(ghost, 6 * 250 + 249, 0), 3);
        assert_eq!(idle_frame(ghost, 7 * 250, 0), 4);
        assert_eq!(idle_frame(ghost, 5000 + 7 * 250, 0), 4);
    }

    #[test]
    fn the_offset_holds_the_first_frame_then_shifts_the_loop() {
        let zombie = 8;
        // Before the offset has passed the step is held at 0 (the original's clamp).
        assert_eq!(idle_frame(zombie, 0, 2999), 1);
        assert_eq!(idle_frame(zombie, 2000, 2999), 1);
        // Then it runs from the table's start.
        assert_eq!(idle_frame(zombie, 2999 + 4 * 250, 2999), 2);
        assert_eq!(idle_frame(zombie, 2999 + 6 * 250, 2999), 4);
    }

    #[test]
    fn every_figure_takes_its_own_table() {
        let at = |figure, step: i64| idle_frame(figure, step * 250, 0);
        assert_eq!((0..4).map(|f| (0..20).map(|s| at(f, s)).max().unwrap()).max(), Some(0), "the heroes stand still");
        assert_eq!(at(4, 14), 2, "Rogue");
        assert_eq!(at(5, 0), 1, "Peasant");
        assert_eq!(at(6, 8), 2, "Knight");
        assert_eq!(at(7, 14), 2, "Necromant");
        assert_eq!(at(10, 14), 7, "Mage");
        assert_eq!(at(11, 14), 0, "past the table");
        assert_eq!(at(200, 3), 0);
    }

    #[test]
    fn ships_cycle_eight_frames_every_200_ms_with_the_offset_added() {
        let frames: Vec<u32> = (0..10).map(|k| ship_idle_frame(k * 200, 0)).collect();
        assert_eq!(frames, [0, 1, 2, 3, 4, 5, 6, 7, 0, 1]);
        assert_eq!(ship_idle_frame(0, 450), 2);
        assert_eq!(ship_idle_frame(-500, 0), 0);
        assert_eq!(hero_ship_idle_frame(1600 + 399), 1);
        assert_eq!(hero_ship_idle_frame(1600 + 400), 2);
    }
}
