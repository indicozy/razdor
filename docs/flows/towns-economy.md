# Flow: buildings, towns and the economy

Read this first if you touch a building window (hall, barracks, garrison, market, sanctuary,
village tribute and offers, shipyard), prices, wages, income, or the daily midnight and noon.

## In one paragraph

A building is a `Location` in `World::locations` (`src/rules/world.rs`), with a kind
(`LocationKind`), an owner (`Owner`), an attitude to the player, stocks (gold, mana, barracks
`Recruit` slots, a market `Shop`) and maybe a garrison (`Stationed`). The hero enters it at the end
of a route (`Game::enter_building`, see `map-step.md`); `town::window_at` picks the first tab and
the UI opens `Screen::Building` (`src/ui/building_view.rs`, `BuildingView`). Each tab calls a
`Game` service in `src/rules/town.rs`, `src/rules/game.rs` or `src/rules/economy.rs`, which
checks, charges and changes the state, returning a `ServiceError`, `HireError` or `TradeError` the
UI turns into the strip's red text. Money moves at the daily moments: midnight regrows stocks and
heals garrisons (`economy_midnight`), noon pays income and wages (`pay_noon`, through
`Game::new_day`). Closing the window may run the event scan (`Game::window_closed`).

## Call chain

```
rules/game.rs  arrive_at_end() → enter_building(l, scan)   events first when just arrived
               → Event::Arrived(l)                          bridges and obelisks open nothing
ui/world_view.rs handle_events → Screen::Building(l)
ui/building_view.rs BuildingView                            tabs = town::tabs_here()
  Main hall  → Game::hall_entries(), take_hall_entry(id)    quests/rumours: events.rs take()
  Barracks   → Game::recruits_here(), hire_price(), hire(kind)          rules/game.rs
               Game::heal(i), resurrect(i), dismiss(i)                   rules/town.rs
  Garrison   → open_garrison(), leave_in_garrison(), take_from_garrison(),
               swap_with_garrison(), move_guard(), garrison_price()      rules/town.rs
  Market     → Game::market_here(), buy(), sell(); prices in economy.rs (buy_price, sell_price)
  Sanctuary  → spells_here(), knows_spell(), learn_spell(id)             rules/town.rs
  Tribute    → tribute_available(), collect_tribute(), village_offer(),
               accept_offer(), decline_offer(), finish_offer()           rules/economy.rs
               UI: building_view::offer_dialog / answer_offer
  Shipyard   → its own small window: ship_price(), rent_ship()           rules/ships.rs
ui/mod.rs  window closed → Game::window_closed()   runs the scan if a service asked for it

daily (from pass_slice, see map-step.md):
  Tick::Midnight → Game::midnight → economy_midnight() (markets, barracks, stocks, healing)
                                   → ai_midnight()
  Tick::Noon     → new_day() → pay_noon() → NoonPay; Event::NewDay(DayReport) if a report shows
```

The selected quest or rumour is reset when switching building tabs so an old row index cannot
change the selected entry's type or the action button label.

## Key types

| Type | File | What it is |
|---|---|---|
| `Location`, `LocationKind`, `Owner` | `src/rules/world.rs` | a building, its kind, who holds it; `attitude_to`, `hostile`, `takes_garrison`, `hires`, `pays_income` |
| `Recruit`, `Stationed`, `Shop`, `Good` | `src/rules/world.rs` | barracks slots, garrison units, market stock |
| `Tab`, `ServiceError` | `src/rules/town.rs` | window tabs (`tabs`, `first_tab`) and why a service is refused |
| `Price`, `Currency`, `HireError`, `TradeError`, `Tribute` | `src/rules/game.rs` | gold or mana prices and refusals |
| `VillageOffer`, `OfferResult` | `src/rules/economy.rs` | what a village offers instead of its tribute, and what accepting did |
| `WageKind` | `src/rules/content.rs` | how a unit is paid (kind 0–3, mercenaries, event units) |

## The rules that matter

**Tabs by building type** (economy.md §7, `town::tabs`). The first tab is the village's tribute or
the shipyard's ship window, else the main hall. A shipyard opens nothing while the hero is at sea.
An ill-disposed building refuses services (`ServiceError::NotHere`).

**Hiring and healing** (economy.md §2). `hire_price` follows the unit's cost and wage kind;
elementals are hired and paid in mana. Healing costs a share of the unit's cost over the difficulty
factor F (`Game::difficulty`: 100 or 120) and takes no
game time. Corpses stay in the army and can be raised where `resurrects_here`.

**Hire tab's left panel** (interface.md §9.5, `building_view::hire_panel_on`). Pointing at a
hire offer paints its unit card (`recruit_panel`, a fresh level-1 unit of the type) over the
tab column, lights its portrait green and puts `[Army]` line 8 (line 10 with none left) on the
strip; the card stays until the pointer enters the hero's grid with nothing selected or a
tab's place. With a unit selected, the hero's grid shows the card of the unit under the pointer.
Recruits have no tooltip.

