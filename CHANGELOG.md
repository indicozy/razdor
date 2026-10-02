# Changelog

What changed in each version of Razdor. The newest version comes first; changes not released
yet are under "Unreleased". Each release lists the SHA-256 of its programs, which anyone
can rebuild from the same commit with `scripts/dist.sh` (see the README). From 0.1.2 the
release pipeline (`.github/workflows/release.yml`) builds and publishes a version when its
tag is pushed, and the SHA-256 are in the release's notes.

Each version names its commit. On 2026-09-30 the history was rewritten to a new author
email and every commit got a new id: the programs released before then show their commit's
old id, and their versions give both.

## Unreleased

### Changed
- **Random numbers as in the original**: one generator, the original's (the C runtime's
  `rand()`), started at 1 on every new map, so a fresh map's markets and every roll after
  them come out the same each time. It is no longer saved: loading a save starts it the way
  the original does (from the map's plants and its armies), so loading the same save twice
  replays the same rolls. Saves of the previous format still load.
- Midnight rolls building by building, each market's new goods before its barracks; a
  barracks slot always rolls, even when it is certain to grow.
- AI armies draw their four wander points as the original does: anywhere in their patrol
  box (or on the whole map), not only on cells they can walk to.
- The Community events' random flag (opcode 18) uses the Community's own generator,
  including its slip in the retry loop.
- Village offers roll as in the original: every roll is drawn until one is offered, also
  the one for the kind offered last time, and a visit with no offer lets that kind come
  back. The innkeeper and the priest compare with half the army rounded down, and the
  priest counts the living units, not only the wounded ones.
- A save load seeds the random numbers from the plants of the original's wider cell
  array (8 columns past the map's edge), as the original does.
- Battle magic as in the original: the target's protection rounds the power half to even,
  and a strike the protection takes to nothing still deals the GodAnger or GodStrike bonus.
  An undead caster's draining curse heals it by the whole amount, even when the target had
  less left.
- Vampirism heals only after melee and long strikes, never after a shot.
- The invulnerable and ghosts still take 1 from any blow or shot, but GodAnger and GodStrike
  add their 10 or 20 on top of it, as in the original. A unit with no attack of a kind
  strikes back with its attack modifier (a blessing's attack counts), and a counter blow or
  a preventive strike that kills the attacker sets off no death curse.
- A caster in the reserve can heal and bless a reserve unit that a NoHeal weapon marked.
- Any army led by a Knight-type unit takes 80% physical damage, an AI lord's as well.
- Counter blows, preventive strikes, poison and bleeding no longer count as hit points lost
  for the battle XP.
- A side left with only surrender-capable units gives up even when its last action wins the
  battle: a player whose hero has fallen and whose priests or mages kill the last enemy
  surrenders and loses, as in the original.
- The space key in battle does what a click on the unit's own card does: one action, a
  spell on itself when it can cast one, else a pass. It no longer skips the whole turn.
- A front-row shooter or mage in the first or last column never sees a clear front, as in
  the original: the shooter hits only the enemies next to it, the mage cannot cast.
- A flying shooter or mage strikes in melee at the three front cells opposite instead of
  shooting or casting there. A Ghost casts at them whatever its magic direction, as long as
  the low byte of its power is positive.
- Rows collapse only after a death or a unit's last action, as in the original, never when
  a battle or a turn starts. When a battle starts with nobody in the player's front row, his
  back row moves up (and stays there); his reserve and the enemy do not move.
- The wide row's blocked cells move forward with a collapsing row, and a blocked cell spoils
  a "clear" front; the player's own battle grid has no blocked cells, as in the original.
- A turn starts unit by unit: each unit's turn-start bonuses come before its own
  regeneration or poison (Berserk reads the hit points it had before).
- The turn-1 initiative of Artillery and FirstShot units is part of their initiative, not a
  modifier: an Elemental mage of the AI may haste such a unit on turn 1, and the panel no
  longer lists it under "this turn".
- The battle AI follows the original's scores to the letter, its slips included: the normal
  AI judges a kill by the hit points of its own unit at the target's place in the list, Life
  mages score curses with the original's defence mix-up, Elemental mages weigh a front-row
  haste by the blows needed to kill the enemies facing it, and a nearly dead Death mage with
  no spell for itself passes. The halving for a single action counts only for back-row mages,
  and a unit the AI counts as a warrior never steps back.
- The battle AI moves as the original's: units in the reserve only move (or tend the
  reserve), a front-row unit may keep its cell when that is where it scores best, and a unit
  with nothing to do casts on itself when its own cell offers a spell.
- A front-row caster of the battle AI with nothing better to do may heal or bless an ally in
  the front row, in any column, when that ally's cell scores best among its moves. A unit
  that wants to step back but finds no free cell behind it may instead step to the edge of
  the front row (or spend the action) when an enemy stands at the far edge, as in the
  original.
- The enemy is arranged anew for every battle as the original does it: its strongest front
  fighter in front, its shooters and mages behind, the rest by strength. Battles between AI
  armies arrange both sides, and the AI's practice battles play by the same rules (no
  Splash, the simple kill test).
- Unpaid units sit out only the battles their side starts: an army that attacks the player
  meets his whole army.
- After a battle the army keeps the cells its units ended on; units that did not fight, then
  the fallen, take free cells from the reserve forward.
- Those units fill each row from its first column, as the original's write-back does, not
  from the centre as a new unit does. A fallen or unpaid unit standing in the front row
  keeps the back row from stepping up when the next battle starts.
- Before every battle the original plays it once in secret, the AI on both sides, and the
  losses it predicts feed the battle XP; Razdor does the same now, so the XP pool follows
  the original's formula with the predicted loss and the worst turn's loss.
- A new unit (hired, given by an event, or at the start of a map, for the AI too) takes the
  first free cell of the reserve, then of the back row, then of the front row, whatever it
  is, as in the original.
- Community Splash as in the patch: the 80% malus counts in every battle and in the AI's
  estimates, and the 40% loses one on a multiple of 5 (an attack of 10 gives 3). Each
  neighbour gets the whole action again, in the order the units stand in their army: its
  preventive strike and counter blow (at 40%), vampirism and the rest. The primary target
  also strikes back at 40%, a splash heal reaches a NoHeal-marked neighbour, and off screen
  heals and blessings still splash.
- No preventive strike before a spell, only before blows and shots.
- Stun takes the same 30% of the target's initiative with every hit; ArmorBreaker leaves
  `x − x/4` of each defence (5 → 4, 1 → 1); FateGift saves a unit from a Neutralize blow;
  Berserk is recomputed before Drying's loss; BloodThrist counts every killing hit and not a
  target that fate saves.
- A Poison mage poisons when its own power, cut by the target's protection as the patch
  computes it (`× (99 − protection) / 100`, Elemental `/ 114`), is above 15, whatever Splash
  or Potent do to the spell.
- Assault's ×2/3 follows the patch's test of the attacker's building and initiative: from a
  building of 1 to 127, or in the open while slowed (a negative initiative modifier).
- A Suicide unit is not removed by its own blow: at 0 HP, with no actions left, it keeps its
  side in the battle until a counter blow or the next turn start removes it, and its
  vampirism can give it hit points back meanwhile.
- The magic drain and floor are per unit type: a type without magic power of its own neither
  drains nor floors, and an undead Death type with its own `MinMagicPower` gets no +25.
- NoHeal's mark stays on the place in the army's list, so a death before the marked unit
  passes the mark to the next one, as in the original.
- Flock compares the two sides as they stood after the last action of the battle on screen
  (battles between AI armies see that battle's counts too), and Hunger's count of removals is
  shared by every battle and kept from one battle to the next.
- Bleeding and Flock divide unsigned as the patch does, Evasion is read as a byte, and the
  player's twelfth unit's death stops the enemy's first unit bleeding, all as in the patch.
- An EternalGift change to a unit's initiative moves it in the turn order only from the next
  turn, and Stun keeps taking 30% of the initiative the unit started the turn with. An
  EternalGift blessing or curse on a unit cursed below 0 attack still changes that attack,
  not its shot.
- The battle AI scores a melee target with 0 Manevres with the patch's huge constant, its
  32-bit wrap included, so it fixates on such a target or ignores it as the original does.
- **The hero's route is planned as the original plans it**: a flood from the clicked cell
  that prices each step by the cell he leaves and stops as soon as it reaches him, so a
  route can be a little dearer than the cheapest (a diagonal first step, say). The walk goes
  to the very cell clicked, also inside a building.
- A click on an unexplored cell does nothing: the hero no longer feels his way into the dark.
  Water is a target only with a ship; a click next to open ground no longer means it.
- The route goes around only castles and forts whose attitude is 0 or less and ruins not
  his; every other building, ill-disposed towns included, is crossed. At sea, bridges close
  only when he clicks land or stands on one.
- Stepping onto an army engages it, before he moves: a hostile one fights, a friend meets
  him. Stepping onto a cell of a village, castle, fort, ruins or bridge meets the army that
  lives there, or the garrison of an ill-disposed castle or fort (attitude 0 included) at its
  gate; an empty one is taken, and so is every unguarded village stepped on, even when the
  route only crosses it.
- A building is entered on its second cell crossed (its events may fire) or where the walk
  ends; its window opens only there.
- AI armies attack or greet the hero only right after a step of his, never while he waits or
  casts, and never step onto his cells: they stop next to him. A friendly army greets him
  when its talk counter is above 0 (it grows as the army steps), then not for a long while.
- AI armies pay for a step with the cell they leave; stationary guards bank no time.
- A pursued army that goes out of reach ends the pursuit and the hero stops.
- An event that fires when the hero steps onto an army takes the place of the battle: the
  army then leaves him alone for a while. Going to sea or ashore makes the AI armies on that
  side lose their banked time and plan again.
- **Ships as in the original**: buying one puts no ship on the water; he steps out of the
  shipyard onto the water to sail, and leaving it on foot loses the purchase. Landing parks
  the ship on the water he left; the original's landing test, which reads a cell further
  south, is kept, so he sometimes stops on open water or steps ashore and loses the ship.
- A plant, mountain or rock standing in the water blocks ships; overlapping hills are laid
  in the original's row-by-row order.
- The hero's sight, speed and casting time stay those of the class he started with, whatever
  unit an event makes him; a Community speed event sets his speed.
- Sight and lanterns explore exactly the original's cells (its soft half-cell stamp: the
  archmage's 8 cells reach 8 more cells than before). A lantern without a radius lights
  nothing.
- The clock starts a minute after the map's start time, and the hero's first noon report is
  always the next day's, even after a morning start.
- The noon report comes in the first event check after 12:00 in which no event fired and no
  spell is being read (so after a cast, not in the middle of it); midnight comes after the
  armies have moved. A noon held up past midnight skips that day's own noon, as in the
  original.
- A friendly army's talk counter grows with each of its steps wherever the hero is, so it
  greets him again sooner. A greeting no longer stops his walk unless one of its events
  fires; an AI attack whose events fire brings no battle. Of several armies next to him,
  the last in the map's order acts.
- After a walk, AI armies keep off the cell in front of the hero (his last step's
  direction) while he stands, as in the original. A pursued army that goes into the dark
  ends the pursuit.
- An event that moves an army to the hero puts it on his cheapest free neighbour (a road
  before grass, a building only as a last resort), moves its home there too but keeps its
  patrol area where it was, and leaves a waiting army off the map.
- **The world-map AI as in the original**, its slips included. AI armies keep no goal: at
  every step they may plan again (every few steps, or every step with anyone near), with
  one flood from everything they want at once, and walk the way it gives. They score other
  armies and the hero by a battle played in secret (aggression shifting it, a hostile one
  worth more, a lost one a danger they route around), and every building by its village
  gold, what they could buy, whether they can take it and its garrison; a stationary guard
  they cannot beat closes a building to them.
- Stationary guards no longer move, plan or get paid; an army with nothing to do steps in
  place, and greets a friend or the hero standing next to it.
- AI armies greet each other (and, after his step, the hero) by talk counters, and attack a
  hostile neighbour only when their battle score says so; an enemy sheltering in a third
  party's building is not attacked.
- In a building an AI army assaults it if hostile (a tavern or church on its way too, a town
  only at its worst attitude), takes villages, castles and forts it wins, makes altars and
  ruins neutral, collects any village's gold (feudal lords), sells its pack and buys items
  by what they add to its units' strength, heals by its units' hit points left, raises its
  dead in towns and churches, hires by battle role and its leader's Nature, and buys and
  deals out its own castles' garrisons.
- AI armies keep their units' worn items, their dead (raised or dropped after a week) and
  their pay; a feudal lord short of gold at its noon leaves its cheapest units unpaid, and
  they stay out of the battles it starts. An army's noon comes at its first step after
  12:00, from its income, its castles' stock and its villages.
- Battles between AI armies: the loser's wage bill and gold go to the winner as in the
  original, the loot of items to the side with more hit points left, worn by whoever they
  help most; the AI's units are promoted by its own rolls, their items to the loot.
- Beaten armies no longer retreat into their castle: they respawn after their days, whole
  when an AI army beat them, the leader alone when the player did (unless the map says
  whole); a rogue respawning at its ruins takes them over.
- Armies placed on water are ships and plan like any army on the sea; ships with no patrol
  of their own wander the whole sea.
- Units with two upgrade options keep them in the first and third slot, as the original's
  loader moves them.
- Saves of the previous format still load; their AI armies start their plans afresh.
- An AI army buys a good of negative price (a personal item) for its absolute price.
- The AI rescores its battles against a feudal army after that army's noon, and against the
  hero after his noon, an event that took effect and a visit to a building, as the original.
- An army placed on water inside a building's footprint other than a bridge is a ship.
- An AI army standing still that the hero's cell bars counts an idle plan or a step as the
  original's path index says (an army that planned nothing in mid-path, or has no path at
  all, counts it idle).
- An army an event brings onto the map comes back with its dead raised and everyone paid,
  takes its place among the armies in their order (it moves in its turn, not last), draws
  its wander points, gets its home back when it stands in it, and the AI rescores it.
- AI hiring scans a building's six barracks slots with their empty ones, as the original:
  a unit hired from the sixth slot moves the scan on to the next role, which can lower the
  cap to 8 units early; a slot's stock is the one that goes down.
- An AI unit raised again (a leader left at 1 hit point, a resurrection) keeps its first time
  of death, as in the original: if it falls again, its corpse is dropped that much sooner.
- An AI army beaten in a fight of its own step goes on with the rest of that step as the
  original's record does: it may attack the next enemy with nobody and lose again, take a
  village's gold, buy, heal or hire, and it comes back with all that when it respawns.
- A beaten army that comes back (by its respawn or an event) is no longer "beaten" for the
  events, and an army beaten by the player and then by an AI army counts as beaten by that
  army only, as the original keeps one mark. An event can bring back a beaten army, even
  one that would never respawn, and an event that removes a beaten army stops its respawn.
- An army that respawns or that an event brings back takes its first step at once, for
  free, as in the original.
- **The noon payment as in the original.** Castles and forts pay the gold stock they have
  grown since the last noon (×F/100), not a fixed income, and towns pay nothing; every
  building with a maximum grows its stock at midnight. No building pays mana at noon, and
  villages linked to the player's buildings give him their gold only.
- Corpses draw no wage. Rear Service cuts the whole wage bill once at noon (by the player's
  stored income, also for the AI's armies), and the wages shown are the full bill. A short
  noon refunds full wages, cheapest first, a corpse's too, never an elemental's; deserters
  leave with their worn items.
- With no mana at a noon (any army's), the Community's mana-short flag goes up and stays up
  until a short-gold noon: meanwhile a unit left unpaid stays unpaid though its wage is
  paid, as in the original.
- The noon report shows the nominal income of the player's towns, castles and forts, the
  bill and the gold before the payment, warns when they do not cover the wages, and is not
  shown when there are neither wages nor income. A Ranger heals 20% more when it is shown;
  a dead Medic still heals at midnight. Saves of the previous format still load.
- Building tabs as in the original, with no attitude test: any building with a barracks
  unit hires and heals (a tavern or altar too, an ill-disposed one too), unless a barracks
  unit is not of ordinary Nature and the building lacks the "all types" flag; castles with
  no barracks no longer heal. Only towns, markets and churches sell items; the obelisk has
  no window. An ill-disposed village pays its tribute.
- The player's dead are never buried by time: they can be raised in a town or church any
  time, and come back paid. A unit whose Cost is 2 more than a multiple of 256 is raised
  for mana after a gold check, the Community's slip; an elemental's healing is checked
  against the gold and paid in mana.
- Ruins keep only their first five goods as treasure.
- The garrison as in the original: one click selects a unit, the second moves it to an
  empty cell or swaps it with a unit of the other grid, free and unpaid as it was (the only
  way into a full army). Taking back a unit that went unpaid and was parked for less than
  a day costs one day's wage; the hero and named units stay with the army; corpses can be
  left. Opening the tab sets the guards' paid marks.
- Dismiss and Bury ask for a confirmation, and the unit leaves with its worn items; the
  pack is not touched.
- **Market stock as in the original.** A market keeps 12 places, the map's goods in theirs;
  its random goods are drawn into the empty places from price bands walking down its window
  (the first from the top half), by the original's type and school rules (churches sell
  amulets and potions, towns and markets no potions, no Death items in towns and churches),
  and are no longer sorted by price. A town's potions are healing potions plus, with more
  than six goods to draw, one of five others; a town with only the map's goods gets a
  healing potion each midnight. A market redraws 12 hours after its last restock at the
  earliest, at a midnight. The map load caps the price window at the dearest item.
- The market lists only items that can be sold, shows unaffordable prices in red, opens on
  the goods when there are some, and a purchase no longer checks the pack's room. An AI
  army shops (sells its pack) in a market even when its goods are gone.
- A map's good of negative price (a personal item placed in a market) pays its buyer, as in
  the original; an attitude outside −3..3 leaves a price unchanged; a dead Merchant still
  bargains.
- A market whose list of candidates runs out while drawing a good reads on as the original
  does (the first item, or an empty place) instead of taking the last refused good.
- **World spells per unit, as in the original.** Every unit holds up to four lasting
  spells of its own; a unit with no free slot is left alone by a spell, its instant heal or
  wound included. A life-draining curse is now a lasting drain of the unit that compounds and
  cuts its hit points twice (at once and through the new maximum); a lifting lowers it.
- Enemy spells reach any army on explored ground, at any distance, friends included (but not
  a friend with a meeting waiting, nor an army in a building); the spell is lost only if its
  target leaves the map. The mana is paid when the spell lands, even below zero. An event
  that fires while the hero casts ends the casting and the spell lands at once. The spell
  card shows the original's casting time (half an hour short with a Caster or, for the
  Archmage, an odd casting time). Only the first 15 spells of the book can be cast; events
  may teach more, and the sanctuary refuses only a book of exactly 15.
- An event's spell on the army resets its time instead of adding to it, a spell number past
  the last spell casts the last one, and a spell that kills the whole army loses the game.
  The dead keep their items, and a dead unit still holding a spell is raised by a new cast
  of it.
- **Stats rebuilt as the original:** the spells' flat changes come before every percent, the
  initiative and actions are worked in hundredths (actions rounded half up), a percent on a
  protection, regeneration or vampirism adds points both ways (protections 0–99), a unit with
  no melee, ranged or magic attack at its level gains none from items, stats can go below 0,
  and a hero at 1 HP has initiative 1. A unit carries one bonus: an item's replaces its own.
  When the maximum HP changes, a wounded unit's hit points follow it proportionally.
- Item wear rules as the original: an item of a magic school only for a unit of that school
  (it no longer gives its school), holy items barred for Undead by Nature, the crown for the
  unit types the original's code lets wear it. Potions add up in one block; a potion's
  magic power never takes effect, its protections replace what an earlier potion gave, and a
  potion of 1000 healing or more raises the dead. An unknown item type reads as a potion.
- Save format 7: spells, drains and hit point fractions are kept per unit; older saves load
  with their army's spells moved onto its units.
- An event that casts a spell while the hero reads one turns his spell onto his own army, for
  free, as the original does; the event's spell lands after it. An event's spell now lands
  after its other results (a unit it adds is hit too) and before its delay passes.
- A spell that leaves someone of the army it hit alive gives a fallen hero 1 HP before the
  army is rebuilt, a curse on an enemy army included. After a battle, the player's dead
  units lose their spells.
- An event whose "no meeting" byte is 1 teaches no spell, opcode or not. A spell's price in a
  sanctuary is exactly its cost, a negative one included. Item types and spell targets are
  read case-sensitively, as the original does.
- **Scenario events as the original runs them:** conditions in its order and with its
  rules: the squad count counts the dead, strength, squads, level, gold and mana are tested
  only with "current stats", an army is inactive only while waiting (a destroyed one is
  neither), "at home" means standing in its home building, an artifact in a slot is one copy,
  an AI army's artifact must be worn, a named-character slot takes any unit of its type, and
  owner 0 fails a unit or artifact check. The "hero at 1 HP" box is a condition, never a
  result.
- Event flags are the original's one string, tested by substring: a flag `AB` also holds
  while `XAB1` is set; `+X` no longer adds a digit, counters need the `^` form. Boarding a
  ship sets the flag `Sea`, landing clears it.
- A Yes makes an answered event a once-event (unless "repeat after yes"); an asking event
  with no message applies no items, units or spells on Yes. An event's items, units and
  spells come first, then XP, gold (which no longer goes below 0), armies and the rest; a
  delay drops the event's chain, and a victory or defeat event casts, fights and waits no
  more. An event can fire again in the same minute on a later scan; one without a duration
  waits 61 minutes.
- Quests of a town or castle are taken in its main hall, with the rumours (they no longer
  fire on entering); event points fire every event they list. A quest received twice is
  listed twice in the journal, which shows the question and the time since it was received.
- A unit joining a full army dismisses the weakest one; one taken from an army brings its
  whole record and an emptied army leaves the map; a removed unit given to an army leads it
  if it is a named character. The army moved next to the hero is always the "given to"
  army. A patrol change re-centres the patrol box.
- The next campaign map gets the hero's whole record (wounds and class too), the old gold
  and mana instead of its own, and the old pack and army in place of its own. The tutorial
  is offered at every new game until it is finished (or a save exists). Double spaces in
  map texts are collapsed, `#HEROCLASS` is not filled any more, and titles end at `#`.
- Community event opcodes as the patch: opcode mode needs "no meeting" = 1 and any patrol
  value, the byte pokes and compares work on single bytes (across records, strict < and
  >), opcode 18 draws the digit of the event's `^` flag, and a "no meeting" event with a
  spell casts it.
- Save format 8: the event flags are one string; older saves load with their flags joined.
- A unit an event removes takes its worn items with it unless it goes to another army,
  where it leads when the event's slot names a character (not when the unit has a name).
  Going to sea also clears the flag `EnterShipyard`.
- A Yes sets an event's once box to its "repeat after yes" byte with the lowest bit flipped,
  as the original does. A named-squad check notes the units it takes in three places only,
  so from the fourth on a unit can count twice.
- Community opcodes reach an event's run-time bytes (last fired, times fired, the answer):
  a poke can let a once-event fire again, and an answer byte other than 0 and 1 counts as
  both "with Yes" and "with No".
- Experience as the original: a level makes a wounded unit's HP follow its new maximum
  (30 of 50 becomes 33 of 55), for the player's units and the AI's, and so does a
  promotion, which now keeps every worn item on, even one the new class could not put on.
  The XP of a won battle is paid after the potions have ended. An army whose map gives it
  an XP correction of 0 pays no XP. A side that surrenders gets no XP, and an AI army
  whose strength at the end of an AI battle is 0 is beaten even with a lone weak shooter
  or mage standing; the stronger side, not the one with more HP, takes the loot. The
  event condition on army strength counts the dead too, with the defence of the building
  the hero stands in. The XP table takes `StartExpirience` and `LevelMultipler` as they
  are (a multiplier below 100 can stop a gain a level early, as in the original), and a
  `CostMultipler` of 0 counts as 0.
- After an AI battle only the survivors gain XP and roll for promotion, as in the
  original: a fallen unit is no longer promoted, nor does it draw from the game's random
  numbers. The player's units left in a building gain XP when its garrison holds out
  against an AI army, and the AI's roll in the upgrade tree can promote them (their worn
  items then go to the battle's loot), since the original keeps them in the garrison's
  record; the worn items of those that fall go to the loot too. The XP an AI army's hires
  get from the player's army uses the units' level value without the Community "at least
  1".
- The install's ini files are read by the original's rules: keys and section names are
  exact (case-sensitive, a key is not trimmed), the last of a repeated key or section wins,
  a section header may sit anywhere in its line, a line ends only at a carriage return, and
  numbers are read the loose way (`1.5` reads 15, a minus anywhere negates). A key missing
  from `_Global.ini` reads 0, not its usual value, and `DecSpellelemental` is spelt as the
  exe asks for it. A unit is any section with a name, starting XP and cost, its upgrades
  come from the section at its position, and an upgrade to a name no unit has is no option.
  An item needs an icon, name, type and cost; a spell's id is its section's position and
  any type but Life and Death is Elemental. Names of natures, schools and bonuses are exact
  (`People` is the ordinary nature, so the AI hires such units alongside ordinary ones).
- The front row is 6 wide only when the install's wide-row option (`OptValue11`) is on,
  else 4 wide, and the options count as on when they read 1, as in the original.
- Maps load as the original loads them: an army stands exactly on its file's cell (two
  shipped armies stand where they cannot walk), and only its "inactive" byte keeps it off
  the map. Unit ids 1–3 listed among the troops of a hero preset, an army or a garrison
  are skipped. A building whose owner byte is 0 is the player's, and one of the player's
  faction is no longer his for that; a bridge flagged for the hero's class is given to him
  too. Named characters keep their double spaces.
