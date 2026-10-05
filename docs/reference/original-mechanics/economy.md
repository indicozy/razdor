# Discord Times: economy and buildings (from the Community exe)

Source: the user's `DiscordTimes.exe` (Community Update, Unstable), static reading only.
The rules are in my own words. Addresses are given so each rule can be checked.
Confidence tags: **code** = read in the code, **data** = consistent with the data files or
footage but not traced, **unknown** = not known.

Spells, items and the event engine have their own files now (§4–§6 point to them).

## 0. Conventions

- **Rounding** (code, 0x402dd0): `Round` is round-half-to-even (2.5 → 2, 3.5 → 4). It is applied to
  the x87 result, so whether a "half" really is a half depends on the constants and on the x87
  precision setting (see the relation factor, §2). Integer division (`div`) truncates towards 0.
- **x87 precision** (engine.md §1): the RTL default is 64-bit mantissas (0x1332), and the
  DirectDraw set-up asks DirectX to put the x87 unit in single precision (24-bit mantissas). The
  running game (Wine 11) computes with 64-bit mantissas: the relation factor's halves (§2) come
  out as the 64-bit product gives them. This only matters for results that land on or next to a
  half.
- **Game time** (code): kept in 1/100 minutes (`0x68dcb8`). "now" means that value /100 plus an
  offset (`0x68dcbc`), in minutes. A day is 1440 minutes.
- **Unit HP** (code): −1 means unhurt, 0 means dead, a positive value is a wounded unit.
- **Difficulty factor F** (code, 0x4b8a10): the option `OptValue10` ("impossible difficulty",
  `RUS_DiscordTimes.ini [Options]`) set to 1 gives F = 100; anything else gives F = 120. The user's
  ini has it on. F scales the player's heal and resurrection prices (÷F/100), his item sales
  (×F/100), his castle and fort income (×F/100) and his battle XP (experience.md §3).
- **Two random generators** (code). Every economy roll below uses the game's own LCG `Rand(n)`
  (0x4832fc): seed × 0x343fd + 0x269ec3, result `((seed >> 16) & 0x7fff) mod n`; `Rand(0)` returns 0
  but still advances the seed. The seed is set to 1 when a map loads (in 0x4b2504). Delphi's
  `Random` (0x403278) is not used by the economy.
- **Unit natures** (code, 0x48f3de): 0 ordinary (also the value `People`), 1 Undead, 2 Elemental,
  3 Rogue, 4 Animal, 5 Hero. The shipped unit file uses only ordinary, Rogue and Undead, so **no
  shipped unit is an Elemental** (data): the Community "paid in mana" rules below never trigger
  with the shipped data, **except the mana-short flag** (§1 steps 8–9), which depends on the
  player's mana and not on Elementals, and does change who counts as paid.
- **Item types** (code, 0x4990cc): 0 BlowWeapon, 1 ShotWeapon, 2 Armor, 3 Helm, 4 Shield, 5 Staff,
  6 Amulet, 7 Ring, 8 Potion (also the default), 9 Item. Items also carry a magic school (1 Life,
  2 Elemental, 3 Death), used by the market rules.
- **Army style** (code, army +0x16b7): 0 feudal, 1 rogue, 2 peasant. The player is style 0.
- The hero's class is kept 0-based at `0x68dccc` (0 Knight, 1 Archmage, 2 Ranger).
- Global settings come from `_Global.ini [GlobalOptions]` (loader 0x4e45bf, 0x4e4678, 0x4e49de).
  Two are stored converted (code): `HealingConst` as the double 100 / HealingConst (0x4ed4bc), and
  `ResurectConst` as the double ResurectConst / 100 (0x4ed4c8). `ShipCost` (`[Costs]`) is read from
  the ini each time it is needed (0x4bbc84, 0x4c60d0).

---

## 1. Wages

**Hiring kind** (code, AddUnit 0x495ce0 stores it at unit+0x19d):
- **Kind 1 (recruit)**: everything the player hires in a barracks (0x4b0fab), the starting army,
  map-load garrisons (0x4b52ed) and AI troops at load, and AI hires in a building the AI owns.
- **Kind 2 (mercenary)**: only an AI army hiring in a building it does not own (0x4a6ac5).
- **Kind 0**: the hero and the AI army leaders (0x4b4383, 0x4b46ae). Never paid.
- **Kind 3**: units added by an event (0x4a94b3). Never paid.
- A unit moved between an army and a garrison keeps its whole record, kind included (0x4b1050).
- The kind does not depend on Nature.

**Wage of one unit** (code, 0x4a163c, 0x4a1857). C is the unit type's `Cost`.
- Kind 1: `Round(C / CostRecrutDiv × f)`, f = 0.25 when C ≤ 50, 0.5 when C ≤ 100, 0.75 when
  C ≤ 150, 1 otherwise. With the default divisor 2: Cost 50 → 6, 100 → 25, 150 → 56, 151 → 76
  (75.5 goes to the even 76), 280 → 140.
- Kind 2: `C div CostMercenaryDiv`.
- **The army's wage bill** (army +0x16e0) is rebuilt by the army-totals routine (0x4a16d4, called
  after every change to an army). **Dead units are not billed** (code, 0x4a184a): a corpse goes into
  the dead totals and the resurrection bill instead.
- Community (code, 0xc25e97, 0xc25ec5): the wage of a **kind-1 Elemental** goes into a separate mana
  bill instead of the gold bill. The mana bill is one global value (0xc25e93), overwritten whenever
  any army's totals are rebuilt. The hook works through a one-shot flag (0xc25e92): every living
  unit whose current Nature is Elemental raises it, and only the next **kind-1** wage clears it.
  So a kind-2 (or kind-0/3) Elemental pays gold itself but sends the wage of the **next kind-1
  unit** (whatever its Nature, later in this army or in the next army whose totals are rebuilt)
  to the mana bill. Dormant with the shipped data.

