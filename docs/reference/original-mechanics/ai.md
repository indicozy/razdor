# Discord Times: the world-map AI (from the user's Community exe)

How the original game steers every army that is not the player's: how each army scores what
it could go for, how it picks one and walks there, what it does on arrival, how two AI armies
fight, and how beaten armies come back. Read from the Community Update (Unstable)
`DiscordTimes.exe` of the player's own install (Delphi, image base 0x400000) by static reading
only. Rules are in our own words; addresses are virtual addresses in that build, given as
evidence. The battle AI (how units act inside a battle) is in [battle.md](battle.md) §4; the
movement costs, the step clock and sight are in [world.md](world.md); wages and prices in
[economy.md](economy.md); XP and promotion in [experience.md](experience.md). This file goes
deeper than world.md §4–5 and corrects it where they differ (listed at the end).

Confidence tags: **code** = read in the code; **data** = consistent with the data files or
footage, not traced; **unknown** = not determined.

Conventions (as in economy.md): `Round` is the FPU's round-half-to-even (0x402dd0); integer
division truncates toward zero; time is in game minutes; a unit's HP of −1 means unhurt, 0
dead. "Octile distance" is `max(|dx|,|dy|) + min(|dx|,|dy|) div 2` in cells (0x4826f8).
"Rand(n)" is the game's single linear congruential generator (0x4832fc), returning 0..n−1;
every AI roll below uses it, so the order of calls matters for a bit-exact replay.

---

## 1. Who the AI is and what it reads

**code** (map loader 0x4b46e7–0x4b4f7e, AI init 0x4a1ff0).
- Every army of the map is an AI army (army records 1..N; record 0 is the player). Armies
  flagged "inactive at start" are off the map until an event places them.
- Per army the AI uses, from the `.DTm` army record (dtm-format.md §7): faction (byte 64) and
  attitudes to the four factions (65–68); behaviour **style** (59: 0 feudal, 1 rogue,
  2 peasant); **target model** (85, a column 0–4 of the `_Global.ini` priority lists);
  aggression (69, signed); patrol flag and radius (60, 61); home building (25); respawn delay
  (70, days); starting gold (17); daily income (80 × 10); speed correction (13); the flags
  ignored-by-AI (76), hunts-only-the-player (77), no-random-targets (78), no-socialising (79),
  no-interest-in-buildings (81); garrison level (82); respawn-whole-army (83); hire XP flag (14)
  and hire XP bonus (19); units-carry-no-money (62).
- The **patrol box** is the start cell ± radius in both axes, clamped to the map. A patrol
  radius of 0 makes a **stationary guard**.
- The priority lists, one value per target model: `MinAtackArmyTarget`,
  `MinAtackCastleTarget`, `MinRandomTarget`, `MinTalkingTarget`, `MinHealingTarget`,
  `MaxHealingTarget`, `Min/MaxGarrisonTarget`, `Min/Max/GoldPurchaseTarget`,
  `Min/Max/GoldVillageTarget` (ini loader 0x4e4678). A missing key reads as 0. The shipped
  file misspells the minimum-healing key, so that value is 0 there. A model byte above 4 would
  read outside the table (no shipped map does it).
- Global keys used: `AIDistance0..2`, `AIGetPathDistance`, `ZeroDensity`, `NeedUpkeepDay`,
  `HealingConst`, `HealingTime`, `ResurectConst`, `MaxTimeResurection`, `VictoryGoldDiv`,
  `MinVictoryGold`, `AIExpiriencePercent`, `BattleEndTurn`, `CostRecrutDiv`.
- Lower scores are **more** attractive everywhere in this file. 0 means "no interest".

## 2. When an army acts

**code** (driver 0x4ade3c, 0x4a28d0, step clock 0x4a399c).

Every frame of game time, armies 1..N in index order:
1. **Alive or respawn** (§12). An army on the map may act only when its "busy until" time has
   passed (it is set by AI healing, §9.4). An army off the map may respawn.
2. **Step clock** (world.md §2 describes the banking): the army banks the tick's minutes (cap
   200 minutes) and takes a step when the bank covers `cost(cell it leaves) × speed` minutes,
   ×1.5 on a diagonal. The cost map is LAND for land armies and SHIP for ship armies (§13).
   If the next cell is the player's cell or the player's next cell, the step's time is spent
   but the army stays where it is. "The player's cell" here and in §7–8 is his record cell
   (`army(0)+0x1724`): while he steps, the cell he leaves, until the walk timer ends the step
   (world.md §5).
3. When the step ends on a cell ("arrival"), the army may **re-plan** (§7) and then runs its
   **arrival** rules (§8–9). A step arrives when its play time runs out (world.md §5): the
   arrivals of all the armies, and the draws they make, come in the order of their times, the
   armies' order only for arrivals in the same frame; an army's next step starts in the frame
   after its arrival. The play time of a step whose bank does not cover the step after it is
   the rest of the window, so it arrives at the tick's end; the step after is charged on the
   cell this step enters, times the weight of the direction stored with that path point,
   which for the path's last point is whatever an earlier, longer path left in the army's
   path buffer (zeroed at a map or save load: direction 0, weight 3; 0x482fe8 never writes
   the last point's direction).

Details of the step clock that matter for the AI:
- **Stationary guards do nothing at all**: no banking, no steps, no planning, no arrival rules,
  so no noon income or wages either (the noon is run from the arrival rules, §14). They only
  defend.
- An army with no path (or a path of one cell) still "steps in place": every time its bank
  covers the cost of its own cell (no diagonal factor) it arrives again, counts one more
  **idle plan**, picks four new wander points and re-plans. A real step resets the idle count.
- **The step after a step in place** is priced, for the play time, on the cell one step along
  the army's direction (+0x1710, tables 0x4ecf8c / 0x4ecfb0) from where it stands: the step
  clock reads `cell + offset(direction)` whatever the path. Every arrival and every plan set
  the direction from the path (8, no offset, when it has no next cell), but the map load
  writes 5 (south) into every record and the AI's set-up 8 only for stationary guards, so a
  fresh army's first step in place is followed by the cell **south** of it; a respawn keeps
  the direction the army died with (0x4a28d0 does not write it). Checked in the running game
  (Проклятое озеро: army 13 on a cell of cost 4 with a cheaper cell south of it played its
  first step in 20 minutes, not the whole window; FINDINGS.md §10).
- On every arrival the army computes the octile distance to every other army on the map
  (the player included) and adds 1 to its **talk counter** towards each of them (§8). An army
  standing on the very same cell has distance 0 and is treated as absent here: no +1, and it
  does not count for the re-plan test, the rescoring of §7.1 or the erasing of §7.5.
- It **re-plans** when its countdown of `AIGetPathDistance` steps has run out, or when any
  army (or the player) is within `AIGetPathDistance` cells. Reaching the end of its path, or a
  blocked next cell, also picks new wander points and forces a re-plan.
- An army whose cell is beyond both the map's width and height leaves the map (events use it).
- When the player boards a ship, every ship army's re-plan countdown and step bank are set to 0;
  when he lands, the same happens to every land army (0x496d28). So the armies on the medium he
  enters re-plan at their next arrival and start banking from zero.

## 3. Relation between two armies

**code** (0x4a0868).
Let a = army A's attitude to B's faction and b = B's attitude to A's faction (−3..3).
- both ≥ 0: relation = (a + b) div 2;
- a < 0: relation = a;
- a ≥ 0 and b < 0: relation = −1.

Relation < 0 is **hostile**, ≥ 0 friendly or neutral. Whether the factions are equal plays no
role. The player is faction 1 with the attitudes of the map header's first row (0x4b5…,
`0xae158d`). Buildings use their own faction and attitudes (dtm-format.md §6) directly, not
this function (§6, §9.1).