- A map file is read leniently, as the original reads it: only the version byte of the
  header is checked, the strings start at the header's text offset, a string may end at
  the end of the data, trailing bytes and odd section sizes are ignored, an event picture's
  size is a full 32-bit value, the container's magic is `A?pf` with its other layout and
  its first scramble mode read too.
- Autosaves take the original's 12 slots: the autosave before a battle reuses the one of
  the same name, a noon autosave the one of the same name on the same map; otherwise a new
  one is made, or the oldest is overwritten once there are 12. A noon autosave on day 9
  of a month is named without the leading zero, as in the original.
- The noon autosave is written only when the noon report opens: a noon with no wages and
  no income makes none, as in the original.
- Save format 9: a game keeps the front-row width it was started with, and a load plays on
  with it whatever the option says now, as the original takes it from the save's header.
  Older saves load with the wide row they were made with.
- The map's mouse and keys as in the original: a left click or any key while the hero
  walks stops him at the end of the step he is taking (no longer on the spot), the right
  button stops nothing and no longer drags the map, and the tooltip of an army or a
  building shows only while the right button is held. While he walks or waits no other
  key or click acts, and the view stays on him. The held arrow key and the mouse at a
  screen edge (5 px) scroll by the original's scroll speed. Esc opens the game menu even
  with the minimap open.
