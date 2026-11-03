# «Пепельная Марка»: narrative, pacing and chapter 1 beats (draft for the owner)

The owner played chapter 1 on the phone (2026-11-03): "I don't understand what's going on. There's
no narrative, no anticipation, no explanation." Then: "the map is so empty it doesn't make sense",
"consider the walking time" and "the player's attention span". This document is the fix's design. The
mechanics rework of chapter 1 (Dread from deeds, raids, side quests) is being built in parallel and
fits under it. The player-facing texts here are Russian drafts. The structure is English.

## 1. What is wrong, measured

Walking on `brod` (50×50, knight, default walk speed: one step = 0.15 s real, a wait tick = 30 game
minutes in 0.15 s, so **one game day of waiting = 7 s real**):

| From → to | Cells | Game time | Real time |
|---|---|---|---|
| start → tower | 14 | 5.4 h | 2.1 s |
| start → ford / toll | 11 | 2.9 h | 1.7 s |
| ruins → toll | 6 | 2.9 h | 0.9 s |
| start → marsh village | 48 | 14 h | 7.2 s |
| start → wraiths | 52 | 16.6 h | 7.8 s |
| barrows → wraiths (the longest) | 54 | 22 h | 8.1 s |

- **Walking is not the problem.** The whole map is crossed in under a game day and 8 seconds of
  screen time. Every point of interest is reached in the first ~2 game days.
- **The problem is what's between the points and after them.**
  - The map has 5 places that do anything (ruins, toll, tower, village, the wraiths) and 659
    decorative objects.
  - Seven of those 8 seconds of walking pass nothing.
  - After the wraiths the old design waited ~11 game days (~80 s of pressing «wait» with nothing
    on screen) for a timer the player could not see.
- **No frame.** The opening says three sentences and points at the tower. The player never learns
  who the hero is, who the enemy is, what the Dread is, what wins the chapter, or why any of the
  four meetings matter.
- **No memory.** Story dialogs vanish when closed. Packages cannot write to the diary («Дневник»:
  active quests, completed quests, rumours, messages), so there is no "what do I do now" anywhere.

## 2. Pacing targets

- **A beat every 20–40 s of real play.** A beat is a dialog, a fight, a discovery or a visible
  change. On this map that means something every 4–8 cells of any main road.
- **Chapter 1 takes 10–15 minutes**: about 15–20 beats. Later chapters take 15–25 minutes.
- **Never wait without a reason on screen.** If the story needs time to pass, the diary says what
  is coming and roughly when ("курганы проснутся через несколько дней"), and something happens
  meanwhile (a raid, a visitor).
- **Always one clear objective in the diary,** plus optional ones. The current main objective is
  also lit on the map (a lantern) when it's out of sight.
- **Every scene answers three questions:** who is this, what do they want from me, and what changes
  if I choose A or B. Choices name their cost in the button text («Заплатить 20 золотых»,
  «Сжечь: +40 золота, Страх растёт»).

## 3. The story frame (the campaign bible, short)

- **The hero.** A Crown officer, disgraced. At the Siege of Varn he refused to burn a village with
  its people in it, and lost his command for it. Now he is exiled with the title «Хранитель Марки»,
  which is meant as a death sentence: a province with no army, no money and a rising evil. His
  question across the campaign: *what will you burn to save the March?* That question is the mill in
  chapter 1, the villages in chapter 2, the Order's pyres in 3a and the Grey Bargain at the end.
- **The threat.** The Barrow King, a warlord buried with his host three hundred years ago under the
  north-eastern hills. The war's mass graves woke him. The **Страх** (Dread) is literal: the dead
  feed on the living's fear. Every burned village and every fled family makes them stronger. That
  is why the hero's deeds raise the Dread: noise, blood and fire wake the barrows. It also makes the
  number mean something to the player.
- **The March.** It is not empty. It is emptied: farms abandoned, roads quiet, people hiding. The map
  should show it: burned farmsteads, a broken cart, graves, a hanged man at a crossroads, a refugee
  camp, a shrine with fresh offerings.
- **The voices.**
  - Varga: blunt, sardonic, a soldier who stayed when the garrison ran.
  - The marsh crone: speaks in warnings.
  - The toll chief: greedy but not stupid; he fears the dead more than the Crown.
  - Brother Ilm (later): earnest, frightened, curious about the dead.
  - Kord (later): a laughing mercenary who means none of it.
- **The chapters' questions:**
  1. Брод: can you hold one ford?
  2. Марка: whom do you trust?
  3. The branch: what is your ally's price?
  4. Врата: will you open the old ways?
  5. Курганы: what will you give to end it?

## 4. The diary (engine work, small, no new UI)

Two new trigger actions write into the existing diary window, its «Активные» and «Выполненные» tabs:

- `{ quest = "<id>", title = "<text key>", text = "<text key>" }` adds or updates an active entry.
- `{ complete_quest = "<id>" }` moves it to the completed tab.

Every story dialog is also recorded in the «Сообщения» tab when shown, so a closed dialog can be
reread. The format reference and the editor guide get the two actions. This is Razdor's own feature
for packages, not original behaviour, so no reverse engineering is needed. It changes no layout.

## 5. Chapter 1 «Брод», beat by beat

The main thread: **hold the ford when the barrows wake.** Everything optional feeds it, with men,
gold, Dread or allies.