## 4. Simulated battles and the score of an army

**code** (0x4a0710, 0x4a08f8).

**Simulation.** The scoring army fights the other side with the full battle engine, the AI
playing both sides, to the end, from the two static sides that keep what earlier battles
left beyond their units (battle.md, "Killable"): so the same two armies can fight out
differently after other battles. The scoring side takes only its **living, paid** units; the
other side all its living units. Each side keeps its building defence bonus (the defence of
the building it stands in, §9.1). The result is each side's **strength** (483ecc,
experience.md §3: the units' strengths scaled by their HP, by rows) at the start (A0, B0: side
+0x7ec, from the battle's set-up) and at the end (A1, B1: +0x7e8, recounted by the end
0x48bb10; each capped at its start value), and the turn the battle ended on. Not hit points:
checked in the running game (Проклятое озеро, army 9 of five units with 240 HP: A0 = 296;
FINDINGS.md §11).

**The unit strengths are the army's cached ones.** A battle side copies each unit's strength
from its army record (+0x1ae, 49855c), which only the army's recount (0x4a16d4) writes, with
the building defence (+0x378c) the army has at that moment. The recount runs at its arrival
in a building other than a bridge (the end of the arrival rules, 0x4a79c5), when it arrives
outside after standing in one (+0x378c cleared first), after each of its AI battles
(0x4a4c68), at a respawn (0x4a28d0) and at a save load; not on its arrivals in the open.
The map load's set-up (0x4a1ff0) recounts **before** it writes +0x378c, so an army that
starts in a building is counted without its defence until its next recount, while its
battles get the defence (the side's +0x844 reads +0x378c). Checked in the running game
(Проклятое озеро: army 2 starts in a building of defence 15: its side strength is 877 at the
load, 1764 after its first arrival there; FINDINGS.md §12). Garrisons are recounted with
their defence at the load (0x4b2504, 0x4b53dd), so theirs always count it; but a ruins'
garrison is recounted there **before** the ruins' goods are given to its units (0x4a273c at
0x4b55aa), so its strengths count the units without those items until its next recount: a
real battle (0x4a4c68, the player's 0x4d21fd), a garrison purchase (0x4a704e) or a reshuffle
(0x4a7972). Checked in the running game (РК4: a garrison with a ring on one unit fights its
off-screen battles at B0 552, not 617; FINDINGS.md §28). A spell that leaves someone
alive in an army recounts it too (0x4900fc → 0x497240).

The player's side (army 0) is copied the same way. His +0x378c is the defence of the
building he stands in when it is his, else 0, written when he comes onto a cell (0x497c68);
his recount runs at the map load's end (0x4b5b64), at every event window closed (Event_Finish
0x4ab1ec, the victory and noon reports and the village window included), at his noon
(0x4abfbc), in a building window on the hire and garrison tabs and at its close (0x4ba854;
the hire tab 0x4bd3a4), when his army window opens (0x4d1814) and after a spell on his army
(0x4900fc); not when he walks. So after he walks out of his town an AI army still scores him
with the town's defence until the next of those (Другой берег: B0 928 in the open, his
units' +0x1ae counted with 15; FINDINGS.md §21). The battle's own defence is the one of
where he stands.

**Score of army A against army C** (computed for A, cached per pair, §7.1):
1. If nothing happened (A1 = A0 and B1 = B0) or the battle ran to `BattleEndTurn`: score 0.
2. Aggression g (A's byte 69, in percent) shifts both results: `B1 −= Round(g·B0/100)` (not
   below 0) and `A1 += Round(g·A0/100)`. For a negative g the second term uses 1000 instead of
   100 (a tenth of the effect) unless side A lost a unit: its living count at the end (the
   side record's first word, 0xc081ac) below its start count (+4, 0xc081b0; experience.md
   §0); a surrendered side counts none. A1 is then floored at 0. Checked in the running game
   (Проклятое озеро, army 2 with aggression −25: A1 shifted by −219 = Round(−25·877/100)
   after losing units, by −22 after none; FINDINGS.md §13).
3. **Win** if A1 > 0 and A1 > B1:
   - if A1 < A0: `s = Round((1 − A1/A0) × 30·ZeroDensity × A0/B0 + 1)`;
   - else (no loss): `s = Round(A0/B0 + 1)`;
   - s is at least 1. A cheap win against a strong enemy scores lowest (best); crushing a weak
     enemy scores high (it is barely worth the walk).
4. **Loss** otherwise: `s = −5 − Round(√(B0/A0) × ZeroDensity × speedA / speedC)`, not below
   −50 (speed = the step multiplier, larger is slower).
5. By relation r (§3):
   - r ≥ 0: `s = s div (r + 1)`;
   - r < 0 and s < 0: `s = ((2 − r)·s) div 3` (fear grows with hostility);
   - r < 0 and s > 0: `s = (AtackArmy[model] + s) × (r + 4)` (−3 → ×1, −1 → ×3).