- Esc in the main menu quits the game at once, and Back or Esc on the hero choice returns
  to the main menu, as in the original. A Yes / No question takes Esc as No and any other
  key as Yes (N included), except Tab, Alt and the Up and Down arrows.
- A battle starts as soon as its window opens, as in the original: there is no deployment
  step (the formation is the one set in the army window). Finish automatically (Q) still
  plays a battle under way out at once.
- The music as in the original, and its draws from the game's random numbers: a map
  starts with `BkgMap2`, then a new track is drawn on a timer per track among the seven
  map themes and the credits theme (never the same twice), each looping until the next.
  Battles play `BkgBattle1` against a garrison and `BkgBattle2` against an army; the
  triumph starts with a won battle's result and loops until a dialog is closed; the
  credits play their own theme. An event's window, a village's and a shipyard's open with
  one of the three chords drawn from the game's random numbers. Like the original, these
  draws shift the rolls that follow.
- A planned walk whose route was dropped, by a click on a cell out of reach or by a wait,
  keeps its cell as the planned one, and clicks on it do nothing until a click elsewhere
  plans again, as in the original.
- Opening a window from the bar while the hero walks no longer stops him on the spot: as in
  the original, he finishes the step under way when the map is back, and a wait goes on.
- F4 on the idle map waits without end, half an hour at a time, through any event's
  message, until F5 is pressed (the Community's endless wait); F5 saves as before when no
  such wait runs.
- The map scrolls by the whole milliseconds since its last frame, as in the original, so
  the first frame back from a window scrolls by the time spent in it.
- Maps whose header time is 0 (ДС1, ДС2, Другой берег) start at minute 1 of year 0, as in
  the original, not at a date of Razdor's choosing, so their events and noons keep the
  original's times. An autosave in their first hour is named "less than an hour".
- An army's own spell in the map (army byte 84, used by many shipped armies) is on every
  one of its units from the start, for good, as in the original. An army record without
  units is kept, so Проклятое озеро's army 44 can be given units and called up by its
  event. A point marked active with a radius lights up at the start whatever its model
  (РК1's point 3), and only towns, castles, forts and ruins read their garrison troops.
- A building given to the player takes the hero's attitudes as the map lists them, his
  attitude to his own side included.
- The autosave before a battle is named by the army's or the building's name only, cut at
  its first `#`, so armies named alike share an autosave as in the original; autosaves are
  written only when the install's autosave option (`OptValue8`) is on.
- A loaded game's front-row width holds for the rest of the session: the next new game,
  restart or campaign map is played with it, as in the original.
- Restart on a campaign map starts it again with what the map before carried over (army,
  gold, mana, book, pack, flags), as the original's restart snapshot, which saves keep.

### Map editor
- **Maps open and save as in the original editor.** Saving makes the original's changes:
  buildings of type 0 take their picture's type, houses with obelisk or ruin pictures become
  them, home buildings take their army as owner with its faction and attitudes, an empty
  first goods slot closes up, objects are written from the cell grid (one hill or mountain
  and one plant a cell), the save counter goes up and custom artefacts are dropped. Opening
  trims every text, clamps and moves objects and buildings as the original's loader does,
  takes building footprints from the install's pictures, and upgrades maps of versions 1–3.
- The original's four file types: Save as offers a normal map, map and text dump (`.DTD`:
  `.DTm` plus `.Eng`/`.Rus` with every text), an uncompressed map (`.DTZ`, written as
  `.DTm`) and a demo map (`.DTs`, zlib and the demo section order); Open reads all of them
  and imports a text dump.
- **The original's map check**: Check lists the 20 rules of the original editor (armies on
  impassable cells, never activated or without a meeting event, upkeep above income, patrols
  past the edge, missing descriptions and incomes, empty garrisons, markets not set up,
  unused local events, rumours and subordinate events, unclosed quests, rewards without text,
  artefacts nothing gives). They never block saving; a click opens the record.