**Garrisons** (economy.md §2, ai.md §9.6). Only the player's castles and forts take one
(`Location::takes_garrison`). The hero and named units are refused. Since f4f3a9f a dead unit is
refused too (Razdor's rule; the original moves corpses both ways), but a corpse already there can
still come back. Garrisons are never paid and heal `GarrisonAutoHeal`% at midnight.
`open_garrison` sets the paid marks, the only place they are set; an unpaid unit costs one day's
wage to take back.

**Markets** (economy.md §2, §5). Stock is redrawn at every midnight, building by building, before
the barracks' draws (random order matters for the difftests). Buy/sell prices go through
`relation_price` and `merchant_price`. The market tab (`building_view::market`, 0x4bca8c) shows
the goods or the pack (`BuildingView::selling`): «Инвентарь» and «Торговая лавка» switch them
(`market_list_buttons`; the button of the list shown is disabled and drawn greyed, as the
original's 0x479bbc), the description pane swaps sides with the list; the filter (Razdor's) takes
the column titles' line only while it is used (`item_filter::field_with`).

**List look** (economy.md §2, "How the market and sanctuary lists are drawn"). Both lists
use the original's column widths scaled to the list (`list_columns`): pictures, names left,
prices centred in their column, each title centred over its column. The market's
(`price_list_in_face`) takes (24, 24, 24) off the marble under the names and prices; its icon
column is Win-paper less the same (`chrome::paper_minus`); a pick adds (64, 64, 64). The
sanctuary's (`spell_list`) is one Win-paper panel less (148, 132, 148), darker below the last
spell, the pictures straight on it; a pick adds (24, 32, 24).
A click on a column title sorts either list (`ListSort` in `BuildingView::sort`, Razdor's
addition, main only): «Название» A→Я, Я→A, own order; «Цена» dearest first, cheapest first,
own order; the pick follows its item (`same_pick`). A new window or tab starts unsorted. On
the market the titles' line still starts the filter when clicked beside the titles.

**Sanctuary UI** (`sanctuary`, `spell_list`, 0x4ba854): the first spell starts selected.
Known spells have blue names and prices, the others yellow whatever the gold (the original
has no orange here). The compact card wraps the name without shrinking it; after learning,
the local blue notice replaces the purchase action, with no extra success message outside
the window.

**Villages** (economy.md §3). Entering a village takes it (no attitude test) and collects the
waiting gold and mana, unless it makes an offer: then the tribute waits for the answer. A village
makes at most one offer per visit, by its rolls, drawn as the window opens. The village that made
the last offer makes none until its tribute is taken. A Rogue gets nothing from villages. The
witch's mana is paid at the result window's OK (`finish_offer`, 14eb900), not at Yes. A click on
the village he stands in enters it again (`Game::reenter_building`).

**Noon** (economy.md §1, `pay_noon`). Castles and forts pay their grown gold stock ×F/100, villages
linked to his buildings pay theirs; towns are counted in the shown income but pay nothing. Then the
gold bill (cut by Rear Service, `rear_service`) and the mana bill go out. Short of gold: the
cheapest units get their wage back and go unpaid, unpaid units sit out battles and leave after
`MaxTimeNotUpkeep`. The report (`DayReport`) shows the amounts before payment; with no report the
payment runs at once. The autosave is written as the report opens.

**Midnight** (`economy_midnight`). Village stocks refill (slower as they fill, `regrow`), barracks
regrow by chance (`grow_stock`), mana stocks grow (`grow_mana`), garrisons heal, Medics heal their
army.

## Traps (from past bugs)

- *Window close runs the scan.* A heal, raise or trade sets the wish to scan when the window closes
  (`Game::window_closed`); an event they enable must open then, not at the next step.
- *Re-entry.* A village stood in can be entered again by a click; a click while the step's events
  are still held entered it twice (5d10303).
- *Mana timing.* Resources that the original pays at an OK must not be paid at Yes (witch, event
  results): see events.md §7.2.
- *Wage shown vs paid.* The hire tab and the noon report show the bill without Rear Service; the
  payment applies it. The class card in the promotion tree shows the type's recruit wage
  (555fc8b).
- *Random order.* Every draw (market, barracks, offers) happens in the original's order; adding a
  draw anywhere shifts every later roll and breaks the difftests.

## Parity: where the truth is and how it's checked

- Specs: `docs/reference/original-mechanics/economy.md` (wages §1, prices and services §2,
  villages §3, tabs §7, AI spending §8, reports §9), `ai.md` §9 (the AI in buildings),
  `world.md` §7 (entering), `events.md` §10 (hall quests and rumours).
- Tests: `src/rules/town.rs` (tabs, heal, resurrect, garrison, noon pay, villages),
  `src/rules/economy.rs` (rounding, price bands, stock growth), `src/rules/game.rs` (hire, market,
  village serves once per day), the difftests in `src/difftest/tests.rs` (village offers, services,
  wage bills on the shipped maps).

## How to …

- **Add or change a service.** Put the rule in `src/rules/town.rs` (or `economy.rs` for prices)
  returning `Result<_, ServiceError>`; give a new refusal its own `ServiceError` variant and its text
  in `ui::building_view::service_error` (strings in `data/lang/ru/game.txt`); call it from the tab in
  `src/ui/building_view.rs`; test it in `town.rs` with the `town()`/`inside()` helpers.
- **Change a price or wage.** `src/rules/economy.rs` (`unit_wage`, `buy_price`, `sell_price`,
  `heal_price`, `resurrect_price`); use `delphi_round` / `round_ratio` for the original's rounding.
- **Something at midnight or noon.** `economy_midnight` or `pay_noon`, in the original's order; AI
  armies have their own noon (`ai_noon`) and night (`ai_midnight`) in `src/rules/ai.rs`.

## Verify the references

`python3 -I docs/flows/verify.py docs/flows/towns-economy.md`