A negative score means "danger" and is used to keep away (§7.4); a positive one is a target.

## 5. Spare gold

**code** (0x4a0530). The gold an army may spend on purchases.
- `cost = (daily wages + 2 × Σ Round(Cost/CostRecrutDiv) over its living units) div 3`
  (the second sum is untiered and includes the leader; 0x4a188c). "Daily wages" is the army's
  gold wage bill; in this Community build the wages of kind-1 Elemental units are moved to a
  separate mana bill and are not in it (0xc25e97, 0xc25ec5; economy.md §1).
- Feudal: `spare = gold − NeedUpkeepDay·cost + (today's income + village average) ×
  (NeedUpkeepDay − 1)`.
- Rogue, peasant: `spare = gold − cost + today's income + village average`.
- Clamped to 0..gold. "Today's income" is set at its noon (§14); the village average is
  `(previous average + today's village gold) div 2` at every midnight, starting at 50.

## 6. The score of a building

**code** (0x4a0ba4). For army A and building B; recomputed when the army is set up, at every
midnight, at its noon, after a battle it survived, and for its home when it does not own it
(§9). Bridges score 0. Four parts, each 0 when it does not apply:

**Village part.** When the building has gold in stock and is a village, or is A's own
building: `v = Round((1 − stock/(GoldVillage + spare)) × (MaxVillage − MinVillage))`; a
negative v becomes MinVillage, otherwise v + MinVillage. Rogues and peasants: ×3.

**Purchase part.** Only when spare ≥ 1 and the building's attitude to A's faction is ≥ 0.
- Recruits: if A has fewer than 12 units, take the barracks slots whose unit has the same
  Nature as A's leader: `U = (Σ count·Cost div Σ count) × min(Σ count, 12 − units)`. Price and
  stock affordability are not tested here.
- Goods: market items whose absolute price ≤ spare: `I = (Σ price div n) × min(n, free item
  slots of all its units)`.
- `V = max(U, I)`; V < 1 → 0, else `Round(MinPurchase·GoldPurchase / V +
  MaxPurchase·GoldPurchase / spare)`.

**Attack part.** The building is attackable when the building's attitude to A's faction or
A's attitude to the building's faction is negative, A does not own it and A is feudal or rogue;
but a **town** only when A's attitude to its faction is exactly −3, and never a tavern,
church, smithy or obelisk (types 5, 7, 8, 15). Then:
- `base = Round(AtackCastle × 50 / (income + 1))`; a result of exactly 0 becomes 1.
- Empty garrison: `base div 4 + 1`.
- Otherwise a simulated fight against the garrison (with its defence). Unless nothing
  happened or it timed out (then base stays): a feudal army counts as winning when
  `A1 + Round(g·A0/100)` is still ≥ 1 (always ÷100 here, never the ÷1000 variant of §4; the
  garrison's result is not compared); a rogue when the garrison's strength left is at
  most `B0 div 2`. No win → −100000 (forbidden). A win adds
  `Round(((1 − A1/A0) + B1/B0) × 30·ZeroDensity + 1)` (unshifted results), at least 1.
- A building with no income and an empty garrison: ×50.
- A stationary guard (any army, the player included) standing in the building that A cannot
  beat — its cached score < 0, or a fresh score < 0 — makes it −100000.

**Garrison part.** A's own building with a garrison: with S = A's strength, G = the
garrison's (a strength is the sum of the tactical costs of all units of the record, dead ones
included, each forced to at least 1 by the Community hook 0xc25d86; 0x4a1828): S < G → `Round((MaxGarrison − MinGarrison)·S/G) + MinGarrison`; G < S →
`Round((MaxGarrison − MinGarrison)·G/S) + MinGarrison`; equal → 0.

**Result:** the smallest positive part, or 0; if the attack part is negative the result is −1
(a forbidden building, an obstacle in §7.3).

## 7. Choosing a goal

**code** (0x4a2d88 and the planner unit 0x482750–0x483244). Each re-plan rebuilds everything:
one flood over the map from all candidate targets at once, then a path read back from the
army's cell. There is no stored "goal": the army follows whatever the latest flood says.

### 7.1 Army scores
For every army (and the player) within `AIDistance[style]` (index = A's style byte, 0x4a24f5)
whose pair is marked **dirty**, the score of §4 is recomputed and the pair cleared. Pairs are
marked dirty in both directions after a battle of either army, a respawn, hiring or healing,
a feudal army's noon (its wage payment ends with it, 0x4a41d8), and when the player's army
changes: after his battles, his noon, an event that took effect (0x4ab1ec) and in a
building's window (0x4ba854), all through 0x497240(0, 1) or 0x4a26e8. A pair out of range keeps its **last
score**, which is still used below: the range limits rescoring, not attraction.
At map start (0x4a1ff0) each pair of AI armies within the first army's `AIDistance`, neither of
them a stationary guard, is scored once and cleared; pairs with the player or a guard stay dirty.

### 7.2 Seeds
Seeds are (cell, value) pairs; values are capped at 32766.
1. **Healing.** If the army has a heal or resurrection bill and spare gold > 0:
   `h = Round((1 − missing HP/max HP of its living units) × (MaxHeal − MinHeal) + MinHeal)`;
   if the heal bill exceeds the spare gold, `h = Round(heal bill × h / spare)`. For every
   building with the services flag (dtm byte 294) whose attitude to the army's faction is ≥ 0,
   its stored building score is lowered to h if it is above h — or to 3h, at a town or church
   when a resurrection is needed. This changes the stored score until the next rescoring, and
   it never turns a 0 or negative score into a target. It can remove one, though: with the
   shipped minimum of 0 (see §1), an army that has lost nearly all its HP gets h = 0, and every
   service building's positive score is then overwritten with 0, so it is no longer seeded. (Heal bill per wounded unit:
   `Round(HP × Cost / maxHP × HealingConst/100)`, current HP; resurrection bill per dead unit:
   `Round(Cost × ResurectConst/100)`; 0x4a188c, 0x4a1828.)
2. **Buildings** (unless no-interest-in-buildings): every building with a positive score,
   at the cell `(left + width div 2, top + height div 2)` of its footprint; a patrolling army
   only if that cell is inside its patrol box.