- **Playability**: the original editor's score, with all its weights, penalties and its
  curve, stored in the map with the quest count and shown in the original's colours; each
  scoring appends its line to `MapData.Txt` in the maps folder. A map the original stops on
  (narrower than 50 cells, a point with an empty event slot or more than five events) is
  not scored.
- If drawing the map fails, the editor saves it to `ErrorSave.DTm` in its folder before the
  program ends, as the original's emergency save.
- **The army panel works as the original's army window**: a new army is the original's
  (model from the tool's four choices, XP correction 100, no faction until saved); saving
  writes the model from the behaviour and the inactive box, byte 8 as 0 and the two stored
  tactical costs (the sum and the side strength, capped at 65,000); at most 12 units with the
  leader (a count typed past it drops until the army fits, as the original's does); the original's ranges and steps; a home building brings its faction, attitudes and
  owner name, a faction its row of attitudes, a named character its class and name; the
  gold cost and upkeep use the install's recruit divisor and put the artefacts on the first
  unit only; the extra leader entry (255) is offered.
- **The building panel works as the original's building window**: its pages follow the
  type; saving takes the footprint from the picture, sets "has barracks" exactly when a
  barracks slot holds a unit, clears the event area past the list, stores no owner as 255,
  keeps only the links the type offers (a village's castle, a dungeon's entrance) and the
  garrison defence within the original's slider (a stored value above 50 comes back as 50);
  bytes 296–300 are no longer overwritten with a copy of the goods (only byte 296 is clamped
  to 0..12, as the original's hidden control does); ruins have five treasure slots; the
  incomes and prices have the original's ranges and steps; a building may list an event
  twice, up to 64; the garrison's tactical cost and strength are shown and a market test
  shows the goods the game's restock draws. At most 254 buildings, as in the original.
- **Events as in the original's event window**: a new event repeats every day and is open
  for 1,440, its title numbered; Duplicate numbers the copy by the original's `#` rule, drops
  its own picture and puts it where the event on the list's next row is, renumbering (with a
  filter, the original's quirk: the copy takes the record of the event before that place);
  Move (or Ctrl+click) reorders events with renumbering; Delete asks nothing, renumbers what
  the original renumbers (no longer the Community opcodes' relative targets) and leaves a
  deleted entry of a building's or point's list as an empty slot, dropping the count, as the
  original does; subordinate and relative-only events lock their time as in the original;
  the original's ranges and steps (open at most 99 hours; a gold "at most" up to 32,768);
  the building pickers offer no towns; a named character brings its class and a new unit
  clears the character; the hidden "generate the battle army" box is no longer shown.
- **Scenario settings as the original's scenario window**: dates are read by the original's
  rule (hour, day, month and year, unchecked, minutes dropped); a new start date moves every
  event's start with it; the hero presets edit experience, gold and mana (0..32,000, written
  as words), five spells and at most 11 starting units (a count typed past it drops until
  they fit), the start building from the
  original's types and the position only from the map; the four alliance presets are the
  original's matrices; the built-in picture cycles 0..5 and a picture file can be imported
  (kept only if it decodes); titles and character names are cut at 64 characters, the next
  map keeps its file name only. Removing a named character renumbers nothing, as in the
  original.
- **Points as the original's**: the point tool places lanterns, event points and AI target
  points (model 10); a new point is the original's zeroed record (no serial), a new lantern
  is lit with radius 10 and asks for its radius in the original's number dialog; the event
  point's panel edits the four target priorities and the active time, the target point's
  panel only those; at most 256 points (the 256th overflows its id as in the original; it
  can be selected and deleted, and the map saves with a warning).
- **Options** (toolbar): the original's text size and bold for the event window's message
  and question boxes and whether new events repeat, kept as the original's `[Option]`
  section in `DTMapEdit.Ini` in Razdor's editor folder (`RAZDOR_EDITOR_DIR`, else
  `razdor/editor` in the data folder), read from the install's editor ini until then.
- **Deleting a record renumbers what the original's delete renumbers**: a building only the
  events' building conditions (no longer army homes, links and preset start buildings), an
  army the owners and the event army fields but the patrol army, the army at home and the
  battle army. The references the original leaves past the end (those, and a deleted named
  character's) are warnings of the file check, so the map still saves; Razdor's game ignores
  them.
- **Unit and artefact editors** (toolbar: Units, Artefacts), as the original's: they edit
  the session's tables, never the map, with the original's ranges; the unit window shows
  the tactical cost and the price by the original's formula (its bands, doubled for the hero
  classes) and the level table; the artefact window copies (ids up to 255), deletes (later
  ids move down, nothing in the map is renumbered) and prices an artefact automatically by
  the original's rule (on the window's fields, an unsaved copy included); storing writes the
  price the window shows, as the original's store does. Each list exports in the original's format to `Rus_Units.New.Ini` and
  `Units.Rus`, or `Rus_Artefacts.New.Ini` and `Artefacts.Rus`, in Razdor's editor folder.
  The file check still uses the install's tables, which the game reads.
- **The main window's tools as the original editor's** (`docs/reference/editor/main-window.md`):
  five pages (terrain, hills, forests, buildings, items) with brush sizes 1–6 on the square
  that ends at the brush centre (even sizes lean down-right), Info, Delete and Move. Only the
  terrain and forest pages paint while dragging; a press with Shift, Ctrl or Alt does
  nothing. Water and lava clear plants, deep sea hills too; a hill is one object of the size
  group, only when its whole square is on the map; forests draw a random member of the
  family per cell, row by row, with the one-in-five alternate picture, and tree replacement
  mode re-rolls only trees of the same family. New buildings take the original's defaults
  (faction 3 and its attitudes, defence by picture, owner 0, house pictures turned into
  obelisks or ruins) and names drawn from the install's `DTMapEdit_Rus.Ini`, seeded by the
  spot; the place check is the anchor cell and the brush square, at most 254. Items (hero
  starts with their radius-5 reveal, the four army kinds, lanterns with the radius dialog,
  event points) open their editor and return to Info. A right press in Info mode (or a left
  one in move mode) picks up an army, a building or a point, the next press drops it
  anywhere. Delete works per page at one cell, every covering building included, and leaves
  fog revealed; the original's quirks are kept: a lantern or event point is refused on a
  figure at the unscrolled view cell, a target place is deleted as an army, the "ignore
  mountains" box inverts after its first toggle and only warns.
- The grid shows the original's seven passability tiles, the fog its reveal counters, the
  patrol zone the hovered army's square; the Info mode shows the original's hints. Burn
  everything (without the original's renamed nouns, its own text), the buildings and armies
  submenus, unit lists by group, role, school and cost (troop slots without the hero
  classes), artefacts by type and price. F2 is the quick save in the editor (not the
  language), Space returns to Info, the arrows move one cell, Save is enabled only when the
  map is modified, New keeps the current size (the old dialog is "New of size"), and the
  last map and the building check are kept in Razdor's editor `DTMapEdit.Ini`.
- An undo or redo while an army or point is held keeps it off the map (a delete or Info click
  at its old cell no longer reads it as there); Ctrl+N no longer switches the music too; the
  editor's status message stays clear of the counts and shows in full on hover.

- **The original's new-map generator** (toolbar: Generate, `docs/reference/editor/newmap.md`),
  in place of "New of size": sizes 50 to 800, the eight map types and their orientation,
  the seven share bars as the original's panel widths (with its splitters' snapping),
  blur, the seed with "keep" and "rebuild relief". It makes the same map as DTMapEdit for
  the same seed, options and installed pictures: the original's draws, roundings (in the
  x87's extended precision where it uses it, with its cosine and sine as an x86 processor
  gives them), fractal, lines, stamps, rivers, coast-band sweeps, swamp, mountains, hills
  and forest, and its quirks (the reseed after the relief, the relief reuse that cuts the
  last forest field without the map type's water and rivers, the small forest pool that
  stops after one cluster, old trees kept on reuse). Where the original would loop for ever, the run
  stops and says so. The bar widths are read from and written to `[MakeMap]` of Razdor's
  editor `DTMapEdit.Ini`; Exit takes the map with the original's new-map header, relation
  matrix, title and file name, and the seed in the header. An 800 × 800 map takes about a
  second.

