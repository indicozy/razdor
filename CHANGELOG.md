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

### Added
- **A close button on the battle window,** as in the original: the red cross in its title bar
  opens the leave-battle window, as Esc does.

### Fixed
- **Hills lie under everything, as in the original:** the green and rocky hills are drawn
  before the trees, mountains, buildings and armies, and the route over them, so nothing
  standing above a hill is hidden by it.
- **A shipyard opens the original's ship window, not the building window:** on land it
  shows the shipyard's picture, the harbour master's words with the owner's name, the price
  of a ship and "Нанять корабль" and "Отмена" (with a warning when your gold is short); at
  sea it opens nothing. Hiring the ship closes the window, so you can click the water next to
  the shipyard to sail. The shipyard's main hall and "Корабли" tab are gone, as the original
  has none.
- **Army cards are arranged by clicks, as in the original:** with a unit selected, pressing
  another unit swaps the two at once, and pressing an empty cell slides the card there; both
  play the card sound and end with none selected. The hero is a unit like any other here:
  selected, a press on another unit swaps it with the hero (before, it only selected that
  unit). Pressing the selected unit deselects it but, as in the original, leaves its promotion
  tree up until the next press; a press on an empty cell with nothing selected brings the pack
  back. The barracks' army grid takes the same clicks, and in the garrison a refused hero or
  named unit stays selected. Dragging a card still works.
- **A unit that cannot be promoted shows its promotion tree locked, as in the original:** at
  its first level, or in a final class, every portrait of the tree, the unit's own included,
  is greyed, tinted dark brown and darkened at the edges. Razdor showed the unit's portrait
  plainly with a note of its own ("The final class…"), which the original does not have.