3. **Armies and the player**, each other army on the map:
   - hostile (§3): its cached score;
   - friendly, with socialising forbidden: 0;
   - friendly: from the talk counter c: `c div 100` when c ≤ 0 (truncated, so 0 for c from −99
     to 0 and a small negative only from −100 down: keep away for a while after meeting; right
     after a greeting, c = −500 gives −5), else `max(0, 800 − c) + Talking[model]`;
   - hunts only the player: other armies' positive values become 0; a value of 0 for the player
     becomes 1 (so a friendly messenger always heads for the player);
   - the other army is ignored by the AI: 0.
   - A positive value is seeded at that army's cell, except: a patrolling army seeds armies only
     while it stands inside its own box and the target is inside it too; and no army targets at
     all while the "just respawned" flag is set (§12; in practice that flag is always clear by
     the time a plan runs).
   - A negative value makes two **repulsion cones** around that army, when both are on the same
     medium (land or water): strength `−s` with slope 1 and strength `−5s` with slope 25
     (slopes ×5 for a stationary guard: its danger zone is five times narrower). See §7.4.
4. **Random points** (when random targets are allowed, or after more than 10 idle plans in a
   row): the army's four wander points, each with `Random[model]`.

**Wander points** (0x4a2550): four points; for a patrolling army `x0 + Rand(x1 − x0 + 1)` then
`y0 + Rand(y1 − y0 + 1)` inside its box (draws returning to 0x4a2594 / 0x4a25d8), otherwise
`Rand(width)` then `Rand(height)` anywhere on the map (0x4a2624 / 0x4a264d; the width and height
are the planner's, +0x376a / +0x376e, the map's size); the box is the one the loader writes
(+0x16c0..+0x16cc: the start cell ± radius, clamped to the map), checked in the running game on
РК1 (17 × 14, 40 × 40 and the whole 50 × 50 map for armies 1, 9 and 14, the same as Razdor's); a point equal to the army's cell is dropped, and a point in column 0, or on a cell the
obstacle pass of §7.3 has closed, is not seeded.
New points are drawn when a path ends or is blocked, after a meeting, and at respawn (then
cleared and the first one set to the army's start cell).

### 7.3 Obstacles
On a per-plan multiplier map (all 1 to start):
- footprints of buildings with a negative score are closed (left/top cell to width × height);
- the cells of all stationary guards on the map are closed;
- the cells of armies within 10 cells (octile, < 11) are closed when either the planning army
  or that army is ignored by the AI (the player is not in this loop; an ignored planning army
  closes its own cell too, which only means the flood never stops early);
- repulsion cones add to it (§7.4).
The effective cost of a cell is `cost × multiplier`; 0 is impassable. Seeds on a closed cell
still count (an army can target a stationary guard).

### 7.4 Repulsion cone
For strength S and slope f around cell (x, y): every scanned cell whose value
`S − floor(f × (max(|dx|,|dy|) + min(|dx|,|dy|)/2))` exceeds 1 gets that value added to its
multiplier. The scan box is not centred: with `k = Round(16384·f)` and
`r = ((S − 2)·32768) div k` (about `2(S−2)/f`), it covers columns `x − r − 1 .. x + r − 1` and
rows `y − r − 1 .. y + r − 1`, clamped to the map. For r ≥ 1 this does not cut any value above
1 in practice, but when r = 0 the box is the single cell up-left of the centre, whose value is
never above 1, so **a weak or steep cone adds nothing at all** (except next to the map's left or
top edge, where the clamp to column or row 0 can bring the centre back in): the slope-1 cone needs S ≥ 3,
the slope-5 cone S ≥ 5, the slope-25 cone S ≥ 15 and the slope-125 cone S ≥ 65. With the
strengths of §7.2 (−s and −5s) a score of −1 or −2 has no effect, and a stationary guard's cones
need s ≤ −5 (first) and s ≤ −13 (second). So danger multiplies the cost of nearby cells; the
army routes around armies it cannot beat instead of avoiding them as a goal.

### 7.5 Flood and path
- The flood starts from every seed at once, each with value + 1 (a seed on the army's own
  cell is dropped). Entering a cell costs its effective cost × 2 (orthogonal) or × 3
  (diagonal). A walk therefore costs `seed + 1 + Σ (2 or 3) × effective cost of the cells it
  leaves` (world.md §5). The effective cost is `cost × multiplier` in 16 bits, for every cell
  except the map's cell (0, 0), whose multiplier is left as it is (the multiplying loop stops
  before index 0).
- It is a Dijkstra in batches: all frontier entries at the current lowest value expand, in
  insertion order, neighbours in direction order 7 down to 0; it stops the moment the army's
  cell is first given a value. Values are 16-bit. Because of that early stop the army's cell
  holds the first value it was offered, which is not always the lowest it would have reached
  (a later batch could have improved it), so the cheapest target does not always win.
- Two seeds on one cell: a new seed is rejected only when an existing seed there is lower by
  2 or more. A later seed exactly 1 above an earlier one is therefore kept and, written later,
  overwrites the cell's value (0x482984, an off-by-one); the earlier, lower seed still expands
  from its own value, so only the seed cell's own value ends 1 higher.
- Then every cell of an army (or the player) within `AIGetPathDistance` cells, and the cell
  that army steps to next, is erased from the flood, so the path never steps onto them
  (for the player: his logical cell and that cell plus his direction, world.md §5; an erase
  of the planning army's own cell leaves it nothing lower to step to, so it stands).
- **Path**: from the army's cell, repeatedly step to the neighbour with the smallest non-zero
  flood value below the current one (direction order 0..7, the first of equals wins), until
  none is lower. No seed reachable → a one-cell path (the army stays and counts idle plans).
- The countdown is set to `AIGetPathDistance` steps, but only when at least one seed was kept.
  With no seed at all the path is set to one cell and the countdown is not touched (it is
  usually 0 already, so the army re-plans at every arrival).

## 8. Meeting other armies

**code** (contact loop in 0x4a548c, driver 0x4ade3c). On each arrival, for every other army
on the map, the player included:
- if the relation is ≥ 0, the army's talk counter towards it grows by relation + 1 (on top of
  the +1 of §2);
- if the other army is on a neighbouring cell or the same cell (|dx| ≤ 1 and |dy| ≤ 1; no
  test of land or water):
  - **hostile**: if the other army is in no building, on a bridge, or in its own building,
    and the cached score is positive and the other is not ignored by the AI, the arriving army
    attacks: the player gets a battle (§8.1), another army an AI battle (§10). If the other
    army is inside someone else's building, there is no fight; both pair scores are tripled
    (when positive), capped at 10000;
  - **friendly**: with the player, if the talk counter is positive the army **greets** him
    (a meeting: the event scan runs with that army). Two friendly AI armies "greet" whenever
    they are adjacent (no event). A greeting sets the arriving army's counter to −500 and gives
    it a re-plan and new wander points, and sets the other side's counter towards it to −500
    as well. With the player and a counter ≤ 0 only the player's side is reset.

### 8.1 Attacking the player
An attack or greeting of the player takes effect only while his step flag is set (0x75e0c7:
set in the frame where a step of his ends and the next begins, 0x4ae975, cleared at the top of
every frame of the walk timer, 0x4ae71e, and written nowhere else): during a walk only in the
frame his step ends; after a walk, while he stands, waits or casts, until his next walk
(world.md §4.3); before his first walk on the map, never. If several armies attack in one frame, the last one in index order is the
foe; likewise the last greeting army is the one met. **A greeting wins over an attack**: if any
army greeted the player in a frame, the event scan runs for the greeting and no attack of that
frame is carried out (0x4ade3c). An attack stops the player, runs the event scan, and starts
the battle if no event fired.
An army that attacks the player's building while he is inside fights him (§9.1).

## 9. Arrival in a building

**code** (0x4a548c). Before the building rules, on every arrival: the home building is
rescored if the army does not own it; if its noon has passed, the noon is run (§14) and all
buildings rescored; with a resurrection bill, dead units whose time of death is older than
`MaxTimeResurection` are removed. Arriving on **any** cell of a footprint counts as being in
the building. Not in a building: the remembered building and its defence are cleared.

### 9.1 Assault and capture
The army assaults when the building's attitude to its faction, or its attitude to the
building's faction, is negative, it does not own it and it is feudal or rogue; a town only at
attitude −3. The building types excluded from the score (§6) are **not** excluded here, so an
army passing through a hostile tavern, church, smithy or obelisk on its way fights it too.
- The player's building with the player inside: the army attacks the player.
- Otherwise it fights the garrison (§10; an empty garrison is won at once). If the building's
  owner army stands in it (and is not ignored by the AI), it then fights that army too,
  **whatever the garrison fight gave**: the owner fight's result alone decides the capture, so
  a garrison fight that timed out can still end in a capture, and an army wiped out by the
  garrison is still sent against the owner.
- Lost: nothing changes (the building's stored score is still zeroed, §9.7). Won: villages, castles and forts — and a town at attitude −3 — become
  the army's (owner, faction and attitudes copied); it becomes the army's home if it had none.
  Altars and ruins become neutral (no owner, faction 3, attitudes 0). Other types keep their
  owner.
- The army's battle defence becomes the building's garrison defence when it took it or owns it
  (in a friendly building it keeps the previous value; unknown whether intended).

### 9.2 Village gold
Feudal armies only: on any village with gold in stock, owned or not and whatever its
attitude (after a won assault too; a lost one ends the arrival), the army takes the whole gold
stock and the mana stock is emptied too; the gold counts towards the village average of §5.

### 9.3 Shopping
Feudal and rogue armies, in a building with goods whose attitude to them is ≥ 0: markets and
churches for armies whose leader is not undead, altars for undead-led ones.
1. Every pack item with a positive price is sold for `RelationPrice(price) div 2`
   (economy.md §2 has the relation price).
2. If the cheapest good is affordable with spare gold: for every unit and good, the good is
   tried on the unit; if it raises the unit's tactical cost, its value is the gain over the
   unit without items. Then repeatedly the pair with the largest value above 5 whose relation
   price ≤ spare gold is bought and worn; the good leaves the shop. Values are not recomputed
   between purchases.

### 9.4 Healing and resurrection
Feudal and rogue armies, in a building with the services flag whose attitude to them is ≥ 0,
or their own:
- **Heal** (with a heal bill): only if the barracks offer some unit that is undead exactly when
  the leader is. Each wounded unit costs `RelationPrice(Round(Cost × HealingConst/100 ×
  HP/maxHP))` — its **current** HP, so a badly hurt unit is cheap — and is healed fully when
  its price is below the gold. Each heal sets "busy until now + HealingTime" (not
  cumulative): the army stands still that long.
- **Resurrect** (towns and churches, with a resurrection bill): the dead unit of highest
  tactical cost first, for `RelationPrice(Round(Cost × ResurectConst/100))` when below the gold
  (busy as above); a unit it cannot afford is skipped. The loop's last round runs once more
  with no candidate and reads memory before the army's first unit (a bug of the original; do
  not reproduce).

### 9.5 Hiring
Same conditions as 9.4. While spare gold ≥ 1 and the army has fewer units than the cap
(12):
1. Sum the tactical cost of its units by role: warriors ×1, shooters ×2, mages ×2 (w, s, m).
   Preference order: if w < s and w < m, warriors, then shooters if s < m else mages; else if
   s < m, shooters, then warriors if w < m else mages; else mages, then warriors if w < s else
   shooters.
2. Go through the preferences, each over the 6 barracks slots, and hire the first unit in stock
   of that role whose Nature equals the leader's (a leader of unit GlobalIndex 74 may hire any
   non-undead), whose relation price ≤ spare gold. Reaching the third preference lowers the cap
   to 8 for the rest of the visit; the cap is only tested before each pass, so a unit found in
   that same third-preference pass is still hired even when the army already has 8 or more.