- **The original's world generator** (toolbar: World, `docs/reference/editor/worldgen.md`):
  its three steps, each run from its tab on the open map as one undo step. *Buildings and
  roads* clears the buildings and the road layer, places towns, castles and villages sector
  by sector with the original's square search, links them by roads over the game's path
  planner with bridges over water, puts forts, taverns, markets and churches on road
  junctions and ruins in the wild; *Economy* sets incomes, goods, spells, barracks and
  factions from the income, trade and library grids; *Armies and garrisons* gives every
  building an army or a garrison from the original's random army builder (themes from
  `[AIArmyGeneration]`, the budget ranges, the minimum point, the two boxes), its units
  priced by the editor's own strength formula in 80-bit precision. The same map, options and
  generator state give the same world as DTMapEdit, quirks included (the reseed at every
  placement, the spiral that only grows toward the bottom right, the extra sector visit, the
  picture drawn before the roll, the six-bridge limit, the village gold from the mana base,
  the town budget from the castle's low end, garrisons that pile up, and others). Where the
  original stops a step with a range error or loops for ever, the step stops and says why
  (also for an old building past the map's edge, a first town refused by a wide brush and
  army gold past 32767). Buildings and roads build the marks again before and after, as the
  original does. An 800 × 800 map's buildings and roads take about half a second.
- A building whose footprint reaches past the map's left or top edge leaves all of its cells
  as they were (plants and marks), as the original's placement stops at the first of them.

## 0.2.2 — 2026-10-01

Commit `b01b492` (tag `v0.2.2`).

### New
- **Discord Times mods**: an install with a mod in it (its own `Rus_*.ini`, art and maps)
  plays with the mod's units, upgrade tree, items and spells, e.g. the Evolution mod (161
  units). The ini files are read as leniently as the original reads them: a value that
  cannot be read takes its default, an entry without a usable `GlobalIndex` (or an item
  without a `Type`) is skipped, and each case is written to `razdor.log`, as is an upgrade
  naming a unit that does not exist. Only a missing or unreadable file refuses the install.