- **Scenario texts are laid out as in the original, without the stray `*`, `^`, `|` and
  `@`:** an event's window reads these marks as the original does: a line with `*` is white,
  with `|` blue, with `@` orange, the others pale yellow; a line with `^` is centred (the
  tutorials' headings), the others are justified paragraphs with an indent, and blank lines
  stay. The tutorial offer and the restart and delete-save questions read them too. The
  journal still shows them as typed, as the original's does.
- **An attacking army no longer comes out of nowhere:** the other armies' steps were drawn
  one step after the hero's, and an attack opened the battle before the attacker's last
  steps were drawn, so it seemed to jump in from far away. The hero and the armies now move
  together, and the battle opens once the attacker is seen arriving next to the hero, as in
  the original. Walking into an army no longer slides the hero towards it first.
- **A unit's abilities are always shown on its panel:** they came last, under the stats and
  the description, and were cut off at the panel's bottom. A healer's or caster's long stat
  list left no room for its ability (Wrath of God on the priests and bishops of the Evolution
  mod), and long ability texts of mods were cut short. Now the text moves up over the figure
  to make room, and in battle the description gives way first.
- **Garrisons and armies start in a sensible formation:** at a map's start the original
  arranges every army and garrison as it does a side in battle. Razdor did it only for the
  hero's army, so a castle's archers stood in front and its infantry in the reserve's edge
  places (seen on Проклятое озеро).
- **A building's tooltip shows its garrison whoever holds it:** holding the right button on
  your own or a friendly castle or fort now shows its defenders (your units left there
  too), as the original does; before, only a hostile building showed them. As in the
  original, ruins say they are guarded but hide by whom, and towns show none.

### Changed
- **A building is guarded only by an army standing in it, as in the original:** stepping onto
  a castle, fort, village or ruins whose own army is away on patrol storms its garrison (or
  takes it); that army no longer comes from across the map to fight you before you move.
- **A ruins' garrison is weighed as in the original:** until its first battle the AI counts
  its units without the treasure items they were handed at the start, so its armies judge
  such ruins as the original's do.
- **Hiring sounds the gold twice, as the original:** the coins ring as the hire button is
  pressed and start again as it is let go.
- **The victory report after a won battle comes 250 ms after the battle screen closes,** as in
  the original (it came at once).
- **The potion sign on the army and building cards:** a unit that drank a potion shows its
  sign on the army screen and in the building windows too, as the original, next to the
  promotion sign, in the battle signs' places (it showed only in battle).
- **The card stat strip's attack label as the original's:** a shooter's attack is written "A:"
  (Razdor wrote "S:"), with the larger of its two attacks; "Pwr:" is kept for a caster without
  a melee attack or standing outside the four middle places of the front line.
- **The hero's route crosses moving armies, as in the original:** only stationary guards, and
  an army a meeting waits for standing next to you, close the way; any other army's cell can
  be walked through, and stepping onto it while it is still there meets or fights it (every
  army's cell was closed before, which sent some routes the long way round).
- **Save and load on the bottom bar as the original's:** save stands next to the message box and
  load beside it, each with the original's picture; the two buttons had each other's action.
- **Armies on the map step as in the original:** their walk frames follow the game time, so
  a figure no longer marches in place while the world stands still, and a step an army takes
  in place now stands for its time instead of hurrying its next steps along.
- **A building taken from its garrison is entered by walking in:** after the battle you stand
  where you attacked from, as in the original, and a click on the building now walks you into
  it and opens its window on arrival (it opened at once from outside, and the building's own
  events ran without you entering).
- **Spell badges on the unit cards:** the army screen, the building windows and the battle show,
  as the original, up to four round badges along the bottom of each unit's portrait, one per
  running spell that costs mana; hovering one shows the spell's picture, name, effect, the
  unit's life loss and the time left, in the original's words. In battle the potion, blessing,
  poison and curse signs now sit where the original puts them (Razdor's bleed and hero marks
  are gone; the turn number moved to the bottom right).
- **An item dropped on the hero's card goes to the pack** (a potion is drunk), from the pack or
  from any unit, as in the original: the hero wears items through his panel's slots.
- **The wait and centre buttons:** hovering the time panel's message box on the idle map shows
  the original's three buttons over it, with its art and hints: wait 1 hour, centre the view
  on the hero, wait 4 hours. The centre button (and Tab) glides the view back to the hero in
  the original's 900 ms cosine; the map takes no input meanwhile.
- **A click or a key stops a wait** (a Razdor choice, not the original's: its waits always run
  to their end): a left click anywhere or any key during a 1 h, 4 h or endless wait ends it
  after the half hour under way, and does nothing else.
- **The front-row width setting applies at once:** a game under way switches to 6 or 4 cells from
  its next battle (never during one), and its saves record the new width. Units on the two edge
  cells a 4-wide row lacks move to free cells, their own row first; the others keep their cells.
- **A quest taken in a building shows its places at once:** when the quest's message is
  closed in the main hall, the camera flies to the places it marks and back over the map,
  then the building window returns, as in the original (the flight waited until you left the
  building).
- **Sound effects never pile up:** a sound played again while it still plays starts over, as the
  original's one buffer per sound does; sweeping the pointer over the main menu no longer stacks
  its bell into a din. Overlapping ovals no longer count the pointer on two buttons at once,
  which rang the bell every frame between them.
- **A building reached under an event opens after it:** when an event's window stops the
  walk inside the building you clicked, the building's window opens once the event is read,
  as in the original (it left you on the map).
- **Events checked as the building window closes:** after healing, raising, buying or selling
  there, an event your purchase allows opens as you close the window, as in the original,
  not at your next step.
- **The front row's width in the settings:** the settings window chooses 6 or 4 front-row
  cells for new games (the install's "wide front row" until chosen); a saved game keeps its
  own. With 4 the formation is the original's 4-column one, drawn as the original draws it:
  the front row's two edge places, like the back row's, are the reserve's.
- **The view stays while you choose a route:** a click on the map with the hero off screen
  no longer brings the view back to him; it follows him once he sets off, as in the original.
- **The back row's ranged defence is shown:** a unit in the back row shows the `Row2Def`
  bonus (+5) on its card ("D: 0/5") and on its panel ("0 + 5", with a building's defence
  when there is one), as the original's card and panel do; the damage already counted it.
- **Counterblows are seen and heard:** a unit that strikes back now lunges at its attacker
  with the blow's effect and sound on it, after the first blow, as in the original; a unit
  killed by a cursed victim gets the sorcery's effect.
- **A won battle ends as in the original:** the battle screen stays 2.5 seconds with each
  unit's experience on its card, then the victory report opens on the map; no result box to
  click away. A level gained no longer plays the promotion sound (the original plays it only
  in the promotion screen). A pass in battle is a short pause.
- **Building windows sound as the original's:** the window and each tab switch play the
  original's tab sound; the money buttons (buy, sell, hire, heal, raise, learn, a ship) play the
  gold sound; a hired unit's card slides from the recruit into your army; healing plays the
  cure with its effect on the card. A purchase no longer plays the item's sound, and gold
  coming in (midnight income, a sale) no longer plays the gold sound by itself.
- **The village's gold sound when you leave:** the tribute's gold sound plays as the village
  window closes, with the window's button, not as it opens.
- **World spells show where they land:** the camera glides to the army the spell is for (when
  it is far) and the spell's effect plays over it.
- **Places an event shows come right after its window:** the camera flies to them as soon as
  that event's message is closed and back to the hero, and the next message waits for it; a
  new map opens the fog around the hero.
- Smaller sounds as in the original: the wait keys and the time panel click, the main menu
  rings as the pointer comes onto an item, the scenario list is silent, Next and Start click,
  choosing another class plays the menu sound, the bar's icons play the panel sound (their
  windows open silent), and an item sounds as it is picked up and again as it is worn.
- **Only the heroes the map offers:** the new-game hero window offers a class only when the
  map gives it a start cell, greys the others and opens on the first one offered, as the
  original does (before, any class could be started on any map).
- **Battle experience at the install's rate:** XP is paid at the `HeroExpirienceModificator` of
  your `_Global.ini`, as the original does (50 in the Community Update, half the gameplay
  video's rate Razdor used before).
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
- When the hero stops (a walk's or a wait's end, an event or an AI army stopping him), every
  AI army facing a next step with a patrol radius, outside buildings, draws its idle offset
  from the game's generator as in the original, after the windows the stop opened; a run
  into an army or a garrison draws none.
- A building the hero walks into as an event's window opens is entered when the window is
  read, as in the original: a village's offer rolls, its window and its tribute come after
  the event, not with it.
- A new map starts the hero's army in the formation the battle's auto-arrange gives it (the
  best warrior in front, the shooters and mages behind), as the original's map load does,
  not with the preset troops in the reserve.
- The ruins' garrison wears the ruins' goods, as in the original: each of the first five goes
  to the unit it helps most (else into the garrison's pack), so the guards fight with them;
  beating them yields what they wore, then the pack.
- AI armies arrive at the end of each step's play time, as the original's step clock plays
  it, and their arrivals, with the wander points and plans they draw, come in the order of
  their times, not army by army; a midnight inside a wait tick comes between them at its
  moment, and a hero's step is one tick however long.
- A village (or an empty castle, fort or ruins) taken on the way opens no window and does
  not stop the walk, as in the original; its window opens only when the walk ends in it.
- The hero's step time is worked out as he comes onto a cell, as in the original, with
  whether he was at sea before it: his first step on the water after going to sea (or on a
  map that starts him at sea) takes no time.
- An AI army's first step in place after the map load times its play by the cell south of
  it (the direction the map load gives every army), as in the original.
- The AI judges the battles it simulates by the sides' strengths, as the original does, not
  by their hit points: its fears and targets among armies and buildings change accordingly.
- An AI army that starts the map in a building is counted, in the battles it simulates,
  without that building's defence until it next arrives in a building or fights, as in the
  original (its units' strengths are recounted only then).
- A cautious AI army (negative aggression) discounts its own losses to a tenth only when it
  lost no unit in the battle it simulates, as in the original.
- During a battle the armies' units carry their battle HP after every action, as in the
  original (the army record follows the battle, a fallen unit at 0), not only at its end.
- The victory box opens with the event window's chord, which draws from the game's random
  numbers, as in the original.
- The noon report ends a wait, as an event's message does, and opens with the event
  window's chord, as in the original; closing it no longer resumes the wait.
- The ranger's first step on a new map takes his own speed, not the knight's, as in the
  original (it was priced before his class was set: a few minutes too long).
- A village's blessing or witch rolls its spell or mana as the offer is made, before the
  question's chord, as in the original, not when it is accepted; the blessing is any of the
  five blessing spells, as in the original.
- An AI army arriving just as the hero finishes a step sees him on his new cell, facing his
  step, as in the original: one standing right ahead of him stays put instead of walking off,
  and the idle draws of the hero's stop count it as the original does.
- An army the hero beats pays its gold share plus its wage bill as it last counted it, as in
  the original, even when none of its units survives the battle (before, a gang wiped out
  paid no wages at all).
- While the hero is taking a step, AI armies see him on the cell he is leaving until the step
  ends, as in the original: their distances, their plans (a patrol only hunts him inside its
  area) and their contacts no longer jump ahead to the cell he steps to.
- AI armies judge the hero's army with the strength it had when it was last counted (in a
  building window, at an event, at noon, in the army window, after a spell), as in the
  original: after he leaves his town they still reckon with its defence until then, and
  avoid or chase him accordingly.
- Scenario events wait for the window before them: when several events come at once, each one
  after a message is checked and applied only once that message is closed, as in the original
  (before, they all happened at once, so an army an event activates moved, and the time an
  event shows was taken, before the earlier messages were read).
- A price that comes to exactly half a gold piece after a building's attitude rounds the way
  the original's arithmetic takes it: up at attitudes 0 and +2 (75 at 1.1 costs 83, not 82),
  down at −3 and −2, to even at −1 and +3.
- Accepting a village's furs, the witch's mana or the innkeeper's pay shows the result in a
  window of its own, with its chord, as in the original (the blessing and the priest show
  none).
- AI armies attack or greet the hero when he ends a step, not in the middle of one, and still
  do while he stands or waits after a walk, as in the original: a friendly army walking
  beside him greets him when it catches him at a step's end, and one that reaches him while
  he waits stops the wait with its meeting or its battle.
- The AI armies' battles among themselves, and the ones they imagine to choose their way,
  pick their targets as the original's do, including its slip of reading leftovers of an
  earlier battle: the same two armies can fight out differently as the game goes on.
## 0.3.8 — 2026-10-06

### Fixed
- **The map no longer shakes during a wait:** since 0.3.5 every wait tick replayed the hero's
  last step, and the view, which follows him, slid one cell and jumped back each half hour of
  the wait. Thanks to the player who sent the video.

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
