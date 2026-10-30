//! `Clone` for the battle and its fighters by hand, for `clone_from`: copying a battle into
//! one that already holds a battle reuses its buffers (the fighters, their names and bonus
//! lists, the log) instead of allocating them again. «Максимум»'s search copies positions
//! this way (Razdor's, the owner's request). Every field is named (no `..`), so a new field
//! does not build until it is copied here too.

use super::*;

impl Clone for Fighter {
    fn clone(&self) -> Self {
        let mut f = Fighter {
            unit: self.unit,
            name: String::new(),
            team: self.team,
            hp: 0,
            slot: self.slot,
            is_hero: false,
            squad_index: None,
            named: 0,
            level: 0,
            xp: 0,
            sign_off_level: 0,
            base: self.base.clone(),
            stats: self.stats.clone(),
            at_start: self.at_start.clone(),
            power: 0,
            mods: Buff::default(),
            items: self.items,
            spells: self.spells,
            drain: 0,
            potion: false,
            blessed: false,
            cursed: false,
            actions: 0,
            cur_initiative: 0,
            regen: 0,
            bleed: 0,
            reserve_move: false,
            suicided: false,
            surrender: 0,
            surrendered: false,
            tactical: 0,
            role: self.role,
            useful: 0,
            taken: 0,
            lost: 0,
            ai_power: 0,
            ai_role: self.ai_role,
        };
        f.clone_from(self);
        f
    }

    fn clone_from(&mut self, src: &Self) {
        let Fighter {
            unit,
            name,
            team,
            hp,
            slot,
            is_hero,
            squad_index,
            named,
            level,
            xp,
            sign_off_level,
            base,
            stats,
            at_start,
            power,
            mods,
            items,
            spells,
            drain,
            potion,
            blessed,
            cursed,
            actions,
            cur_initiative,
            regen,
            bleed,
            reserve_move,
            suicided,
            surrender,
            surrendered,
            tactical,
            role,
            useful,
            taken,
            lost,
            ai_power,
            ai_role,
        } = src;
        self.unit = *unit;
        self.name.clone_from(name);
        self.team = *team;
        self.hp = *hp;
        self.slot = *slot;
        self.is_hero = *is_hero;
        self.squad_index = *squad_index;
        self.named = *named;
        self.level = *level;
        self.xp = *xp;
        self.sign_off_level = *sign_off_level;
        self.base.clone_from(base);
        self.stats.clone_from(stats);
        self.at_start.clone_from(at_start);
        self.power = *power;
        self.mods = *mods;
        self.items = *items;
        self.spells = *spells;
        self.drain = *drain;
        self.potion = *potion;
        self.blessed = *blessed;
        self.cursed = *cursed;
        self.actions = *actions;
        self.cur_initiative = *cur_initiative;
        self.regen = *regen;
        self.bleed = *bleed;
        self.reserve_move = *reserve_move;
        self.suicided = *suicided;
        self.surrender = *surrender;
        self.surrendered = *surrendered;
        self.tactical = *tactical;
        self.role = *role;
        self.useful = *useful;
        self.taken = *taken;
        self.lost = *lost;
        self.ai_power = *ai_power;
        self.ai_role = *ai_role;
    }
}

impl Clone for Battle {
    fn clone(&self) -> Self {
        let mut b = Battle {
            content: self.content.clone(),
            formation: self.formation,
            fighters: Vec::new(),
            round: 0,
            log: Vec::new(),
            log_on: true,
            attacker: self.attacker,
            building_defence: [0; 2],
            strength_defence: [None; 2],
            bare_strengths: [false; 2],
            knight: [false; 2],
            cells: self.cells,
            mean_initiative: [1.0; 2],
            deploying: false,
            ended: None,
            threshold: 0,
            first_threshold: 0,
            cursor: (0, 0),
            current: None,
            start: [SideStart::default(); 2],
            xp_correction: 0,
            interactive: false,
            screen_object: false,
            splash: Splash::default(),
            crippled: [[false; RECORDS]; 2],
            ai_level: 0,
            surrender_mana: [0; 2],
            stale: [[0; RECORDS]; 2],
            start_count: [0; 2],
            stale_reads: Default::default(),
            predict: false,
            predicted: [0; 2],
            turn_lost: [0; 2],
            max_turn_lost: [0; 2],
            bench: Vec::new(),
            god: false,
            max_ai: false,
            max_stakes: MaxStakes::Campaign,
        };
        b.clone_from(self);
        b
    }

    fn clone_from(&mut self, src: &Self) {
        let Battle {
            content,
            formation,
            fighters,
            round,
            log,
            log_on,
            attacker,
            building_defence,
            strength_defence,
            bare_strengths,
            knight,
            cells,
            mean_initiative,
            deploying,
            ended,
            threshold,
            first_threshold,
            cursor,
            current,
            start,
            xp_correction,
            interactive,
            screen_object,
            splash,
            crippled,
            ai_level,
            surrender_mana,
            stale,
            start_count,
            stale_reads,
            predict,
            predicted,
            turn_lost,
            max_turn_lost,
            bench,
            god,
            max_ai,
            max_stakes,
        } = src;
        if !Arc::ptr_eq(&self.content, content) {
            self.content = content.clone();
        }
        self.formation = *formation;
        self.fighters.clone_from(fighters);
        self.round = *round;
        self.log.clone_from(log);
        self.log_on = *log_on;
        self.attacker = *attacker;
        self.building_defence = *building_defence;
        self.strength_defence = *strength_defence;
        self.bare_strengths = *bare_strengths;
        self.knight = *knight;
        self.cells = *cells;
        self.mean_initiative = *mean_initiative;
        self.deploying = *deploying;
        self.ended = *ended;
        self.threshold = *threshold;
        self.first_threshold = *first_threshold;
        self.cursor = *cursor;
        self.current = *current;
        self.start = *start;
        self.xp_correction = *xp_correction;
        self.interactive = *interactive;
        self.screen_object = *screen_object;
        self.splash = *splash;
        self.crippled = *crippled;
        self.ai_level = *ai_level;
        self.surrender_mana = *surrender_mana;
        self.stale = *stale;
        self.start_count = *start_count;
        self.stale_reads.borrow_mut().clone_from(&stale_reads.borrow());
        self.predict = *predict;
        self.predicted = *predicted;
        self.turn_lost = *turn_lost;
        self.max_turn_lost = *max_turn_lost;
        self.bench.clone_from(bench);
        self.god = *god;
        self.max_ai = *max_ai;
        self.max_stakes = *max_stakes;
    }
}