**Noon payment** (code, 0x4a41d8). The player's noon runs from the event scan (0x4abfbc) once
nothing fires, `now` has reached the next noon and no world spell is being cast. An AI army's noon
runs on its first arrival on a new cell after noon (0x4a5534). In order:
1. Next noon = the next day's 12:00.
2. Today's income starts at the army's base income (+0x16dc).
3. For every building, in file order:
   - a **castle or fort** (types 3, 4) owned by this army gives its **gold stock** (§3), times F/100
     truncated for the player; the stock is reset to 0. **Towns give nothing** at noon.
   - a **village** whose linked building (byte 293) is owned by this army gives its gold stock,
     which is reset. Its mana stock is not touched.
4. Gold += all of that.
5. The stored "income" (+0x16e4, used by the noon report and the AI) is then rewritten as base +
   the castles' and forts' nominal `income` fields + the village stocks, not what was paid.
6. An army of style 1 or 2 pays no wages: every unit is marked paid, last paid = now. Done.
7. Otherwise the gold bill is deducted, scaled by **Rear Service** (Community, 0xc250e4/0xc25001):
   when any of the *player's* 12 unit slots has bonus 16 (`AddPayment`, flag set in the strength
   code at 0xc2502b), the deduction is `bill × 178 div 256` if the *player's* stored income is not 0,
   and `bill × 78 div 256` if it is 0. The flag and the income test read the player even while an AI
   army is paid, so the player's Rear Service also cuts AI wages (reproduce as written).
8. Community: the global mana bill is taken from **the player's mana**, for every army's noon
   (0xc25eef). If the player's mana is then **0 or below**, it is set to 0 and a "mana short" flag is
   raised. This test runs even when the mana bill is 0, so with the shipped data the flag goes up
   at any noon (the player's or an AI army's) where the player has no mana. Nothing resets the
   flag at map or save load.
9. If gold is not negative (0x4a463b, hook 0xc25f7d): every unit gets last paid = now. Without the
   flag, every unit is also marked paid. **With the flag up, nobody is marked paid**: Elementals
   are marked unpaid and every other unit **keeps its previous paid mark**, so units left unpaid
   by an earlier short-gold noon stay unpaid (they cannot act in battle) although their wages were
   paid; their last paid is refreshed, so they do not desert. **This path does not clear the
   flag** (the clearing code at 0xc25fd9 is unreachable), so the flag stays up, for every army,
   until some army's short-gold noon clears it.
10. If gold is negative (0x4a4401): every unit is marked paid, then repeatedly:
    - pick the paid unit of kind 1 or 2, not an Elemental, with the **lowest full wage** (no Rear
      Service; the earliest slot wins a tie); corpses take part although they were not billed;
    - (the first time) if the mana-short flag is up, Elementals in slots 1–11 are marked unpaid and
      the flag is cleared;
    - mark the picked unit unpaid and give its wage back;
    - stop when gold ≥ 0.
    Then every unit still marked paid gets last paid = now, **gold is set to 0**, and every unit
    (dead or alive) whose `last paid + MaxTimeNotUpkeep < now` **leaves the army** (0x4a4608 →
    0x4965b0). Desertion is checked only on such short noons. The hero is never a candidate, so he
    is always paid and never leaves.
- Edge (code): if gold stays negative with no candidate left, the loop adds 100000 and stops; then
  gold is set to 0. This cannot happen in normal play.
- **Unpaid units** (flag unit+0x1a5 = 0) do not act in an interactive battle unless their side's
  override flag is set (code, Unit_CanFight 0x497588; flags 0x669df4 for the player, 0x669df5 for
  the enemy). The battle window sets them when it opens (0x4d211a): when the hero walked into
  the enemy, the player's flag is 0 and the enemy's 1; otherwise (an AI army attacked the hero,
  or an event started the battle) the player's is 1 and the enemy's 0. So the **attacker's**
  unpaid units sit out and the defender's fight (battle.md §9).
- **Garrisons are never paid** (code): the noon routine runs only for the player and the AI armies.

**Player's noon sequence** (code, 0x4abfbc, 0x4ac427):
1. Ranger hero: every wounded unit heals 15% of its max HP (truncated; above max → unhurt).
2. The army totals are rebuilt.
3. If the gold bill is 0 and the owned towns, castles and forts have no nominal income
   (0x497308), the payment runs at once and the game autosaves.
4. Otherwise the **noon report** opens. When it is closed, the payment runs, and a Ranger heals
   every wounded unit **another 20%** (0x4ac6a4; the Community F4 copy at 0xc27842 does the same).

## 2. Prices, market, barracks, healing, resurrection

**Relation price factor** (code, 0x4a03ec), used for player purchases and all AI prices:
- The attitude is the building's attitude towards the buyer's faction (building +0x151 + faction);
  it counts as +3 when the buyer owns the building.
- Price = `Round(base × m)`:

  | Attitude | −3 | −2 | −1 | 0 | +1 | +2 | +3 or own |
  |---|---|---|---|---|---|---|---|
  | m | 1.7 | 1.45 | 1.25 | 1.1 | 1 (no rounding) | 0.9 | 0.75 |

  An attitude outside −3..3 leaves the price unchanged.
- **Halves.** The code is `fild base`, `fmul` by the constant, then `Round` (0x402dd0:
  `fistp`). The constants: 1.25 and 0.75 are single floats (exact), 1.7, 1.45, 1.1 and 0.9 the
  double values widened to 80 bits (0x4a0524, 0x4a0518, 0x4a0508, 0x4a04fc), so slightly off
  (1.7 and 1.45 a little low, 1.1 and 0.9 a little high). The game's own control word is
  Delphi's Default8087CW 0x1332 (64-bit mantissas, round to nearest even), loaded at the start
  and again by the RTL's FPU init (0x403984: `fninit`, `fldcw [0x4e8024]`, called from six
  RTL sites); its other `fldcw` sites only save and restore around conversions. With 64-bit
  mantissas the product keeps the constant's error, and a price that would be exactly x.5 in
  decimal rounds **down** for 1.7 and 1.45 (15 → 25, 30 → 43), **up** for 1.1 and 0.9
  (15 → 17, 5 → 5, 75 → 83), and to even for 1.25 and 0.75; negative bases symmetrically.
  This is what the running game does (Wine 11: Проклятое озеро's church at attitude 0
  charged 83 for an item of Cost 75, FINDINGS.md §24), and Razdor follows it. Under single
  precision (what a Direct3D 7 device created without "FPU preserve" would set, §0) every
  half would go to even instead (82); not seen in the running game.

