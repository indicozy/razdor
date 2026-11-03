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

## 6. The later chapters

The rules for every map:
- an **arrival** that says whose land this is, what's at stake and the first objective;
- a **diary thread** with one main objective at a time;
- a **point of interest every 6–10 cells** on the main roads of a 100×100 map, and every 10–15 on
  a 200×200 one (lore, loot, a choice, a fight);
- a **warning a day before every big fight**, with its place lit;
- **no wait the player can't see**: a time-gated beat gets a diary line saying it is coming;
- a **hand-over line** to the next map;
- every **hidden counter that decides something gets words**:
  - Dread: the diary's «Курганы» entry, updated at each step («спят» / «шевелятся» / «проснулись»
    / «идут»);
  - taint: the curse is named when it lands, and the cure's place is named;
  - standing: the council says who backs the hero and why.

The style for every text:
- short sentences, concrete things (mud, rust, smoke), no modern words;
- each character keeps the voice set in §3;
- choice buttons say what they cost («Заплатить 50», «Сжечь деревню»).

### 2. Марка (hub, 100×100). Question: whom do you trust?

- **Arrival (rewrite).** The hero rides into Ашфорд (Пепельный Брод). The Crown's surgeon patches
  him up and explains the hub: three powers want the March (the Crown in Ашфорд, the Order at the
  abbey, the Free Companies in the south-east camp), and the marsh folk want only to be left alive.
  «Чтобы удержать Марку, тебе нужен союзник. Совет соберётся, когда ты поможешь хотя бы двоим.»
  The main diary entry is «Найти союзника» (2 favours, then the council), with sub-entries per
  power that appear as each is met.
- **Make the council's rule visible.** The diary counts favours (`quests_done`), and each favour
  line names the power it pleases. The council lists only the powers that back the hero, and says
  why the others don't («Орден не забыл, что ты взял силу алтаря»).
- **Fill the long legs**, each > 25 cells today:
  - Ашфорд→святилище: a burned tithe barn (the Order's tithe, the hint of the shrine) and a
    pilgrim's corpse with a prayer book.
  - Святилище→Серые Ивы: the salt road's milestone (a rumour about the wraith lair, which puts it
    on the map) and a dying horse.
  - Серые Ивы→логово→лагерь: an abandoned toll post with Kord's mark («Корд берёт 40 золотых и
    держит слово — пока ему платят»). This tells the player the price before they arrive.
  - Ашфорд→лагерь: the caravan's road. The caravan master asks for an escort (a new optional
    quest; it explains the raid).
- **Dread hosts:** warn a day ahead with the target village named and lit.
- **The ritual stone:** the shrine's cleanse gives a diary entry naming the stone and what it
  needs. Fix the text so it matches where the gate actually sends the host.
- **Revisit:** a short scene. The March reacts to the branch's outcome (the abbey stood, the salt
  gate fell, the camp held). Then «Врата» opens as the next objective with one line of why: the
  standing stones are the dead's road, and whoever holds them holds the March.

### 3а. Аббатство (Order, 100×100). Question: what is the Order's price?

- **Arrival (fix the contradiction):** the side is already chosen. Prior Агнесса receives him:
  the abbey must hold until the inquisitors come, and the dead gather in the cold to the east.
  Diary: «Удержать аббатство» with sub-entries «Встретить инквизиторов» and «Разведать холод».
- **Fix the unreachable beats.** On this route Ilm is always already with the hero (ilm 1), so the
  gate meeting becomes Ilm's return home: the monks know him, and his doubts start here. The
  «Аббатство устояло» ending needs a reachable condition (the Ольховка «ride on»).
- **Ольховка is the chapter's heart.** The inquisitors demand it burned for hiding a «tainted»
  family. The scene must state this before the choice: who, why, what burning means. Ilm pleads.
  Burn, spare or ride on: each line names its cost.