Map additions (cells approximate; the builder places them on reachable land off footprints):
- **Burned farmstead** east of start (~36, 30): lore and a few coins.
- **Hanged deserter at the crossroads** (~38, 22): a note, which seeds the deserters quest.
- **Overturned cart** on the road south (~30, 32): the refugees' cart.
- **Fresh graves** by the ruins (~31, 14).
- **A dead scout with a letter** halfway to the barrows (~42, 10): foreshadowing.
- **A herb seller's hut** on the way to the marsh (~20, 36).

Each one is a `hero_enters` area with a one-paragraph dialog, so walking pays every 4–8 cells.

| # | Where / when | Beat | Diary |
|---|---|---|---|
| 1 | Start | **Arrival.** A Crown courier hands over the writ and leaves in a hurry. «Хранитель Марки. Корона дарует тебе провинцию — и ни одного солдата. Гарнизон башни бежал неделю назад. Говорят, курганы на северо-востоке шумят по ночам. Если мёртвые перейдут Пепелку, Марку не удержит никто.» | Main: «Удержать брод» («Мёртвые придут к среднему броду. Собери людей, пока есть время.»). Sub: «Занять сторожевую башню» |
| 2 | Walking north | **Burned farmstead.** A doll in the ashes, coins in the oven. The family fled toward the marsh. Seeds the refugees and the marsh. | — |
| 3 | Tower | **The tower.** Two looters in the garrison: a fight. Inside, the deserted garrison's log: «…третью ночь земля у курганов теплая. Капитан велел уходить.» It unlocks hiring and income, and is the base. | Done: «Башня»; new: «Нанять людей», «Узнать, что творится в курганах» |
| 4 | From the tower | **The stones** (exists): the view of the barrows and the standing stones. A lantern lights the barrows. Dread +1 ("the barrows stir": the hero's arrival woke them). | — |
| 5 | Ruins | **Varga.** She stayed when the garrison ran and buried the dead herself (the fresh graves). Choices: «Идём со мной» / «Уходи» / «Под руинами золото — сжечь» (+40, Dread +2, she leaves, and the March remembers). | Sub: «Варга» done |
| 6 | Crossroads | **The hanged deserter.** The note: «Нас было шестеро. Мы не хотели воевать с мертвецами.» The other five hide in the woods by the north ford. | Opt: «Дезертиры» |
| 7 | Toll | **The toll.** The chief names the stakes: «Мёртвые пойдут через этот брод. Мы уйдём на юг, а ты стой тут.» Pay / talk / fight, as now; the talk also yields where the dead were seen. | Sub: «Переправа» done |
| 8 | Road south | **The overturned cart.** The family from beat 2, a broken wheel, a child crying. «Отведи нас в Тростниковую Топь.» Escort or not. | Opt: «Беженцы» |
| 9 | Herb hut | **The herbalist.** Healing for gold; she knows the marsh witch. A hint: «Призраков бьют не железом, а огнём и верой.» | — |
| 10 | Marsh village | **The plea** (exists) and the refugees' arrival (+trust). The wraiths' threat is explicit: «каждую ночь они забирают одного». | Opt: «Призраки топи» |
| 11 | Wraiths | **Fight.** Charm, hunters, trust, Dread +1. | done |
| 12 | Deserters' wood | **Deserters.** Hire them (30 gold, 2 swordsmen) or hand them to the Crown (crown +1). | done |
| 13 | ~day 3–5, Dread 2 | **First raid** from the barrows on the tower or the village. A diary entry with the target, and the raid is lit on the map. | Opt: «Набег» |
| 14 | Dead scout | **The letter.** The scout counted «сорок мертвецов и одного в короне». This names the Barrow King for the first time. Seeds the barrows choice. | Opt: «Курган» |
| 15 | Barrows | **The barrow:** break it (a fight; the host is weaker, and chapter 5 sends one host fewer) or seal it (gold, Dread −1, the same chapter 5 effect). | done |
| 16 | Dread reaches 3 | **Warning, a day ahead:** «Ночью в курганах зажглись огни. К утру они будут у брода.» The ford is lit. | Main updated: «Мёртвые идут. Встреть их у среднего брода.» |
| 17 | The ford | **The host** comes at the hero. The fight. | — |
| 18 | Victory | **The ending** (fire or mercy, as now) plus a short look ahead: a rider from the March's capital: «Марка видела, как ты держал брод. В Пепельном Броде тебя ждут — не все с хлебом.» This hands over to chapter 2. | Main done |

The chapter takes about 15–20 beats and ~12 minutes. The longest stretch of walking without a beat
is ~6 cells (~1 s). The waiting is gone: the Dread is driven by beats 4, 5, 7, 11 and 13, and the
weekly backstop only catches a player who stops playing.

## 6. The later chapters (to do after chapter 1 is approved)

The same treatment for each map:
- a frame (whose land, what's at stake);
- a diary thread;
- points of interest every 4–8 cells on the main roads;
- a warning before every big fight;
- a hand-over line at the end.

The map sizes make it more urgent there: 100×100 and 200×200 maps have walks of 30–60 s.

## 7. Decisions for the owner

1. The hero's backstory (refused to burn a village) and the Barrow King as a named warlord: yes or
   change?
2. The diary actions (`quest`, `complete_quest`, dialogs into «Сообщения»): an engine feature for
   packages, no new UI. Yes?
3. Chapter 1's beat list: cut or add beats?
4. Who polishes the Russian texts: the owner, the community, or the agent with the owner's edits?