### Fixed
- One value Razdor could not read made it drop the whole install and fall back to the demo
  with placeholder art. The Evolution mod's spell «Сангвинаре Вампирис» ends an effect line
  with an empty start time (`Effect2=…,1500,`), which now reads as 0.

SHA-256 of the released programs (built by the release pipeline):
```
994ecb4309cbfd932280b37fe4a3fd6bfe89cd0c84b605f207d11d434d0fc1d8  razdor
a6f42ab99ad4968b6af2181db53ea022a59260f2ce4b4d0147c8d0afad5ac4d7  Razdor.exe
0770c065199ea27449911994f331acca4452c679b6787a12b3c1e5757791a7cb  razdor-macos
```

## 0.2.1 — 2026-10-01

Commit `68cc569` (tag `v0.2.1`).

### New
- **A macOS program**, `razdor-macos`: one universal file for Apple Silicon and Intel Macs
  (macOS 11 and later), built by the release pipeline on a Mac (`scripts/dist-macos.sh`) and
  published with the Linux and Windows programs. It is not signed by a developer: lift the
  quarantine once (`xattr -d com.apple.quarantine razdor-macos`) after downloading it.
- The release pipeline can be run by hand, to build a commit's programs without publishing.

SHA-256 of the released programs (built by the release pipeline):
```
ce4e76a42cf85a37b16c638782013f0994fb686ae323c26c0a1a5d3d91883768  razdor
c63e6bd7544c836df4afc84ecdffb456fcf8c9ce77ee9194ebdc353b420198c6  Razdor.exe
ca3ce480dc84634988249aacad2c6ad73b8d10996c139230d1dba63753f739f3  razdor-macos
```

