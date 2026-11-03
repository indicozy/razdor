//! Chapters 2 «Марка» (the hub), 3а `abbey`, 3б `salt` and 3в `camp` of «Пепельная Марка», played
//! by the harness of `playthrough`. Each map is entered by the campaign's own hand-over
//! (`rules::campaign::enter`) from the map before it, so the squad and the variables are the ones
//! the campaign carries. The skipped tests need `RAZDOR_DT_DIR`.

use std::sync::Arc;

use super::open::Opened;
use super::playthrough::{install, meet_host, open_map, prepared_to_wraiths, Run, Stuck, PREPARED};
use crate::dt::install::DtInstall;
use crate::rules::campaign::{after_victory, enter};
use crate::rules::content::Content;

/// Chapter 1 won on the prepared route, and the game handed over to the map the campaign sends it
/// to (`marka`). The run is the hub's first visit.
fn into_marka(install: &DtInstall, seed: u32, choices: &[(&str, &str)]) -> Result<Run, Stuck> {
    let (brod, brod_content) = open_map(install, "brod");
    let mut run = Run::new(&brod, brod_content, seed, PREPARED)?;
    prepared_to_wraiths(&mut run)?;
    meet_host(&mut run, 30)?;
    if !run.victory() {
        return Err(Stuck::Broken("chapter one was not won".into()));
    }
    let to = after_victory(&mut run.g, &brod.routes).ok_or_else(|| Stuck::Broken("chapter one has no next map".into()))?;
    hand_over(install, &brod, &mut run, &to, choices)
}

/// Drinks a potion of the pack for each wounded living unit, until none is wounded or the pack has
/// no potion left (a player drinks from the army screen).
pub(crate) fn drink_for_wounded(run: &mut Run) {
    loop {
        let wounded = (0..run.g.squad.len()).find(|&i| run.g.squad[i].alive() && run.g.squad[i].hp < run.g.squad[i].max_hp(&run.g.content));
        let Some(unit) = wounded else { return };
        let drunk = (0..24).find_map(|k| run.g.drink(unit, k).ok().map(|healed| (k, healed)));
        let Some((_, healed)) = drunk else { return };
        run.log.push(format!("drank a potion: {} +{healed} HP", run.squad_names()[unit]));
    }
}