3. The unit is hired at level 0 as a recruit (wage kind 1) in the army's own building, as a
   mercenary (kind 2) elsewhere (economy.md §1). XP: see experience.md §5 (the hire XP rule);
   it is fed level by level with a promotion try at each level (§11).
4. After each hire the role sums are recomputed. Then the army's items are redistributed
   (§10.1).

### 9.6 Garrison buying and reshuffle
A feudal or rogue army, in its own town, castle or fort, with a garrison level (byte 82) and a
positive stored score:
- **Buying**: while its spare gold is above a third of the gold it had when it started, and the
  garrison has fewer than 12 (later 8) units, it buys into the garrison with the same
  preference rule, using the garrison's role sums and the leader's Nature (no exception), as
  recruits. The unit's price is not compared with the gold: it is simply deducted, so the
  gold can go below zero.
- **Reshuffle**: all living units except the leader (army and garrison together; the dead are
  dropped for good) are dealt again between army and garrison:
  - L = garrison level, D = the building's garrison defence, `q = L / (D/25 + 1)`;
  - quota table T[army][role], T[garrison][role] start at 0, then the garrison row holds the
    pool's tactical cost per role; if q < 50 the garrison row becomes `Round((100 − 2q)% of
    it)`; if q ≥ 50 the army row becomes `Round(2(q − 50)% of the pool sums)` and the garrison's
    warrior cell 0 (a quirk); every garrison cell then loses 2 × income, floored at 0 — a
    fort's cells first get `150 − 2 × income` added, so a fort's cells change by
    `150 − 4 × income` in all (the income is subtracted twice);
  - units with the Garrison bonus go to the garrison first, adding `Round((D/25 + 1) × cost)` to
    the garrison's warrior cell;
  - up to 201 rounds (the round counter is tested before it is raised): the cell with the smallest value (scan army before garrison, mages
    before shooters before warriors, the last of equals wins) takes the unassigned unit of that
    role with the highest `Round(cost + √XP)`; the army cell grows by the cost, the garrison
    cell by `Round((D/25 + 1) × cost)`; no unit of that role, a full side (12), or an army out
    of spare gold adds 100000 to the cell (out of spare gold: to all three army cells). Units
    left after the last round are lost. Garrison-bonus units are placed without the 12-unit
    check.

### 9.7 After the visit
The stored score of that building is set to 0 for this army (until its next rescoring: noon,
midnight, or a battle it survives). After a full visit it also re-plans at its next arrival;
the short exits (a lost assault, a bridge) skip that reset, though a lost assault re-plans
anyway through §10.

## 10. AI against AI

**code** (0x4a4c68). The arriving army attacks; the defender is an army or a building's
garrison. One simulated battle with the full engine decides (only the attacker's paid units
fight).
1. Units' HP, deaths (time of death = now) and stats are written back.
   "Wiped out" below means a side **strength** of 0 at the end (side +0x7e8, experience.md
   §3), not an empty side: a lone shooter or mage whose fifth rounds to 0 is beaten while it
   stands, and a side that surrendered is 0.
2. **Attacker wiped out**: off the map, destroyed, "beaten by" the winner. A feudal or rogue
   winner takes the loser's wage total (if the loser is feudal) and its gold: all of it if below
   `MinVictoryGold`, else `gold div VictoryGoldDiv`. All the loser's items go to the loot pool.
3. **Attacker survives**: each surviving unit gains XP (§11); a dead leader is set to 1 HP; the
   dead units' items go to the pool.
4. **Defender wiped out**: an army is beaten as above; a garrison is emptied. A feudal or rogue
   winner takes, from a town, castle or fort, its income + the garrison's gold + its stock
   (the stock is emptied); from an army, its wage total (when the loser is feudal or rogue) and
   `gold div VictoryGoldDiv`; from the garrison of any other building, `gold div
   VictoryGoldDiv` of the garrison's gold — here with **no** `MinVictoryGold` rule. All its
   items go to the pool.
5. **Defender survives**: XP for its survivors, its leader at 1 HP (armies only), its dead
   units' items to the pool.
6. The pool goes to the attacker when its end strength is strictly greater than the defender's,
   otherwise (ties included) to the defender; that side wears the best items (§10.1). The
   attacker re-plans; the survivors rescore all buildings.
7. A battle that ends with both alive counts as a defeat for the attacker's purposes (the
   building is not taken), with no other effect.

### 10.1 Item handout
Pool + pack: repeatedly the (unit, item) pair with the largest tactical-cost gain above 5 is
worn (first of equals wins); the rest fills the pack by price, dearest first, up to 12; the
remainder is lost. **code** (0x4a473c)

## 11. XP and promotion of AI units

**code** (0x4a4a7c, 0x4a4c04, loader 0x4e0448).
- Battle XP: `award × AIExpiriencePercent div 100` per surviving unit (experience.md §3).
- After every gain the unit tries the upgrade tree: Militia (unit 4) takes slot 1 with
  Rand(3) = 0, else slot 3; Infantry (unit 8) slot 3 with Rand(3) = 0, else slot 1; any other
  class repeats `Rand(3) + 1` until it hits a filled slot. The roll is made even when the level
  is too low. If the slot's `NextUnitNLevel` ≤ the unit's 0-based level, the unit becomes that
  class at level 0 with 0 XP and its worn items go to the loot pool.
- The unit loader rearranges the options: a lone option goes to slot 2, two options always end
  in slots 1 and 3. Militia and Infantry have two options each in the shipped unit file, so
  their picks never land on an empty slot (this settles experience.md's open point). Were one
  of them given a single option, the pick would land on an empty slot whose level is 0 and the
  unit would turn into an invalid class (not reproduced; no shipped data does it).
- Hired units get their starting XP level by level, with this promotion try at each level.

## 12. Respawn

**code** (0x4a28d0, 0x496834, 0x4c50ec).
- A beaten army is off the map and destroyed, with its time of defeat. It comes back when it
  has a home, a respawn delay (byte 70 × 1440 minutes) and the delay has passed (strictly).
- **Where**: feudal armies use their home if they still own it, else the first town they own,
  else castle, else fort (in building order); owning none cancels the respawn for good. Rogues
  and peasants always use their home, and if it is a village, shipyard, altar or **ruins** they
  take it over (owner, faction, attitudes), from whoever holds it, the player included.
- It appears at the centre of that building's footprint with every unit in its record at full
  HP and paid, no path and a stored step cost of 0 (so it arrives again at once, an idle
  plan), gold + (delay in days) × daily income, the "just respawned" flag set (no army
  targets until it finishes a path) and its first wander point at its start cell.
- **Who comes back** depends on who beat it: beaten by the player, the record keeps only the
  leader unless the respawn-whole-army flag is set; beaten by an AI army or a garrison, the
  record keeps all its (dead) units, so the **whole army** returns whatever the flag, but
  without its items.
- An army removed by an event (deactivated) is not "destroyed" and never respawns; that holds
  for a beaten army waiting for its respawn too (0x496900 clears its "destroyed" flag), while
  an activation brings back any army off the map, a destroyed one included, and clears its
  "destroyed" flag and "beaten by" mark (0x4969b8). A respawn clears both as well, so the
  events' "beaten" conditions no longer hold for an army that came back.
- There is no retreat of a beaten lord into his castle (none found).

## 13. Ships

**code** (loader, 0x4a1ff0, 0x4a2d88). An army placed on water (terrain code 0–2, not on a
bridge) is a ship army for good: it plans and steps on the SHIP cost map (water and building
footprints). The ship type byte only picks its picture. Ships have no AI of their own:
targets on cells the SHIP map closes (land) are dropped, so ships go for other ships, the
player at sea, and buildings (footprints are open). Repulsion only acts between armies on the
same medium; contacts (§8) do not check the medium.

## 14. Noon and midnight for the AI

**code** (0x4a548c → 0x4a41d8; 0x4a1998).
- **Noon** is run lazily at the army's first arrival after 12:00 (next noon kept per army, set
  to 12:00 of the following day), not at 12:00 itself: the gold gained = daily income + the gold
  stock of its castles and forts + that of villages linked to its buildings, all stocks emptied;
  feudal armies pay wages (economy.md §1). The "today's income" kept for §5 counts the castles'
  and forts' own **income** instead of their stock (base income + their incomes + the linked
  villages' stocks; 0x4a41d8).
  Rogues and peasants get income and pay nothing. Stationary guards never get a noon.
- **Midnight** (for every army on the map): an army with a medic heals every wounded unit by
  10% of its maximum; the village average is updated; every AI army rescores all buildings.

## 15. Attitudes and factions over time

**code**. An AI army's faction and attitudes never change in the original code itself; only
the Community event opcodes can set them (0xc27862, events). Being attacked does not change
anyone's attitude. Buildings change attitudes only on capture: an AI winner copies its own
(§9.1), altars and ruins go neutral, a rogue's respawn takes over its home (§12), and the
player's captures copy his (economy.md §3).

## 16. Random rolls

All from the one game generator (0x4832fc), in this order of appearance: wander points (x then
y, four times) whenever they are renewed; promotion picks after every XP gain of an AI unit;
the hire XP roll; plus whatever the battle engine draws inside simulations (battle.md). Goal
choice, scoring and the flood themselves are deterministic. The same generator also serves other
subsystems in the same frame (the midnight barracks restock right after the army loop, one roll
per army at map load), which matters for a bit-exact replay.

---

## Razdor now → original

Razdor's AI is `src/rules/ai.rs` (with `game.rs` `move_armies`/`ai_contact`, `map.rs`
`flood_maps`/`descend`, `world.rs` unit records and respawn data). "Razdor now" is the state
after the parity pass.

| Topic | Razdor now | Original | Work |
|---|---|---|---|
| Thinking cadence | Re-plans at an arrival when its countdown of `AIGetPathDistance` steps ran out or any party is within that distance; one flood from every seed at once, in the original's pass order (`TileMap::flood_maps`), path by steepest descent (`Game::ai_plan`) | Re-plans on arrival every `AIGetPathDistance` steps, or every step while anything is within `AIGetPathDistance`; one flood from the targets, path by descent (§2, §7.5) | Matches |
| Goal memory | No goal kept; a visited building's stored score is zeroed until its next rescoring | No goal kept; the flood decides each time; a visited building's score is zeroed until rescoring (§9.7) | Matches |
| Step cost | Cost of the cell left; a step into the hero's cell (or the one ahead of him) waits; an army with no path steps in place on its own cell's cost, each an arrival; a step in place the hero bars counts as a step or an idle plan by the original's path index and length; after a respawn or an activation the first step is free (its stored cost 0) | Cost of the cell left; a step into the player's cell (or next cell) waits (§2) | Matches |
| Stationary guards | Never bank, step, plan, arrive or get a noon | Never step, plan, arrive or get a noon (§2) | Matches |
| Relation | §3 for every decision of the AI (`relation_between`), factions not compared | Two-sided rule of §3, factions not compared | Matches |
| Range | Pair scores cached per army with dirty flags (marked after battles, respawns, hiring, healing, a feudal noon; the hero's after his battles, his noon, a fired event, a building's window); only those within `AIDistance[style]` rescored, the others still seeded | Range limits *rescoring*; cached scores of armies out of range still attract (§7.1) | Matches |
| Army score | §4 (`army_score`): shifted results, relation scaling, negative scores; for a negative aggression ÷1000 only when the side lost no unit | §4 exactly | Matches (÷1000 always until 2026-10-03) |
| Simulated battle results | the sides' strengths at the start and the end (`simulate`) | side strengths +0x7ec / +0x7e8 (483ecc) | Matches (Razdor counted hit points until 2026-10-03) |
| Cached unit strengths | an army's sides count its units with the defence of its last recount (`AiMind::strength_bd`): 0 from the map load, the building's after an arrival in it, an AI battle or a respawn | +0x1ae per unit, written by 0x4a16d4 only | Matches |
| A ruins' garrison's strengths | counted from its units' level stats, without the items the load gave them, until its first recount (a battle, a purchase, a reshuffle; `Location::strengths_bare`) | recounted at the load before the items (0x4b53dd, 0x4b55aa) | Matches (until 2026-10-04 Razdor counted the items) |
| Danger | Two repulsion cones per danger on the multiplier map (`repulsion`, the original's box), ×5 slope for guards, same medium only | Repulsion cones around losing matchups (§7.4), ×5 slope for guards | Matches |
| Peasants | Score armies and buildings (no assault, villages ×3), talk and wander | Peasants score armies, buildings (no assault, ×3 villages), talk and wander like others (§6, §7) | Matches |
| Building score | The four parts of §6 (`Game::building_score`); −1 forbids and closes the footprint | The four parts of §6, smallest positive wins; −1 forbids and blocks the footprint | Matches |
| Healing | The heal seed lowers the stored scores of friendly service buildings; heals priced by the current HP, `HealingTime` busy from the last; the dead raised dearest first in towns and churches | Heal seed lowers existing scores only (§7.2); heal cost uses current HP; busy `HealingTime` once (§9.4) | Matches |
| Village gold | Feudal, any village it stands on, its mana thrown away; the village part for villages and its own buildings with stock | Feudal, any village it stands on (§9.2); score part for villages and own buildings with stock | Matches |
| Shopping | Sells the pack at half, then buys by tactical gain above 5 while the spare gold covers it, values not recomputed (a good bought for a unit that can no longer wear it is paid and lost), a good of negative price at its absolute price; markets and churches, or altars for an undead leader | Sell the pack at half, then buy by tactical-cost gain > 5 (§9.3); markets/churches or altars by leader nature | Matches |
| Hiring | Role order by tactical sums, leader's Nature (unit 74 any non-undead), the six barracks slots scanned with their empty ones, the cap of 8 set on entering the third role (also by a hire from the last slot of the second) and tested only before a pass; kind 1 at home, 2 abroad; hire XP level by level | Role balancing, leader-Nature match (unit 74 exception), third role caps at 8 (§9.5); kind 1/2 by ownership | Matches |
| Garrison | Buying while the spare gold is above a third of the starting gold (the price simply deducted), then the quota reshuffle by byte 82 and the defence | Buying and the quota reshuffle of §9.6, driven by byte 82 and the defence | Matches |
| Contact | At every arrival: a hostile neighbour in no building, on a bridge or in its own is attacked when the cached score is positive and it is not ignored; in someone else's building both scores are tripled (cap 10000) | On arrival, hostile neighbour with a positive cached score, not ignored; no fight inside someone else's building (§8) | Matches |
| Greeting / talk | Talk counters per pair (+1 per arrival at a distance, + relation + 1 when friendly), −500 on both sides after a greeting, talk seed `800 − c + Talking`, `c div 100` below 1 | Talk counters (+1 per arrival, + relation + 1), −500 after a greeting, talk seed `800 − c + Talking` (§7.2, §8) | Matches |
| Messenger | Falls out of the seed rule (a 0 for the player becomes 1) and the greeting | Falls out of the seed rule (value 0 → 1) and the greeting (§7.2) | Matches |
| AI vs AI loot | Asymmetric: `MinVictoryGold` only when the attacker loses, wage bills by style, a garrison's gold; worn items pooled and handed out by tactical gain to the side with more HP, the rest packed dearest first | Asymmetric (§10): MinVictoryGold only when the attacker loses; wage totals; pooled items to the side with more HP, best-gain wearing | Matches |
| Beaten in its own arrival | Off the map, the rest of the arrival runs with its record (another fight with nobody, village gold, shopping, healing, hiring), then it leaves | The arrival goes on with the beaten record (0x4a548c) | Matches |
| Leader survival | Leader at 1 HP when its side survives; it, and a unit resurrected, keep the first time of death (cleared only by a respawn or an activation) | Leader at 1 HP when its side survives (§10); the time of death is written only when it is 0 | Matches |
| Lord retreat | None | No retreat; only the respawn rule (§12) | Matches |
| Respawn content | Beaten by the player: the leader only unless byte 83; beaten by the AI: the whole record, its dead raised | Byte 83 only when the player beat it; beaten by the AI → whole army (§12) | Matches |
| Beaten mark | One "beaten by" mark, the last winner's; cleared at a respawn and at an activation; an army that never respawns stays destroyed off the map, where an activation brings it back and a deactivation stops its respawn | One mark at +0x16a7, overwritten by 0x496834, cleared by 0x4a28d0 and 0x4969b8; 0x496900 clears "destroyed" (§12) | Matches |
| Respawn takeover | Village, shipyard, altar, ruins, from anyone | Village, shipyard, altar, **ruins** (§12) | Matches |
| Promotion slots | The loader moves the options as the original (`normalise_upgrade_slots`): two always in slots 1 and 3 | Two options move to slots 1 and 3 (§11) | Matches |
| Noon | At its first arrival after 12:00; its base income, its castles' and forts' stock and its linked villages'; today's income with the castles' income; feudal wages (the player's Rear Service too); others all paid | Lazily at the first arrival after noon; peasants get income too; no hiring at noon (§14) | Matches |
| Midnight | Medic 10% (with the economy's midnight), village average, every building rescored | Medic armies heal 10%; armies rescore buildings (§14) | Matches |
| Ships | An army placed on water (not a bridge) is a ship for good; the same AI on the SHIP map | Same AI on the SHIP map (§13) | Matches |
| Arrival order | Each tick (a hero's whole step, or a wait tick) the armies bank and step by the step clock's play time; every arrival at the end of its play time, arrivals in time order (army order at the same moment), a midnight among them at its moment (`Game::ai_move`) | Frame by frame (world.md §5): at most one arrival per call, the next step at the next frame, the frame rate deciding ties | Matches the limit of short frames; frame effects of the original are not modelled |
| Step in place after the load | the step after is priced south of the army until its first arrival (`AiMind::stand_facing`); a respawn keeps the last direction | the cell along +0x1710: 5 (south) from the load, the path's or none after each arrival or plan | Matches |
| Idle draws at the hero's stop | Every army it steers with a next step on its path (the original's direction below 8), a patrol radius above 0 and no building under it draws `Random(3000)` once per stop, after the windows the stop opened (`Game::armies_snap`) | 0x4ad8a0 (world.md §2.2.1) | Matches |
| Contact with the player before he moves | Acted on only after his step | Not before his first step (§8.1) | Matches |

Left out for now: what an army beaten in its own arrival then does to the hero (an attack
or a greeting) is dropped, where the original would open a battle or a meeting with the
beaten army; the Community's mana bill taken from the player's mana at every AI noon, and its
short-mana flag (economy.md §1); the frame effects of the step clock (world.md §5: a step
lost to coarse frames, two arrivals sharing a frame); an AI army's attack on the hero comes
after his whole step, where the original stops him in the frame of the arrival (on screen
the step's window now plays out first, the attacker seen arriving, `Game::tick_shown`). An AI
army's noon takes its castles'
and forts' gold stock, which Razdor's economy grows only for villages so far (economy.md §3,
"Stock growth").

## Unknowns

- Whether the player's record counts as "on the map" while he is inside a building (affects
  contacts with him there). **unknown**
- What happens when more than 512 seeds are added in one plan (no bound check; a map with
  many buildings and armies could pass it). **unknown**
- The stale building defence kept when an army stands in a friendly building it does not own
  (§9.1): bug or intent. **unknown**
- The effect of the resurrection loop's extra round on the preceding army record (§9.4).
  **unknown** (0x4a66c7)
- How the XP award per surviving unit is mapped to the army slots in the battle result list
  (the offsets read do not match an entry layout cleanly); battle.md's XP rules are assumed.
  **unknown**
- Armies destroyed other than by battle or event (spells): which respawn path they take.
  **unknown**