## 0.2.0 — 2026-09-30

Commit `610a898` (tag `v0.2.0`); its programs show `85b063f`, the id before the history
was rewritten.

### New
- **Following an army** as in the original («Автоматически преследовать выбранную армию»):
  after a click on an army (a second click, with the route preview) the hero keeps going to
  where it is now, until they meet; a right click or Space stops him.
- **Rearranging the army by dragging**: on the army screen and in the barracks a unit's card
  dragged onto another cell goes there, swapping with a unit standing in it.
- **The play log** (`razdor-play.log`, next to `razdor.log`): the session's games, screens,
  messages, walks, world and scenario events, and every battle in full with both armies'
  units, stats and items, for reading back when something plays wrong.
- **Route preview** (a Razdor extra): a click on the map shows the route and its travel
  time; clicking the same spot again, or a double click, walks it. A right click, Space or
  a move of the hero drops the preview.
- **A click while the hero walks stops him**, as in the original (a right click and Space
  still do too).
- **Armies on the minimap:** every army on explored ground shows as a mark in the original's
  colours (`ColorMarkEnemy` for hostile ones, `ColorMarkAlly` for the others), under the
  hero's blinking mark.
- **Drag the map with the right button** (a hand cursor while it moves); a right click that
  does not move still stops the walk.
- **Edge scrolling** as in the original: the mouse at an edge or a corner of the window pans
  the map; a click on the map or Tab brings the view back to the hero.
- **After a quest shows places on the map** the camera flies back to the hero by itself
  (a click or Tab still skips straight back).
- **The defeat screen** offers «Загрузить последнее сохранение» (the newest save, manual or
  automatic, of the same map) and «Начать карту заново» (the same map, the same hero),
  besides a new game.
- **A shown place opens like an iris:** a circle grows from its centre to its edges, with a
  soft rim, instead of the whole area fading in at once.

### Changed
- **A meeting's words come before the fight:** when an army comes at the hero with an
  event's message («Встреча с Блэки»), the message is read over the map and the battle opens
  after «ОК», instead of the battle opening under the message.
- **No ship hints** in the map's top left corner («Корабль ждёт…», «В море…»): the original
  has none.
- **Map zoom** follows the screen size: at zoom 1 the map shows as much ground as the
  original at its 1024×768 (about 32 cells across), with larger cells and figures on larger
  screens, instead of fixed 32 px cells that made everything small on big screens. The mouse
  wheel and +/- still zoom from there.
- **AI armies keep to the same roads as the hero:** their routes go around castles and
  forts that aren't their own or a friend's, ruins that aren't theirs, and any other building
  hostile to them. The building an army heads for (to take it, heal, hire…) and the one it
  stands in stay open. Before, armies walked straight through any building.

### Fixed
- **The invulnerable take 1 hit from any blow or shot:** units with «Неуязвимость» and
  ghosts («Яростный Дух») lose exactly 1 hit however hard they are hit, piercing blow
  («Проникающий Удар») or not. «Кара Господня» and «Гнев Господен» added their 10 or 20 on
  top of that 1.
- **Spell pictures in the book and the sanctuary** take each layer's `ColorC` away, as the
  ini says ("colour correction (-RGB)"): «Исцеление» is green, the lightnings purple and
  cyan. They were multiplied by it, which tinted every picture towards that colour.
- **A percent bonus to a protection adds its points:** 44% magic protection with +20% from a
  spell or a potion is 64%, not 55% (the rest of the way to 100 closed by a fifth), up to
  100%. The same for regeneration and vampirism.
- **Casting on the map takes its time in front of you:** the hero reads the spell while the
  clock runs, as a rest does (the time panel shows «Чтение: 2 час» counting down), the armies
  move meanwhile, and the spell lands at the end. Before, the whole reading passed in one
  frame and the clock only jumped. An enemy reaching the hero loses the spell; a message
  pauses the reading; walking, resting or Space drops it. Mana is only spent when the spell
  lands.
- **Battle cards show the actions left:** "Mnvr" on the card and in the panel counts down as
  the unit acts and shows extra actions (haste, a first-turn bonus) in blue and lost ones in
  red, refilled every turn. It used to show the unchanging stat.
- **An item that raises maximum HP brings the hit points with it:** a unit at 70/70 given
  +10 HP is 80/80, not 70/80 (from the pack, handed from another unit, or given by a quest).
  Hit points already lost stay lost: 60/70 becomes 70/80.
- **Objects at the edge of the dark** (trees, hills, bridges, buildings) are drawn and fade
  into it with the fog's soft edge, instead of vanishing while part of the edge still showed
  ground.
- **Messengers come to the hero:** a friendly army that hunts only the player (such as
  «Посыльный» on «Тихая пристань») stood still in its castle, because the AI let armies go
  for the player only to attack him. It now comes to meet him once he is within its view
  range, and the meeting's event runs.