**Player market** (code, 0x4b9e18):
- **Buying**: price = relation factor on the item's `Cost` (the sign is taken off the goods id, not
  off the Cost: a map-placed fixed good with a negative Cost would get a negative price and pay
  the buyer; random goods never have a negative Cost); a **Merchant** (bonus 5 on any unit
  of the player's army, checked when the building window opens, 0x4bbc84) takes off
  `price × 30 div 100`. Bought only if price ≤ gold. The bought item goes to the pack (no pack
  space test here) and onto the sell list; its market slot is emptied (written back to the
  building when the window closes).
- **Selling**: `Cost × ItemSaleCost × F div 10000`; a Merchant adds half of that again (truncated).
  The relation does not count. The sell list holds only pack items with **Cost > 1**, so personal
  items (negative price) and the cheapest ones cannot be sold.
- **What the market shows** (code, Market_FillLists 0x4bca8c, row handler 0x4b9c40, icons
  0x4bddf0):
  - When the window opens (0x4bbc84) the shop's 12 places are packed into a list with the empty
    places left out (fixed goods by their absolute id), and the sell list is built from the pack
    (Cost > 1). The market tab opens on the **buy** list when the shop has goods, else on the
    sell list (0x4ba854).
  - Each row shows the item name and a price, computed **exactly as the purchase or sale charges
    it**: the buy price is the relation factor on the item's Cost, less the Merchant's 30%; the
    sell price is the formula above, plus the Merchant's half. A buy price above the player's gold
    is drawn in red, others in the normal colour; sell prices are never red. Seven item icons
    run beside the visible rows. The player's gold is shown under the lists.
  - Selecting a row shows the item's full description (ItemText_Build 0x499eb0) and its picture.
    On the sell list the Sell button is always enabled; on the buy list the Buy button is enabled
    only when the shown price is at most the gold, the same test the purchase makes. So the shown
    price, the red colour, the button and the charge always agree.
  - After a sale or purchase the list is rebuilt and the previous row stays selected (the last
    row when the list got shorter). The switch to the buy list is offered only while the shop has
    goods.
- **Spells**: exactly `CostGold`, if CostGold ≤ gold and the spell is not known (0x4ba3b0); the
  book limit is the shop's (magic-items.md).
- **Barracks hire**: exactly the unit's `Cost` (0x4b0fcf), no relation factor, no Merchant. The
  slot count drops by 1 and the unit joins as kind 1. Community: an Elemental is hired for its Cost
  in mana (0xc25dbd); the barracks list compares the price with mana for it (0xc25e38).
  - The Hire button is enabled only when the price check passed and the army has fewer than 12
    units (0x4957e9). Its release handler (0x4c7380) finds the cell the recruit will take by
    adding a placeholder unit and removing it again at once (AddUnit 0x495ce0, then
    Army_RemoveUnit 0x4965b0: no lasting effect), then starts a card-slide animation
    (0x4b0c04). The slot count, the gold and the new unit change only **when the slide ends**
    (0x4b0fab). The new unit is paid with last paid = now (AddUnit) and its garrison stamp
    (unit+0x1bb) is set to 0 (code).
- **Ship**: `ShipCost` gold, read when the window opens (Buy enabled if ShipCost ≤ gold and the hero
  is not at sea) and read again when paying (0x4c60ac).
- Gold changes go through one routine (0x4ab150) that clamps gold at 0. It also carries an
  anti-cheat seal: if gold no longer matches its seal, the hero becomes unit 59 at level 1 with no
  army and 5 + Rand(20) gold (code). The noon payment writes gold directly and reseals.

**Market stock** (code, 0x4be178; candidates 0x4bdf74). Markets exist only in **towns (1), markets
(6) and churches (7)**: map load wipes the goods of every other building type, altars included
(0x4b5600 area).
- At map load the map's goods become **fixed goods** (kept as negative ids); a restock timer is
  set (to 1) if there are fixed goods or random goods. With random goods the maximum price is capped at
  the dearest item's Cost (0 means that cap), the minimum is set to 0 if it is not below the
  maximum, and the market is stocked at once.
- The market tab exists only while the timer is set (0x4bbc84).
- **When**: every midnight the routine runs for every building (0x4a19e8); it acts only when the
  timer is set and due. After acting it sets the timer to now + 12 h (when the building has random
  goods), so a market with random goods is redrawn **every midnight**. A market with only fixed
  goods keeps them until bought; its timer stays at 1, so the routine still runs every midnight,
  and a **town** with no random goods and 1–4 fixed goods gets one new healing potion each
  midnight (step 3 edge; the previous one is dropped as a random good).
- **The first midnight**: the load's stocking passes the clock as `time div 100 + start`
  (0x4b5549), where `start` is the header's start minute **+ 1**, so its timer is the start
  + 721; a midnight passes the minute of the AI driver's frame in which it comes (0x4a1998).
  On a map that starts at noon the timer falls one minute after the first midnight: the market
  restocks there when that frame's minute is past the midnight's, and not when it is the
  midnight's own minute. The length of the frame decides (FINDINGS.md §5); from the second
  midnight on the timer is always due.
- **Steps**:
  1. Random goods (positive ids) are dropped. R = byte 295 minus the fixed goods still there.
  2. Price window: MX = min(word 335, 5000), plus 1 with chance 1/5 (`Rand(5) = 0`); MN = max(word
     333, 5), lowered to MX if above it.
  3. **Towns** first: n = R div 5 + 1 potions, and R −= n. If R is then still above 6, one of
     the n is item **95, 96, 97, 114 or 115** (`Rand(5)`, equally likely). The others are healing
     potions **98 + Rand(3)** (98, 99 or 100). These do not use the price window, and they are
     written even when all 12 places are full (into the 12th). Edge: R can be 0 or negative here
     (random count used up by fixed goods, or 0); R from −4 to 0 still gives n = 1, one healing
     potion.
  4. Each of the R remaining slots i = 1..R draws from a **price band** that walks down the window:
     with A = (MX − MN)/180 + 1 and t = (R − i)/(R − 1), q = t / (A − (A − 1)·t) (square-rooted in
     a church); the band's lower edge is `MN + Round((MX − MN)·q/2)` (at least MN) and its upper edge
     is the previous band's lower edge (MX for the first band). Exception: if the previous slot's
     list ran down (step 7), the upper edge is instead `MN + Round((MX − MN)·(1 + q)/2)`. If both
     edges are equal the lower one becomes `Round(lower × 0.8)`. With R = 1 the band is the whole
     window. So the first slot draws from the top half and later slots from lower bands, bunched
     towards the top when the window is wide.
  5. Candidates are items whose |Cost| lies in the band, and:
     - towns and markets: no Potion or Item types; churches: only Amulets and Potions; other
       buildings: no Item type;
     - never a negative Cost; never a Death-school item in a town or church; never, in a market,
       an Amulet costing exactly 150.
  6. If there are no candidates, or with chance 1/(number of candidates) (`Rand(n) = 0`), the band
     widens (upper ×1.2, lower ×0.8, rounded; kept inside [MN, MX] when R > 1) and the list is
     rebuilt. A list of exactly one candidate is therefore never accepted: if fewer than two items
     can ever fit (inside [MN, MX] when R > 1), this loop never ends (a hang in the original).
  7. A candidate is refused if the item is already stocked twice, or once when word 335 (uncapped)
     is above 500; refused ones leave the list. If the list runs down to one, the band widens once
     more and the list is rebuilt. After 26 rounds the last item drawn is stocked anyway.
  8. The item goes into the first empty place. If all 12 places are full the slot is skipped
     (nothing is overwritten), but steps 4–6 have already run and used their random rolls.
- Finally ids outside the item range are cleared. **The list is not sorted**: the Community exe
  jumps over the sort loop (0x4be9a0), so goods stay in the order drawn.

**Barracks regrowth** (code, 0x4a1998, every midnight, buildings with byte 294 set):
- Each of the 6 slots with a unit, a max above 0 and count < max gains +1 when
  `Rand(MaxDayCountForNewUnit div max) = 0`. A divisor of 0 or 1 always gains.
- With the default 10: max 5 → 1/2 a day, max 3 → 1/3, max 2 → 1/5, max 1 → 1/10, max 6–10 → every day.

**Healing** (code, button 0x494d63, action 0x4b12ef):
- Offered on each wounded unit (0 < HP < max) of the player's army **in the hire tab**, so in any
  building whose hire tab is shown (§7).
- Price = `Round((max − HP)/max × Cost × HealingConst/100 × 100/F)`, and 1 if that rounds to 0.
  Paid at once; the unit is healed to full. **No game time passes** for the player
  (`HealingTime` is the AI's).
- Community: an Elemental pays in mana (0xc2609e), but the heal button still compares the price
  with gold (only the resurrect button switches to mana for an Elemental, 0xc260c5/0xc260de).

**Resurrection** (code, 0x494cfa, 0x4b1389):
- Offered on a dead unit in the hire tab when the building is a **town (1) or church (7)**.
- Price = `Round(Cost × ResurectConst/100 × 100/F)`, no minimum. The unit comes back at full HP and
  is marked paid (its last paid time is not changed).
- **No time limit for the player**: `MaxTimeResurection` is read only by the AI (0x4a55ca). The
  player's corpses stay in the army until resurrected or buried (Bury is the Dismiss action on a
  dead unit, below), or until a short-gold noon makes them desert (§1 step 10).
- Community bug (code, 0xc2606c): the "pay in mana" test at resurrection reads the low byte of the
  unit's **Cost** instead of its Nature. Any unit whose Cost is 2 more than a multiple of 256 is
  resurrected for **mana** (clamped at 0) while the button checked gold. In the shipped data that is
  unit 56 (Cost 2050, Undead).

**Daily heal effects** (code):
- Garrisons: every wounded garrison unit +`GarrisonAutoHeal`% of max HP at midnight (0x4a1ce5).
- **Medic** (bonus 4 on any unit of an army, dead or alive): every wounded unit +10% of max HP at
  midnight, for every army on the map, the player's included (0x4a1dca).
- **Ranger hero**: +15% at noon, and +20% more when the noon report is shown (§1).
- All heals truncate (`max × p div 100`); a unit that goes above max becomes unhurt.

**Garrison exchange** (code, 0x4ba854 tab 2, 0x4c653c, 0x4acff4):
- Opening the tab stamps every unit of the hero's army with "now" (unit+0x1bb).
- A garrison unit counts as **paid** when it was last paid less than 1441 minutes ago, or when
  1440 minutes or more have passed since that stamp. Units placed at map load have no stamp, so
  they are always paid.
- The paid mark of garrison units is set **only when the tab opens**. A unit put into the
  garrison during the visit keeps the mark it had in the army.
- Taking a paid unit is free (its last paid becomes now). Taking an unpaid one costs **one day's
  kind-1 wage** of its type (`RecruitWage(Cost)`), asked only if that price is strictly below the
  gold; paying marks it paid with last paid = now. So an unpaid unit cannot be "laundered" by
  parking it for less than a day. The question is asked **only when the unit goes to an empty
  cell** of the hero's grid; a swap skips it (below).

**Garrison moves** (code, hero grid click 0x4c653c, garrison grid click 0x4c6f50, slide
0x4b0c04 from 0x4b0ff5):
- Both grids have 12 cells. One click selects a unit (in either grid); clicking it again
  deselects it. The second click acts, then the selection is cleared; a refused move (below) does
  nothing at all and the selection stays. Every swap and slide plays `Card-Move`. Clicks are
  ignored while a slide animation runs. The garrison grid reacts only in the garrison tab; the
  hero grid follows the same rules within the hero's army in every tab that shows it.
- **Same grid**: clicking another unit swaps the two places; clicking an empty cell slides the
  unit there. Only the formation changes.
- **Hero's army → empty garrison cell**: the unit slides into the garrison. The **hero** (unit 1)
  and **named units** (personal name, unit+0x14 > 0) are refused; nothing happens.
- **Garrison → empty cell of the hero's grid**: free when the unit is paid, else the purchase
  question above. An empty cell means the army has fewer than 12 units, so a full army cannot
  take a unit this way.
- **Swap between the grids** (a unit of one grid selected, a unit of the other clicked): the two
  unit records are exchanged in place, each taking the other's cell. The army-side unit must not
  be the hero or a named unit; the garrison-side unit can be anything. There is **no purchase
  question and no price**, so an unpaid garrison unit enters the army for free, still marked
  unpaid. Only when the garrison unit was selected first (click on the army unit) does it get
  last paid = now, and only if it is paid; the other click order refreshes nothing. This is also
  the only way to bring a garrison unit into a full army.
- The whole unit record moves: type, level, XP, HP, worn items, hiring kind, paid mark and last
  paid. **Corpses** can be moved both ways (no HP test). Each move between the grids rebuilds the
  army totals and redraws the status bar (Army_Recalc 0x497240, 0x49d224).

**Dismiss and Bury** (code, army window: button handler 0x4c3744, confirm 0x4c3808, cancel
0x4c3828, action kind 2 of 0x4b11cc at 0x4b1778):
- Only in the army window (not the building window). The button is drawn under the selected
  unit's card for every unit **except the hero** (0x495988); its caption is Bury for a dead unit
  and Dismiss otherwise. There is no other test: named units, event units (kind 3) and the
  army's last unit besides the hero can all be dismissed.
- Pressing it replaces it with a confirm and a cancel button: **one confirmation step**. Cancel
  restores the panel. Confirm plays a 350 ms effect, then removes the unit (Army_RemoveUnit
  0x4965b0), selects the hero, redraws the 12 cards, switches the inventory to the pack view and
  rebuilds the army totals and the status bar.
- **No refund and no cost**: gold and mana are not touched. The wage bill drops through the
  rebuilt totals.
- **Worn items are lost**: Army_RemoveUnit only closes the gap in the unit list and the
  formation; it does not move the 4 worn items to the pack (unlike the event path, which calls
  the pack routine first, events.md). The pack is not changed.
- Bury and Dismiss are the same action; only the caption differs.

## 3. Villages, buildings, loot

**Stock growth** (code, 0x4a1998, every midnight, **every building** whose max is above 0):
- Gold: `stock += Round(income × √(1 − stock/max))`, then capped at max (words 282, 284, 286).
- Mana: the same with bytes 350 (income), 351 (max) and the runtime mana stock. The addition is
  done on a byte, so a sum above 255 wraps before the cap (only possible with max near 255).
- Map load: a village starts with one day's income in both stocks (0x4b55f0). **Castles, forts and
  towns start at 0**, so the first noon after the start pays nothing from them.
- Growth slows as the stock fills: 30 a day with max 90 gives 30 → 54 → 73 → 86 → 90.
- A building with income but max 0 never accumulates anything (data: 10 castles and dozens of forts
  in the shipped maps).

**Who collects** (code):
- Castles and forts: their owner, at the owner's noon (§1), ×F/100 for the player.
- Villages linked to a building the army owns: that owner, at noon, gold only.
- Villages entered by the hero: the hero (below).
- Villages entered by a feudal AI army (style 0): that army, whoever owns the village; the village's
  mana is thrown away (0x4a5e9e).
- Towns, ruins, shipyards: nobody at noon; their stock is only taken as loot (below).
- Mana stocks of castles, forts and towns are never collected.

**Entering a village** (code, 0x4bbc84). Entering an unguarded village first captures it (world.md).
Seen in the running game (diff test, memory of the building record): the hero's step into a
neutral village sets its owner to the player (ДС1 village 13, Проклятое озеро villages 2 and 30),
and one an AI army took sets it back (РК1, `rk1-village-taken.jsonl`: army 9 takes the hero's
start village 6 at 13:00 with its whole stock; the hero walks in at 18:30 the same day, the
owner becomes the player again and the village window pays nothing). So the tribute is only
what is in stock: an army that came first the same day leaves nothing until the midnight
refill. Razdor: the same (`rk1_a_village_emptied_by_an_army_pays_the_hero_nothing_that_day`).
The events are scanned first; one that opens its window keeps the village pending (0x4ed42c)
until it is read, and a walk that ends in the village as an event opens enters it only after
that window (world.md §7.2): the offer rolls below come after the event's OK.
- A hero of Nature Rogue gets nothing: no offer, no window.
- Otherwise, if the village has gold in stock and it is not the village of the last offer (0x671d0c),
  the offer chooser runs (below). If it offers something, the offer opens.
- Otherwise the **village window** opens. Taking, and also closing the window, gives **all the gold
  and all the mana**, with no F factor and no attitude test, and clears "the village of the last
  offer" (0x4c6000).

**The 5 alternatives** (code, chooser 0x4bba40, offer 0x4aca80, finish 0x4ab966):
- Rolls in this order; the first that passes is offered. "Last" is one global value for all
  villages (0x671d14):
  1. **5 innkeeper**: `Rand(2) = 0`; unpaid units ≥ army size div 2; gold − wage bill + stored
     income < 0; last ≠ 5.
  2. **2 priest**: `Rand(3) = 0`; total missing HP of the living > 50; living units (HP above 0,
     wounded or not) ≥ army size div 2; last ≠ 2.
  3. **1 long blessing**: `Rand(6) = 0`; number of own-army spells in effect on the units ≤ army
     size; last ≠ 1.
  4. **3 furs**: (only if 3 failed) `Rand(6) = 0`; fewer than 25 pack entries; at most 2 furs in
     the pack; last ≠ 3.
  5. **4 witch**: (only if 4 failed) `Rand(6) = 0`; mana < gold; more than 2 spells known; no
     own-army spell in effect; last ≠ 4.
  - "Last" becomes the result, **or "none" when nothing was offered**, so a kind can come back
    after an empty visit. Army size counts the dead.
- What each gives: 1 casts spell 3 + 2·Rand(5) (3, 5, 7, 9 or 11) with the long event duration;
  2 casts spell 1; 3 gives item 135 (furs; Cost 1000, so it sells for 250 with F = 100); 4 gives
  300 + 50·Rand(5) mana; 5 marks every unit paid with last paid = now. **Both `Rand(5)` are drawn
  when the offer is made**, as its question is built (0x4aca80: 0x4acb89, 0x4acd76), right after
  the chooser's rolls and before the event window's chord; the answer draws nothing. The
  blessing's spell is 3 + 2·Rand(5) whatever spells the install has.
- **Every option is a Yes/No question.** Yes applies it and **empties both stocks without paying
  them**. The offer's event record (0x4aca80) carries a result message for 3, 4 and 5
  (`VillageBonus3/4/5Result`), none for 1 and 2: so a Yes to the furs, the witch or the
  innkeeper opens the event window again with that message (0x4c2100: its chord is drawn,
  its OK finishes the offer, 0x4ab966), while a Yes to the blessing or the priest finishes at
  once with no window. No re-enters the village, which (being the village of the last offer) opens the plain
  window, where the stocks can be taken.

**Rumours** (unknown): no fixed rumour price exists in the code. Rumours are events, so their cost
is the event's gold result (events.md).

**Loot after the player's win** (code, 0x4c50ec; paid when the victory window closes):
- **An army**:
  - Gold = `enemy gold div VictoryGoldDiv`. `MinVictoryGold` is not used.
  - Plus the enemy's wage bill when its byte +0x3822 is 0 and its style is below 2 (not peasants).
    The bill is the record's +0x16e0 as its last recount (0x4a16d4) left it: the player's battle
    recounts neither side (its write-back, 0x4988c0, copies HP only), so a gang he wipes out pays
    the wages of the units it had at its last arrival in a building, AI battle, respawn or the map
    load. Checked on Проклятое озеро (diff test, `lake-gang.jsonl`): army 17, 150 gold, two
    robbers (Cost 70) and a chieftainess (130) behind its leader, pays 75 + 85 (18 + 18 + 49; the
    leader draws no wage). +0x3822 is the map's byte 62, "units carry no money".
  - Every item the enemy units wore and its 12-item pack go to the loot list (32 entries; a larger
    haul would overrun the loot gold and mana that follow it in memory).
  - If the building the beaten army stood in (+0x3788) is a castle or fort with an empty garrison,
    it becomes the player's (faction and attitudes copied from him).
- **A building's garrison** (castle, fort, town, ruins):
  - Gold = the building's gold stock + the garrison's gold + **one day's income** (word 282); no
    division. The stock is reset.
  - The building becomes the player's; faction and attitudes are copied from him.
  - Items: as for an army, every item the garrison's units wore (unit by unit, slot by slot),
    then its 12-item pack (0x4c50ec reads the beaten record whichever it is).
  - Ruins: at load their treasure gold (word 335) becomes the garrison's gold, and their **first 5**
    goods become garrison items (0x4b554e): with garrison units, each good in turn goes to the
    unit whose tactical value (mode 2) it raises most, the first of equals, else into the pack
    (0x4a273c, as an AI army's starting items); with no units, all five go straight into the
    pack. So the garrison fights wearing them, and both come back through this rule. The
    building's own goods words are not cleared (ruins keep them as the map has them).
- Mana: the battle code's value (0x66ae44); not traced here (battle owner).

**AI-vs-AI loot** (code, 0x4a4c68):
- Only a winner of style 0 or 1 takes anything.
- **Defender wins**: it takes the attacker's wage bill if the attacker is style 0; then all the
  attacker's gold if that is below `MinVictoryGold`, else `gold div VictoryGoldDiv`.
- **Attacker wins against an army**: the defender's wage bill if the defender's style is below 2,
  plus `gold div VictoryGoldDiv`, **with no minimum rule**.
- **Attacker wins against a town, castle or fort garrison**: income + garrison gold + stock (stock
  reset). Against any other garrison (ruins): only `garrison gold div VictoryGoldDiv`.
- The loser keeps the rest of its gold. Items go to a pool shared out to the side with more survivors.

**Other gold sources** (code): an AI army that respawns gets `(delay div 1440) × base income`
(0x4a28d0). Event results and campaign carry-over are events.md.

## 4. Spells

Moved to **magic-items.md** (record layout, learning, casting cost and time, duration, targets,
instant effects, potions). Economy keeps only the prices: a spell costs exactly `CostGold` (§2);
the village blessing and priest cast spells 3/5/7/9/11 and 1 (§3).

## 5. Items

Moved to **magic-items.md** (stat order, wear rules, pack). Economy keeps: Cost > 1 to be sold, a
negative Cost marks a personal item (§2), the market type rules (§2).

## 6. Event engine

Moved to **events.md** (order of checks, windows, answers, results, campaign carry-over). Economy
keeps: event gold and mana results go through the clamped gold routine and are clamped at 0;
units added by events are kind 3 and never paid; the three spare event slots after the last event
are the noon report, the village offer and the garrison unit purchase.

## 7. Building types and their tabs

Building types (byte 6): 0 palace, 1 town, 2 village, 3 castle, 4 fort, 5 tavern, 6 market,
7 church, 8 smithy, 9 shipyard, 10 altar, 11 dungeon, 12 ruins, 13 stone bridge, 14 wooden bridge,
15 obelisk.

Entering a building first runs the event scan (events.md). Then (code, 0x4bbc84):
- **Bridges and the obelisk** (13–15): no window.
- **Village** (2): the offer or the village window (§3).
- **Shipyard** (9): the ship window when the hero is on land (§2).
- **Every other type** opens the building window with these tabs (no attitude test anywhere):
  - **Info**: always (description, quests and rumours).
  - **Hire**: when some barracks slot holds a unit and either every slot unit is of ordinary
    Nature or the building's "all types" byte (356) is set. Healing and resurrection live here.
  - **Garrison**: the player's own castle or fort (towns have a garrison record but no tab).
  - **Market**: while the restock timer is set (towns, markets, churches; §2).
  - **Spells**: when any of the six spell bytes (308..313) names a valid spell.
- Garrison records exist for towns, castles, forts and ruins (0x4b5291); map-load garrison units
  skip unit ids 1–3 (the hero types) and are kind 1.
- The building marked as the start building of the chosen class becomes the player's at load.
- The hire tab also shows Gold, Payment (the wage bill without Rear Service) and Income (the
  nominal income of the player's towns, castles and forts ×F/100) (0x4bd3a4).

## 8. AI spending (economy parts of the AI's arrival routine 0x4a548c)

All AI prices go through the relation factor (code). No F factor applies to the AI.
- **Spare gold** (0x4a0530): with W = (wage bill + 2 × the untiered wage sum Σ Round(Cost /
  CostRecrutDiv)) / 3, a feudal army may spend gold − NeedUpkeepDay × W + (income + daily village
  average) × (NeedUpkeepDay − 1); other styles gold − W + income + village average; kept within
  0..gold.
- **Corpses**: an AI army drops a dead unit `MaxTimeResurection` minutes after its death (checked
  on arrival).
- **Shopping** (style 0/1, attitude ≥ 0, a market (6) or church (7) for a living leader, an altar
  (10) for an undead leader — altars have no goods, so never): first it sells every pack item for
  half the relation price, then buys the best (unit, item) upgrades worth more than 5 that it can
  spare gold for (ai.md §9.3). It buys at all only when the gate AI_CanAffordMarket (0x4a0618)
  passes (code): the spare gold is above 0 and the **cheapest** of the shop's 12 goods (positive
  ids only, so map-placed fixed goods are ignored), priced as the relation factor on its absolute
  Cost, is at most the spare gold.
- **Garrison buying and reshuffle** in its own town, castle or fort (0x4a70e7..0x4a79ad): see
  ai.md §9.6 (code there).
- **Healing** (style 0/1, attitude ≥ 0 or own building, barracks byte set, a barracks unit of the
  leader's kind (undead or not)): per wounded unit, `Round(Cost × HealingConst/100 × HP/max)` with
  the unit's **current** HP, through the relation factor, if below its gold; the unit is healed and
  the army is busy for `HealingTime` minutes.
- **Resurrection** (town or church): the most valuable dead unit first, `Round(Cost ×
  ResurectConst/100)` through the relation factor, while affordable; busy as above.
- **Hiring**: by battle role (the role with the lowest total value first), the slot unit's Nature
  must match the leader's (leader type 74 may hire anything not undead), price relation(Cost) ≤
  spare gold, up to 12 units (8 when filling the third role); kind 1 in its own building, kind 2
  elsewhere; optional XP top-up from the player's average (ai.md).

## 9. Reports and displays

- **Noon report** (event slot N+1, 0x4a9b75): gold, mana, the nominal income of the player's towns,
  castles and forts ×F/100, and minus the wage bill (without Rear Service). It warns when gold +
  income < wages. The gold actually received at noon differs (§1: stocks, no towns, linked villages).
- **Hire tab**: the same three figures (§7).

---

## Razdor now → original

Razdor's code as read for this pass: `src/rules/economy.rs`, `town.rs`, `world.rs`, `game.rs`,
`ai.rs`, `ui/building_view.rs`, `ui/game_bar.rs`, `ui/items_view.rs`.

| Topic | Razdor now | Original (this file) | Match |
|---|---|---|---|
| Wage kinds | Hero and AI leaders free, event units free, the rest recruits; AI hires kind 2 in foreign buildings | Same for the player; AI kind 2 in foreign buildings | Yes |
| Wage formulas | Brackets and divisors as the original | §1 | Yes |
| Corpses' wages | Not billed; may be picked for a refund (`Game::bills`, `pay_noon`) | Not billed; may still be picked for a refund | Matches |
| Rear Service | On the whole bill at noon, truncated once, testing the player's stored income; not in the displayed bill; also cuts AI wages (`rear_service`) | On the whole bill, truncated once; not in the displayed bill; tests the stored income; also cuts AI wages | Matches |
| Short gold | Refunds full wages, cheapest first (earliest of equals), corpses included, never Elementals | Refunds full wages, cheapest first, corpses included, never Elementals | Matches |
| Mana short | The player's mana pays every army's elemental bill; 0 or below raises the saved flag; while up, enough-gold noons leave the marks as they were (Elementals unpaid); only a short noon clears it (`Game::mana_short`) | Mana 0 **or below** at any army's noon raises the flag; it sticks across noons; while up, enough-gold noons leave every unit's paid mark as it was (Elementals unpaid); the player's mana pays every army's elemental bill | Matches |
| Desertion | Short noons, `last paid + MaxTimeNotUpkeep < now` | Same | Yes |
| Castle/fort income | Castles and forts pay their gold stock ×F/100 (truncated); towns nothing | Castles and forts pay their √-grown **stock** ×F/100; towns pay nothing | Matches |
| Building mana income | No mana at noon | No mana at noon at all | Matches |
| Linked villages | Gold only, to the owner of the linked building | Gold only | Matches |
| Stock growth | Every building with a maximum; the mana sum wraps on a byte (`grow_mana`) | Every building with a max > 0; byte wrap for mana | Matches |
| Ranger heal | 15% at noon, 20% more when the report is shown | 15%, plus 20% when the report is shown | Matches |
| Medic, garrison heal | 10% / GarrisonAutoHeal% at midnight; a dead medic counts | Same (a dead medic counts) | Matches |
| Relation factor | The x87 product of the code's constants, rounded to 64 bits then to even (`relation_price`); an attitude outside −3..3 leaves the price unchanged | Same table; halves down for 1.7/1.45, up for 1.1/0.9, to even for 1.25/0.75 (64-bit precision, §2) | Yes |
| Buy / sell / spells / hire / ship prices | As the original; a fixed good of negative Cost has a negative price that pays the buyer; a dead Merchant counts | §2 | Matches |
| Market buildings | Towns, markets, churches only: the map load drops every other building's goods | Towns, markets, churches only | Matches |
| Market stock | 12 places with the map's goods fixed in theirs; a 12-hour timer; bands walking down the window, town potions (one of 95/96/97/114/115 when more than 6 remain, the rest 98 + Rand(3)), type and school rules, 1/n widening, run-down lists, 26 tries, no overwrite, not sorted (`restock_market`); a list that runs out is read on past its end as the original (the zeroed buffer, item 1; then the last index and the building's number: `Candidates`). Where fewer than two items can ever fit, or deeper past the end, the original hangs or reads its stack; Razdor gives up on that good | Bands walking down the window, n − 1 healing potions + one of 95/96/97/114/115 when R > 6 remains, type and school rules, 1/n widening, no overwrite of a full list, not sorted | Matches |
| Barracks regrowth | `1/(10 div max)` | Same | Yes |
| Heal price | As the original, exact rational | Same formula in floating point | Yes |
| Where to rent a ship | Every shipyard, whatever its attitude: the ship window opens on land, nothing at sea, with no main hall or other tab; `rent_ship` tests only the building type, and Buy closes the window (`town::tabs`, `Game::window_at`, `Game::shipyard_here`, `building_view::ship_window`) | The ship window for every type-9 building entered on land (nothing at sea), no attitude or owner test; Buy iff ShipCost ≤ gold, and it closes the window (0x4bbc84, 0x4d3ec0, 0x4c60ac) | Matches |
| Where to heal / hire | Any building whose hire tab shows (a barracks unit, all of ordinary Nature or the all-types byte); no attitude test (`Location::hires`) | Any building whose hire tab shows (ordinary barracks or byte 356); no attitude test | Matches |
| Resurrection | Town or church, no time limit for the player; corpses stay until raised or buried by hand; raised paid, last pay kept | Town or church, **no time limit** for the player; corpses are never buried automatically (only by hand, Bury) | Matches |
| Resurrection currency bug | Cost ≡ 2 (mod 256) pays in mana, clamped at 0, after a gold check (`resurrect_price`, `can_pay_service`) | Cost ≡ 2 (mod 256) pays in mana (unit 56) | Matches |
| Garrison take-back | Unpaid units cost one day's kind-1 wage, asked only below the gold, into an empty cell; the paid marks are set when the tab opens (`open_garrison`, `take_from_garrison`) | Unpaid units cost one day's wage (into an empty cell) | Matches |
| Garrison moves | Hero and named units refused, the selection kept; `Card-Move` on swaps and moves; corpses move both ways; a cross-grid swap exchanges the records with no price, last pay refreshed only in one click order (`swap_with_garrison`) | Hero and named units refused; corpses move both ways; a cross-grid swap exchanges records with no price (unpaid stays unpaid) and is the only way into a full army | Matches |
| Dismiss / Bury | A confirm step; no refund; worn items lost; the pack untouched; only the hero protected; the hero selected after | A confirm step; no refund; **worn items are lost**; the pack is untouched; only the hero is protected | Matches |
| Hire timing | Immediate | The unit, gold and slot count change when the card slide ends | Yes (no visible difference) |
| Market display | Prices as charged; the sell list only Cost > 1; unaffordable buy prices red; Buy enabled iff price ≤ gold, no pack test; opens on the goods when there are some | Prices as charged; sell list only Cost > 1; unaffordable buy prices red; Buy enabled iff price ≤ gold, no pack test | Matches |
| Village offers | Every roll drawn until one passes, the last kind's included (it cannot pass); "last" becomes none after an empty visit; innkeeper and priest against army size div 2, the priest counting the living; all options are questions (`Game::visit_village`) | Rolls every step; "last" becomes none after an empty visit; all options are questions | Matches |
| Village tribute | No attitude test | No attitude test (entering captures the village) | Matches |
| Ruins' goods | The first 5 go to the garrison at load, worn by the unit they help most or packed (`ai::give_item_to`); a win loots the worn items, then the pack | 0x4b554e, 0x4a273c; loot 0x4c50ec | Matches |
| Village under an event's window | A walk that ends in the clicked village as an event opens enters it when the windows are read (`Game::enter_waiting_building`): offer rolls, window and tribute then; the tribute is taken as the window opens | Entered after the event's OK (0x4ed42c, 0x4bbc84); the stock is paid when the village window closes (0x4c6000) | Order matches; the tribute's moment within the window differs (no draw in between) |
| Player's loot | gold div VictoryGoldDiv + wage bill unless peasant or "no money" | Same (the "no money" byte is +0x3822) | Yes |
| AI-vs-AI loot | Threshold only when the defender wins; wage bills by style; winner style 0/1 (ai.md §10) | Threshold only when the defender wins; wage bills by style; winner must be style 0/1 | Yes |
| Castle capture gold | Stock + income (garrisons carry no gold) | Stock + garrison gold + income | Yes |
| Ruins treasure | The first 5 goods | The first 5 | Matches |
| Displays | Report and hire tab show the nominal income of towns, castles and forts ×F/100 and the bill without Rear Service; the report shows the gold before the payment and warns when gold + income < wages; no report when there is neither | Nominal incomes (towns included) and the bill without Rear Service | Matches |

## Unknowns

- The loot mana value (0x66ae44, written by the battle code).
- The exact meaning of the army bytes +0x3822 (probably "units carry no money") and +0x3825.
- The player's stored income before the first noon: the AI init (0x4a1ff0) adds his castles' and
  forts' nominal income to it, also after a save is loaded (then on top of the saved value). It
  affects the Rear Service factor and the innkeeper test until the first noon.
- How the global "last village offer" is reset when a map or save loads (0x4b58fe, 0x4b850f).
- Rumour prices (expected to be the event's own gold result).
  and the last bit of every other float formula here (engine.md).