/// The game of `to`, entered from `from` (the run's game) by the campaign's hand-over.
fn hand_over(install: &DtInstall, from: &Opened, run: &mut Run, to: &str, choices: &[(&str, &str)]) -> Result<Run, Stuck> {
    let (opened, content): (Opened, Arc<Content>) = open_map(install, to);
    let g = enter(&run.g, from, &opened, None, content).map_err(Stuck::Broken)?;
    Run::resume(g, choices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::triggers::VarValue;

    /// The hub's quests on the way to the council: the marsh folk's offer taken and the wraiths beaten
    /// (Tess's quest), then the shrine cleansed (the Order's quest). Each walk is the game's own route.
    pub(crate) fn hub_quests(run: &mut Run) -> Result<(), Stuck> {
        drink_for_wounded(run);
        run.walk_to((15, 69), 10)?;
        run.walk_to((30, 84), 12)?;
        run.walk_to((28, 36), 10)?;
        Ok(())
    }

    /// The hub's first visit with its quests done, the council answered with `ally` (1 the Order, 2 the
    /// marsh folk, 3 the Companies). Returns the run once the hub has handed over (the chapter is done).
    pub(crate) fn hub_to_council(install: &DtInstall, seed: u32, ally: &str) -> Result<Run, Stuck> {
        let choices = [("council", ally), ("shrine_inside", "cleanse"), ("shrine_offer", "accept"), ("marsh_offer", "accept"), ("marka_arrival", "take")];
        let mut run = into_marka(install, seed, &choices)?;
        hub_quests(&mut run)?;
        run.wait_until_done(40)?;
        Ok(run)
    }

    /// The branch `to` (the hub's ally choice, then the campaign's route), entered by the hand-over from
    /// the hub, with its healing drunk. `choices` are the branch's own dialog answers.
    pub(crate) fn into_branch(install: &DtInstall, seed: u32, ally: &str, to: &str, choices: &[(&str, &str)]) -> Result<Run, Stuck> {
        let mut hub = hub_to_council(install, seed, ally)?;
        let (marka, _) = open_map(install, "marka");
        let next = after_victory(&mut hub.g, &marka.routes).ok_or_else(|| Stuck::Broken("the hub has no route".into()))?;
        if next != to {
            return Err(Stuck::Broken(format!("the hub sends the hero to {next}, not {to}")));
        }
        let mut run = hand_over(install, &marka, &mut hub, to, choices)?;
        drink_for_wounded(&mut run);
        Ok(run)
    }

    /// The hub's council over `seeds` seeds for `ally` (the choice: `order`, `marsh`, `companies`): how many seeds
/// reached the chosen ally's branch `expect_to`.
    fn council_rate(install: &DtInstall, ally: &str, expect_to: &str, seeds: u32) -> (u32, Vec<String>) {
        let mut won = 0;
        let mut problems = Vec::new();
        for seed in 1..=seeds {
            match hub_to_council(install, seed, ally) {
                Ok(mut hub) => {
                    let num = match ally { "order" => 1, "marsh" => 2, _ => 3 };
                    let ally_value = hub.var("ally") == Some(VarValue::Int(num));
                    let (marka, _) = open_map(install, "marka");
                    let next = after_victory(&mut hub.g, &marka.routes);
                    if hub.victory() && ally_value && next.as_deref() == Some(expect_to) {
                        won += 1;
                    } else {
                        problems.push(format!("seed {seed}: victory {} ally {ally_value} next {next:?}", hub.victory()));
                    }
                }
                Err(e) => problems.push(format!("seed {seed}: {e}")),
            }
        }
        (won, problems)
    }

    /// The hub opens with the Crown's surgeon's draught, and the knight drinks it (he rode in from the ford
    /// with the chapter 1 wounds; the start heals him, the draught is tested from 1 HP).
    #[test]
    fn marka_opens_with_the_surgeons_draught() {
        let Some(install) = install() else { return };
        let mut run = into_marka(&install, 1, &[]).unwrap_or_else(|e| panic!("{e}"));
        assert!(run.log.iter().any(|l| l.contains("dialog marka_arrival")), "{:?}", run.log);
        // The hub's start restores the squad (triggers/marka.toml m_restore), so the knight is whole when he drinks:
        // the draught's own heal is tested on a wounded knight.
        assert_eq!(run.g.squad[0].hp, run.g.squad[0].max_hp(&run.g.content), "the start restores the knight");
        run.g.squad[0].hp = 1;
        let before = run.g.squad[0].hp;
        drink_for_wounded(&mut run);
        assert!(run.g.squad[0].hp > before, "the draught heals the knight: {before} -> {}", run.g.squad[0].hp);
    }

    /// The hub's quests and the council's choice of the Order: the campaign routes the hero to the abbey.
    /// Over 15 seeds (the route is the same; the rate is the route's, the target 70%).
    #[test]
    fn the_hub_council_takes_the_order_and_routes_to_the_abbey() {
        let Some(install) = install() else { return };
        let started = std::time::Instant::now();
        let (won, problems) = council_rate(&install, "order", "abbey", 15);
        eprintln!("hub council (the Order) over 15 seeds: {won}/15 in {:.2}s {problems:?}", started.elapsed().as_secs_f64());
        assert!(won * 100 >= 70 * 15, "won {won}/15: {problems:?}");
    }

    /// The same hub with the marsh folk's council: the campaign routes the hero to the salt marsh.
    #[test]
    fn the_hub_council_takes_the_marsh_folk_and_routes_to_the_salt() {
        let Some(install) = install() else { return };
        let started = std::time::Instant::now();
        let (won, problems) = council_rate(&install, "marsh", "salt", 15);
        eprintln!("hub council (the marsh folk) over 15 seeds: {won}/15 in {:.2}s {problems:?}", started.elapsed().as_secs_f64());
        assert!(won * 100 >= 70 * 15, "won {won}/15: {problems:?}");
    }

    /// The branches open through the campaign's hand-over, with their arrival moments.
    #[test]
    fn the_branches_open_with_their_arrival_dialogs() {
        let Some(install) = install() else { return };
        let abbey = into_branch(&install, 1, "order", "abbey", &[]).unwrap_or_else(|e| panic!("{e}"));
        assert!(abbey.log.iter().any(|l| l.contains("dialog abbey_arrival")), "{:?}", abbey.log);
        let salt = into_branch(&install, 1, "marsh", "salt", &[]).unwrap_or_else(|e| panic!("{e}"));
        assert!(salt.log.iter().any(|l| l.contains("dialog salt_arrival")), "{:?}", salt.log);
    }
}