- **A crash on the map** ("byte index 1 is not a char boundary … `Деревня`"): a building
  drawn without its picture showed the first letter of its type, cut as a byte, which broke
  on Russian names.
- **The sell shop** shows items the market does not buy (personal and quest items such as
  «Проклятые кости», a price of 1 or less) as «не продаётся», and «Продать» stays off for
  them; before they looked sellable and the button only said no.
- **Enemies' items in battle:** the unit panel showed worn items for the player's units only;
  an enemy's now show too (a Тень wearing «Проклятые кости» looked as if it wore nothing).
- **Past the map's right edge** a strip of half a cell showed terrain without fog; the view
  now ends at the map's edge, and anything beyond the map is black.
- **A shown place** no longer shows a faint ring before it fades in: the fog over it is kept
  exactly as it was, soft edges included, until the reveal.
- **Builds:** a local build could differ from the release pipeline's when the Rust source
  component was installed (its real paths went into the programs); `scripts/dist.sh` maps
  them back, and a local build of v0.1.2 gives the released `Razdor.exe` bit for bit.

SHA-256 of the released programs (built by the release pipeline):
```
066579d52a7d5862d766c64d50deb97d13547b2cb52fe25dc1d0457a17dcf989  razdor
2f77bab02e97413fce47bbf39ebb328d95a4c613e5c83541239f6e94ca44bd84  Razdor.exe
```

## 0.1.2 — 2026-09-30

Commit `bb09967` (tag `v0.1.2`); its programs show `378c873`, the id before the history
was rewritten.

### New
- **Battle AI, easy or expert:** the settings window has the original's «Улучшенный
  интеллект противника в битве». Expert lets the enemy count a unit as killable when the
  actions it has left can finish it, not only with one hit. Until changed it follows the
  install's own setting (`OptValue9`).

- **Quests show places on the map:** when an event lights a lantern or shows an army, the
  camera flies there once its message is read and the uncovered area fades in from the fog,
  one place after another. A click on the map or Tab skips it.
- **Unit cards in battle** show every gain or loss against the start of the battle (blue
  raised, red lowered), also the lasting ones, and the building's defence in the D values;
  the unit panel writes it apart, as the original does («15 + 12»).

### Changed
- **Ships** on the map are the original's: the hero's galley (at sea and waiting at the
  shore), pirate ships and merchant cogs, rowing and turning as they move. The drawn
  placeholder is left only for playing without an install.
- **The noon report** no longer opens when nothing came in or went out that day (no
  income, no wages, nobody unpaid or gone).
- **Item restrictions** follow the original: shields need a melee attack (warriors only),
  artillery cannot use bows, the undead cannot wear holy items («Святое писание», icons…),
  and «Королевская корона» is for the hero and a few noble units.
- **Building defence** also counts in a friendly building for the hero's side, and for an
  enemy army attacked in a building of its own side (before: the hero's own buildings and
  castle garrisons only).
- **Battle:** a side with nobody in the front row steps forward at once, also at the start
  of the battle (as the player sees in the original), not only after a death.
- **Map:** buildings are drawn in front of hills, rocks and trees, which no longer hide them.
- **Market:** after a buy or a sale the selection moves to the next item (or the one
  above), for many trades in a row.
- **The autosave before a battle** is the moment just before it: loading it puts the hero
  on the map next to the enemy, not straight into the fight.

### Fixed
- **Item and spell bonuses on protections, regeneration and vampirism** (`p-` values) did
  nothing for a unit starting at 0%: «Святое писание», «Меч "Кровопийца"», «Латы
  крестоносца», «Шлем Героя» and others now give their percent.
- **Esc in battle** opened the ways out and closed them in the same moment, and did nothing
  while an animation played.
- **Music after loading a game:** the triumph of a battle won before no longer carries on;
  the map music starts again.

## 0.1.1 — 2026-09-30

Commit `8d42bff` (tag `v0.1.1`); its programs show `48af7af`, the id before the history
was rewritten.

### New
- **New game:** the scenario list groups the maps as the original does. A campaign is one
  row under its name («Раменское королевство», «Сказка странствий»), with its chapters
  listed under it in play order; single scenarios are rows of their own. The list scrolls
  with the mouse wheel when it is longer than the window.
- **New game:** each map shows its own picture in the map frame, as in the original; the
  terrain preview moves to a small square next to the name, status and size.
- **The tutorial offer:** the first «Новая игра» opens «Обучающий сценарий», the original's
  window with its picture and text. «Да» starts the tutorial map and the hero choice, «Нет»
  opens the scenario list. It comes once (remembered in `settings.json`), and not at all when
  the install says the tutorial is done.
- **No OpenGL driver:** when Windows offers only its OpenGL 1.1 fallback (a Remote Desktop
  session, or no graphics driver, as in many virtual machines), Razdor explains what to do,
  in Russian and English, instead of showing the bare "WGL_ARB_pixel_format is required".

### Changed
- **Battle:** the enemy under the mouse is framed green, as in the original, not red.
- **World map:** the hero cannot walk through any army, friendly or hostile; he goes
  around it, or stops if there is no way. Before, only armies standing guard blocked him.

### Fixed
- **Menu:** the «Рестарт» question closed at once, because the click that opened it also
  answered «Нет». A question now takes clicks only from the frame after it opens.
- **Load window:** the same flaw in the «Удаление сохранения» question could delete a save
  with one click, without showing the question.
- **Builds:** the programs' SHA-256 depended on the folder they were built in (the order of
  the path remappings in `scripts/dist.sh`). The same commit and tools now give the same
  files anywhere.

SHA-256:
```
6f3291970860c208770d0af4d09e89d2b93d595fb84e22472fa80e4a30815540  razdor
4f39e09b7d4cd042cddc27314fd715be458d6c1a9104267b2f3cd44fb2cce3b0  Razdor.exe
```

## 0.1.0 — 2026-09-29

Commit `bf01d7a` (tag `v0.1.0`); its programs show `8593815`, the id before the history
was rewritten.

The first release: `Razdor.exe` (Windows x86_64) and `razdor` (Linux x86_64).

- Plays the original's scenarios from the player's own copy of *Discord Times*
  (Community Update 1.2): world map, buildings, armies, battles, economy, events and quests,
  spells, saves, sounds and music, with the original's art and texts read at runtime.
- The map editor, the built-in demo without an install, English and Russian interface.
- Army screen: items are dragged from the backpack onto a unit's card to give them to it,
  and between units.
- Settings: an optional FPS counter in the top right corner.
- Fixed a crash when the window is minimized or very small.
- The author's credit in the programs, the MIT license and the disclaimer.

SHA-256:
```
2863148726d8105bc7e14c6fdc377ec18f1db4c7546b2c4a9f5c313e3b0e9abe  razdor
b808bcb2d487f7cc8ea9c4d6618a9b86f91d1781f3dbcc9fc2bc57146fd1fdfe  Razdor.exe
```