- **The cold (blight):** name the taint when it lands («холод въедается в кости: −…») and say
  where the cure is (the abbey's rite, 30 gold).
- **Siege timing visible:** the dead's patrol is seen marching on the abbey a day before; the
  diary says «Мёртвые идут к аббатству. Будь внутри стен». The weekly wait gives way to a trigger on
  the Ольховка outcome plus a day (the player's act, not the calendar).
- **Points of interest on the start→abbey road (41 cells):** a pilgrim column turned back, a
  wayside cross with the Order's edict, the watch fort's garrison (it already exists: give it a
  scene).

### 3б. Солончаки (marsh folk, 100×100). Question: what is the marsh's price?

- **Arrival:** the witch's message through Tess. The Salt Gate's castellan hangs marsh folk for
  smuggling salt, and the salt is the marsh's only coin. Take the gate and the marsh trusts the
  hero for good. Diary: «Взять Соляные Ворота». The wagon in the arrival is explained as a Crown
  salt tax wagon, and stopping it is an explicit option (a sub-entry with its cost: crown −1).
- **The disguise is an honest plan.** Reedmere proposes it: «В наших плащах тебя примут за
  контрабандиста — до ворот». The gate scene keeps that promise: the disguise gets him in, then the
  castellan recognises him. Say this in the text so the forced draw reads as story, not a trick.
- **The trail and the witch** get hints: Tess mentions the trail when she joins, and Reedmere
  mentions the witch's cure when the bog curses him.
- **Fill start→Reedmere (57 cells):** the smugglers' den (it exists: a scene, and the salt economy
  explained), a hanged smuggler on a salt cairn, a flooded chapel.

### 3в. Лагерь рот (Companies, 100×100). Question: what is Kord's price?

- **Arrival:** Kord's camp. He explains the deal: the Crown will send a relief column to retake
  the camp in about three weeks, and until then the hero holds it with the companies. Diary: «Удержать
  лагерь до подхода Короны» with the time **stated**. Replace the pure weekly gate with «the relief
  is sighted» after the hero's two big acts (the red company and the supply column) or the third
  week, whichever comes first, and warn a day ahead.
- **Kord is with him** (kord ≥ 1 here): rewrite Kord's camp lines for a hired Kord, and drop the
  unreachable offer and the «keep» choice text or make them reachable.
- **The supply column** is the chapter's moral beat: robbing the Crown's convoy (gold, the
  companies' respect, the Crown's enmity) versus letting it pass. State it.
- **The waygate:** lighting it says what it does in chapter 4.

### 4. Врата (100×100). Question: will you open the old ways?

- **Arrival:** the four standing stones are the Barrow King's old road. Open them and the hero
  moves fast but so do the dead; seal or claim the last and the March is shut to them. Diary: «Врата»
  with **four sub-entries, one per stone, each naming its guard and its other way** (rite, toll,
  charm, fight). Fix the fixed «Два из них» text to read the actual `gates_open`.
- **Teleport cost** stated at the first stone: «каждый проход оставляет холод: скверна +1».
- **Kord's pay:** make the text and the trigger agree.
- **The legs are 22–36 cells and empty:** add the dead's road markers (bone cairns that grow as
  Dread grows), a Crown courier, Ilm's or Tess's or Kord's companion scene halfway between stones
  (one per ally), and the wraith messenger heard before he's seen.
- **The last gate:** close (Dread −2) or claim (power, taint +1), with both consequences said.

### 5. Курганы (finale, 200×200). Question: what will you give to end it?

- **Arrival:** the whole March marches. The diary shows the three endings' **conditions in words**
  («Корона и Орден за тебя», «Роты или болото за тебя», «Ильм слышал посланника»), so the council
  isn't a surprise. The conditions are read at the end, not frozen at the start (fix it), so the
  finale's choices can still change them.
- **Start→ford fort is 64 empty cells,** the worst walk in the campaign: add the allies' camp, a
  field hospital (a heal), refugees on the road, the first Dread host warned and lit.
- **The companion fates are the emotional core:** each scene states what it costs and gives a
  choice with weight. Tess and Ilm get real choices, not «no effect».
- **Varga→Ilm is 48 empty cells:** add the barrow fields (graves opening as the Dread rises), the
  King's herald (he offers terms, which sets up the grey bargain for everyone, not only Ilm's
  heresy: a refusal everyone can make).
- **The King:** a one-line monologue before the fight that answers the campaign's question
  («Ты не сжёг деревню под Варном. Посмотрим, что ты сожжёшь сейчас.»).
- **Endings:** each closes the question, plus one line per companion's fate and the March's state
  (the roads, the villages).

## 7. Decisions for the owner

1. The hero's backstory (refused to burn a village) and the Barrow King as a named warlord: yes or
   change?
2. The diary actions (`quest`, `complete_quest`, dialogs into «Сообщения»): an engine feature for
   packages, no new UI. Yes?
3. Chapter 1's beat list: cut or add beats?
4. Who polishes the Russian texts: the owner, the community, or the agent with the owner's edits?
