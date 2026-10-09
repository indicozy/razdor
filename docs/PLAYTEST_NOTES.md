# Playtest notes

Things noticed while playing Razdor, to look into. Newest first. Each note says which branch it
was seen on and what to check.

## 2026-10-09, main: вторая карта обучения и РК1 от игроков из Discord (0.3.18)

Скриншоты игроков (оригинал и Razdor) в ветке Discord, в репозиторий не кладём.

Обучающий2:

1. **Окно события не показывает выученные заклинания** («Дар отшельника»). В оригинале под
   текстом строка «Выучены новые заклинания:» и их значки.
2. **Нет просмотра заклинания или предмета из окна события.** В оригинале щелчок левой или
   правой кнопкой по значку открывает его описание (предмет — окно с названием и свойствами,
   заклинание — карточка с картинкой, маной, временем чтения и действия).
3. **Значки предметов и заклинаний в окне события не центрированы.** В оригинале они
   расставлены по ширине окна в зависимости от их числа, подпись «Получены предметы:» над ними.
   Пункты 1–3 исправлены (0x4d5884, 0x4a8ae8, 0x4c1fec, 0x4c2048): под текстом, как в
   оригинале, строки значков с зелёной подписью по центру окна (`[Event] AddItem`,
   `NewSpell`), снизу вверх: полученные предметы, вступившие и ушедшие бойцы, выученные
   заклинания; значки разнесены по ширине по их числу (центр i-го в W·(i+1)/(n+1)).
   Нажатие левой или правой кнопкой на значок показывает предмет или карточку заклинания по
   месту нажатия, пока кнопка не отпущена. Заодно: событие «без встречи» теперь, как
   оригинал, прячет только строку золота, маны и опыта (предметы только у опкода 6), а не
   всё сразу.
4. **Наём в казарме не центрирован.** В оригинале портреты для найма расставлены по ширине в
   зависимости от их числа, в Razdor прижаты влево.
5. **В окне армии у бойцов нет описания** (текст класса и способности под характеристиками,
   как у «Ополченца» в оригинале). Подсказка ветки развития выводится под блоком «Описание
   артефакта» и частично не видна.
6. **Окно здания: на вкладке «Рынок» не влезает «Золото»**, на вкладке «Святилище» в списке
   нет картинок заклинаний, описание заклинания наползает на другие элементы и не читается
   (вылезает за рамку и под список). Игрок просит привести заклинания к виду предметов на
   рынке: значки в списке, описание в своей рамке с переносом строк.
   Исправлено. Святилище разложено как в оригинале (0x4ba854, 0x4ba078; размеры сняты со
   скриншота оригинала): «Описание заклятия» над карточкой заклинания (картинка в серебряной
   рамке слева, справа название, действие, мана и чтение, время действия, всё с переносом
   внутри рамки, как у предметов), под ней сообщение оригинала (`AlreadySpell`,
   `NoMoneyForSpell`, `NoPlaceForSpell`; «Купить» тогда недоступна), «Деньги» с суммой
   под словом и «Купить»; справа список «Название заклятия / Цена» на шесть строк со значком
   каждого заклинания. Лишняя строка Razdor «Книга: N/15…» убрана. На рынке «Деньги» (ini
   `[Building] Gold`, как в оригинале) стоят над суммой и помещаются между кнопками.
7. **Деревни после посещения стали моими** (зелёный контур, имя владельца сменилось).
   Исправлено отображение, захват остался. Оригинал тоже отдаёт деревню герою, как только он
   на неё наступил и в ней нет армии (0x4ad94c), но этого не видно: подсказка здания называет
   держателя только у замка, крепости и развалин (0x4cb18c), у остальных всегда владельца с
   карты, а колец под зданиями оригинал не рисует. Подсказка теперь так же; кольца под
   зданиями убраны (см. п. 15). Тест
   `a_captured_village_still_names_the_maps_owner`.
8. **Именной персонаж** (Крестьянин Йошка): не показывает своего имени, не отличается от
   остальных (в оригинале полоса характеристик голубая, у героя красная, у прочих оранжевая),
   не имеет личных предметов (надеты, снять нельзя, пока он жив; у Йошки вилы), теряет
   предметы, когда событие убирает его и потом добавляет снова.
   Исправлено.
   - Имя: правило оригинала (0x49747c, заголовок карточки 0x492f24). Герой — класс и имя
     («(нет имени)», если имени нет: `[NewHero] PrivateHeroName`), именной персонаж — своё
     имя, остальные — класс. Так в заголовке карточки окна армии (имя именного своим
     цветом), в подсказках армии и здания и у бойцов в бою (у любой стороны). Гарнизон вне
     боя по-прежнему называет класс, как оригинал.
   - Строка подсказки окна армии теперь оригинальная (0x4c2f54): строки 0–2 `[Army]` с
     именем в кавычках, над пустой клеткой — Line1–Line3 по номерам карточек.
   - Полоса характеристик (0x49462c): у героя красная, у именного голубая, в армии героя и в
     бою (в бою красная и у первого бойца врага).
   - Личные предметы: оригинал при загрузке отдаёт первому бойцу армии счёт его надетых
     предметов (+0x18, 0x4b2504), и первые столько ячеек живого бойца не снимаются
     (0x4c24f4), над ними `[Army] ItemI`. Теперь так же: у Йошки вилы.
   - Боец, которого событие отдаёт армии, уносит всю запись (опыт, предметы, оплату, имя,
     личные предметы), и событие возвращает его таким же. Имя теперь у каждого бойца армии
     (а не одно на армию). Тест `tutorial2_yoshka_keeps_his_name_and_pitchfork_through_the_events`.

Замечено попутно на тех же скриншотах: в оригинале у героя в окне армии и в казарме передний
ряд на 6 клеток (резерв только по краям заднего), в Razdor передний ряд на 4 с шатрами по
краям. Исправлено: ряд и в Razdor широкий (`OptValue11=1` установки), но шатры рисовались на
обоих концах обоих рядов. В широком ряду оригинал рисует передний ряд одними мечами, шатры
только по краям заднего (0x4de2xx); теперь так же.

12. **Излишек опыта при развитии.** Герой сохраняет излишек опыта при переходе на новый
    уровень, остальные бойцы при развитии в новый класс теряют весь опыт (оригинал: уровень 1 и
    0 опыта, 0x4b1df0). По просьбе пользователя сделано лучше оригинала, только в main: опыт
    переносится в новый класс по правилу набора и может сразу дать уровень. ИИ развивается
    по-прежнему, как в оригинале.

13. **Журнал героя по главам.** История журнала (дополнение Razdor, в оригинале журнал
    пустеет на каждой карте) переносится на следующие карты кампании, и к третьей карте в ней
    трудно ориентироваться. Разделить её по главам (картам). Сделано, только в main: история
    помнит название карты каждой главы, а список журнала, когда в нём записи нескольких карт,
    ставит над записями каждой карты заголовок с её названием (в старых сохранениях «Глава N»).
    Тесты `chapters_are_headed_by_their_maps`,
    `the_journal_list_heads_each_chapter_when_there_are_several`.

14. **Кнопка «Нанять» тёмная и плохо читается**, в отличие от яркой «Лечить». В оригинале
    «Нанять» ярко-зелёная (казарма Замка Молот на скриншоте игрока). Исправлено: оригинал
    делает зелёную кнопку из синей (smb-up, smb-down), меняя местами зелёный и синий каналы
    (0x48dd40 с 1, 3, 2). Razdor брал серую копию и тонировал её зелёным, отсюда тёмная
    кнопка. Теперь так же, как в оригинале.

15. **Тень на карте почти не видна**, а «нелепые кружки вокруг юнитов и строений» её
    перекрывают. Исправлено. Тень фигуры у оригинала в альфе самих кадров `.ugs`, и Razdor
    её рисует так же; глушили её добавки Razdor времён заглушек: светло-зелёный круг под
    героем, тёмное пятно у ног каждой фигуры и сплошное кольцо поверх ног армии. Всё это
    убрано. Вместо кольца, как в оригинале, под героем и армиями тонкая пунктирная метка на
    земле (`Selection-1.lit`, 0x48eeb0, 0x4ce30c), в проходе земли под деревьями, зданиями
    и фигурами (0x4c9459): у героя жёлтая, у армий красная. В оригинале она вращается
    (5,625° за 100 мс), в Razdor пока стоит. Кольцо под замками и крепостями тоже убрано:
    оригинал под зданиями ничего не рисует (0x4c9b5b).

16. **Значки благословения и проклятия в бою не пропадают после конца эффекта.** Оригинал
    ставит их, когда заклинание легло, и снимает только после боя (0x490720), так что это
    поведение оригинала. По решению пользователя в main значок виден, пока действует сам
    эффект (до начала следующего хода, когда модификаторы сбрасываются). Только main.
17. **Окно загрузки всегда открывается на «Личных»**, хотя чаще грузят автосохранение.
    Запоминать последнюю вкладку или открывать автосохранения первыми. Сделано, только main:
    окно открывается на вкладке, на которой его оставили, в первый раз на автосохранениях
    (пустая вкладка, как и раньше, уступает другой).
18. **«Рестарт» в окне выхода из битвы начинает сценарий заново**; игрок хочет перезапуск
    этой битвы. По решению пользователя в main «Рестарт» перезапускает битву (с
    автосохранения «Битва - …», сделанного перед ней); рестарт сценария остаётся в главном
    меню. Сделано, только main: битва держит игру в том виде, в каком она началась (с
    противником), и «Рестарт» в её окне выхода, после вопроса «Начать эту битву заново?»,
    восстанавливает её и открывает ту же битву. Текст окна говорит о перезапуске битвы.
    Тест `a_game_kept_as_its_battle_began_comes_back_with_its_foe`.

РК1:

9. **Вражеские армии в подсказке перемешаны.** Армия барона Балтазара фон Моргена (армия 8:
   Конный Рыцарь-предводитель, 2 Конных Сержанта, 2 Лучника, 2 Святых брата): в переднем ряду
   стоят лучники и святой брат, сзади один святой брат. По авторасстановке оригинала (483b3c)
   спереди должны быть рыцарь и сержанты, сзади стрелки и монахи.
   Исправлено. Армия 8 ждёт за картой, пока её не выведет событие, а Razdor расставлял при
   загрузке только армии на карте, и она стояла в клетках, как её набрали (сначала резерв).
   Оригинал проводит через бой и обратно каждую запись армии при загрузке (0x4b56a8), при
   выводе на карту (0x4969b8, с защитой здания под ней), при возрождении (0x4a28d0) и после
   найма ИИ (0x4a7923, 0x4a7989); теперь так же (`game::arrange_troops`). Тест
   `rk1_balthazars_waiting_army_stands_arranged`.
10. **Именной персонаж Людвиг фон Отл** (королевский герольд, Идальго, событие 27) показан как
    «Идальго». Его должно быть нельзя оставить в гарнизоне.
    Исправлено: событие 27 даёт ему имя 1, и теперь он везде называется «Людвиг фон Отл»
    (см. п. 8). Оставить его в гарнизоне и раньше было нельзя: любой путь в гарнизон
    отказывает герою и именным, как оригинал, но молча. Теперь окно здания, как оригинал,
    пишет над гарнизоном «Уникальный персонаж не может быть перемещен в гарнизон!»
    (0x4c6964, `[Army]` строка 9), а над героем или именным при выбранном бойце гарнизона
    «Нельзя обменяться местом с выбранным персонажем!» (0x4c612c, строка 11).
11. **Молнии по дороге к магу не видны.** В точках, которые невозможно обойти, молния бьёт по
    армии игрока. В Razdor урон проходит, но на карте удар никак не показан.

## 2026-10-09, main: первая карта обучения от игрока из Discord (0.3.17)

Скриншоты и видео игрока (оригинал и Razdor) в ветке Discord, в репозиторий не кладём.

1. **«Попытка вернуться в Долину Призраков» сразу после «Встречи с призраком»** (Обучающий1),
   хотя герой просто идёт дальше. Исправлено. Ответ «Да» призраку (событие 6) убирает его с
   карты (армия 4), а в оригинале снятие армии с карты (0x496900) заканчивает и встречу с ней.
   Событие 7 («встреча с армией 4» и «Да на 6») поэтому ждёт новой встречи, когда событие 8
   вернёт призрака. Razdor встречу не заканчивал, и молния била сразу. Тесты:
   `a_meeting_ends_when_its_army_is_deactivated`, `tutorial_the_ghost_sent_away_does_not_strike_at_once`.
2. **Мосты сдвинуты, «ходим по воде».** Исправлено. Оригинал ставит картинку здания правым
   нижним углом в правый нижний угол его клетки (на 11 точек выше, если здание шире, чем выше;
   0x4ce30c), а мостам (типы 13 и 14) добавляет сдвиг из таблицы в exe (0x4ecd88; на 21–34 точки
   вниз). Razdor центрировал картинку на клетках и сдвига не знал, поэтому мост стоял почти на
   ряд выше своих клеток. Сверено с оригиналом под Wine на первой карте обучения: оба моста и
   деревня совпадают с точностью до 1–2 точек.
   Заодно деревья, камни и холмы на карте теперь стоят там же, где в оригинале. Картинка горы
   или камня встаёт правым нижним углом на 16 точек правее и на 11 ниже угла своей клетки
   (0x4ce30c). Деревья и кусты сдвинуты на свой случайный сдвиг: оригинал считает его при
   загрузке карты из координат клетки (0x4cfb24). Razdor ставил всё по центру клетки. В редакторе
   карт здания и мосты теперь ставятся по правилу редактора оригинала (0x5ab588); предметы
   карты в редакторе пока по центру. Сверено с оригиналом под Wine: мосты, деревня, камни и
   сухие деревья совпадают с точностью до точки.
3. **Победное событие показывается раньше перехода.** Исправлено. На Обучающий1 победа —
   событие 21 «Встреча с королевским гонцом» (встреча с армией 11). Правила заканчивали
   сценарий внутри шага, а окно события выводилось только после того, как шаг доиграет на
   экране; проверка конца ждала лишь пустой очереди окон, и «Победа!» открывалась раньше окна.
   Теперь конец ждёт окна, а после него кампания сразу переходит на следующую карту, как в
   оригинале (events.md §11, 0x4b5b64); экран «Победа!» остался только для карт без следующей.
   Тесты `a_victory_met_on_a_step_is_over_before_its_window_is_on_screen`,
   `the_tutorial_ends_on_meeting_the_herald_and_hands_over_to_its_second_map`.

## 2026-10-08, main: РК1 от игрока из Discord (0.3.17, Windows)

Всё на стартовой карте «Раменского королевства» (РК1). Скриншоты игрока в ветке Discord, в репозиторий
не кладём.

1. **Морские разбойники.** В оригинале они боятся героя и уходят в море при его приближении, но
   если подождать у деревни, пока они в неё заплывут, и напасть, сбежать они не успевают. В Razdor
   «входа в здание» у них нет, поймать их невозможно.
   **Проверено, правила не менялись.** На РК1 (армия 9, корабль) в Razdor они, как и в
   оригинале, заплывают в Деревню Соленую (контуры зданий открыты на карте кораблей) и стоят в
   ней. Шаг героя на любую клетку деревни, пока они там, начинает бой с ними (0x4ad94c,
   правило 2). Если герой ждёт внутри своей деревни, они нападают на него сами (0x4a548c).
   Отталкивание от героя на суше не действует, как и в оригинале (разная среда). Добавлен тест
   `rk1_sea_robbers_are_caught_in_the_village`. Уточнить у игрока, какую установку он
   использовал (Evolution?) и как именно пытался напасть: в Razdor они стоят в деревне 1–3
   игровых часа.
2. **Гарнизон отрядов и строений отображается перевёрнутым.** Сделано. В окне здания (вкладка
   гарнизона) гарнизон рисовался фронтом вверх, как армия героя. В оригинале (0x4d9060) верхняя
   сетка гарнизона зеркальна: передние ряды гарнизона и героя смотрят друг на друга через
   разделитель. Теперь так же (`building_view::screen_line`, тест
   `the_garrison_grid_is_mirrored_the_heros_is_not`). Окно армии, бой и всплывающие подсказки
   армий и зданий (фронт внизу, 0x4ca9f0/0x4cb18c) уже совпадали с оригиналом.
3. **Порядок диалогов в церкви.** Исправлено. События шага приходят на экран после того, как шаг
   проигран (`Game::tick_shown`), а место, открытое фонарём события 9 (замок Бонитура), уходило
   на карту сразу, и окно 9 ещё не стояло в очереди: камера летела первой. Страховка
   `release_unshown_window` тем временем считала окно 9 непоказанным и отпускала сканирование:
   окно 10 открывалось раньше 9, квест (9) приходил последним. Теперь места, открытые за шаг,
   ждут вместе с его событиями, а страховка ждёт, пока они придут. Порядок как в оригинале:
   окно 9 (квест), по OK полёт к замку, затем окно 10 (тест
   `rk1_church_windows_come_in_the_originals_order`).
4. **Вражеские строения подсвечиваются как дружественные.** Сделано. «Подсветка» — это рамка
   подсказки на карте. В оригинале их три (0x4ca8ac): обычная зелёная, враждебная красная
   (`Win-red`, оранжевые имена) и нейтральная тускло-коричневая. Замок, форт или руины не
   игрока — красная при отношении к игроку ниже 1, руины с гарнизоном всегда, пустые руины и
   мосты — нейтральная, остальные здания — обычная (0x4cb18c); армия — красная при отношении
   ниже 1 (0x4ca9f0). Razdor рисовал все подсказки зелёными. Тест
   `rk1_buildings_take_the_originals_tooltip_frames`.
5. **Razdor.exe в папке с пробелами в пути не нашёл игру**
   (`D:\Games\Discord Times\Discord Times Community Update Evolution Mod`); в
   `D:\Games\BOOK_OF_WOE` нашёл. Не повторилось: 0.3.17 под Wine из папки с тем же именем
   нашёл игру, значит, дело не в пробелах. Скорее всего, в корне папки с модом нет `Maps_Rus`
   или `Rus_Units.ini`, либо какой-то файл не читается. Теперь список сценариев пишет причину
   (какой папке чего не хватает или какой файл не прочитан) и советует положить Razdor рядом с
   `DiscordTimes.exe`. Попросить у игрока `%APPDATA%\razdor\razdor.log`.

## 2026-10-08, main: a small lantern stays dark; the sound crackles

1. **"The lantern did not work: the minimap shows a burning red building, but the map keeps it
   in the fog of war"** (a mod map, screenshot only). The camera did fly there and the cells
   were explored, but Razdor draws the fog as a 5×5 blur of the explored cells: a radius-1
   lantern opens 9 cells and the blur kept their centre at darkness 245 of 255, on the map
   and on the minimap. In the original the soft fog is full up to the radius (world.md §3).
   Now an explored cell with no dark neighbour is clear, and one at the edge is at least a
   third lit (`minimap::darkness_of`, test `a_small_lantern_shows_through_the_dark`).
2. **"The sound crackles at any volume; now and then it stops for a few seconds, then starts
   again."** (Windows.) quad-snd's WASAPI loop looks sound and neither it nor macroquad has a
   Windows crackling report. What it does wrong on every platform: it brings every sound to
   44100 Hz by repeating the nearest sample, and all of DT's sounds are 22050 or 11025 Hz,
   so a tone at 8 kHz keeps an image at 14 kHz at about two thirds of its level: a metallic
   buzz at any volume. Razdor now resamples every sound to 44100 Hz itself with a Lanczos
   filter (`Pcm::resampled`, 64 ms for the longest track), so quad-snd's resampler is not
   used. **To confirm with the player** on the next build; if it still crackles, look at the
   WASAPI output loop next.

## 2026-10-07, main: crash after the courier's meeting (0.3.10)

1. **"The game crashes"** (Другой берег, Evolution install, Windows build of 0.3.10): `index
   out of bounds: the len is 20 but the index is 20` at `world_view.rs:1377`, right after
   E108 «Встреча с посыльным» (meets army 49, deactivates it) and, as its window closed,
   E162 «В Таверне» with a `MET` of an army no longer on the map. The crash is fixed:
   `describe` looks the army up with `get` as `play_event` already did (test
   `a_meeting_with_an_army_gone_from_the_map_is_told_without_its_name`). **Still open:**
   how a `Met(i)` reached the screen after its army left. `meet_as` takes the index after
   the events ran, so it should be fresh; suspects are `Game::held` (a stretch's events
   wait for its step to play while later code changes the armies) and the events delivered
   around an event window's close. Not reproduced from the player's save (taken an hour
   earlier, day 5 2 h): walks to every cell within 20, waits of 1/4/8 h, the endless wait and
   a chase of the courier all fire E108 cleanly, and E162 never fires (no tavern near). Worth
   carrying armies by uid in `Event::Met`/`Encounter` if it shows up again.

## 2026-10-06, main: РК3 → РК4 keeps the army and the items

1. **"The move from the capital to the eastern province takes away your soldiers and all
   items but the hero's own; now the army stayed (and the items would have, had I not sold
   them)."** Done. The hand-over (0x4b5b64) reads the carry-over bytes 0x110–0x116 of the map
   it has just loaded, the **next** one (events.md §12); Razdor read the old map's. РК3 has
   `[1,1,1,1,1,1,1]`, РК4 `[1,1,1,1,1,0,0]`: no pack, no army. `Game::next_map` now hands
   over everything and `Game::apply_carry_over` takes what the new map's bytes allow; a test
   on the install's РК3 → РК4 checks it (`rk4_takes_neither_the_army_nor_the_pack_of_rk3`).

## 2026-10-06, main: abilities missing from the unit panel

1. **"Not all abilities are marked: Wrath of God does not show on healers, nor custom
   abilities of mods."** Done. Seen with the Community Update Evolution install, whose
   priests and bishops (units 32, 33) have `Bonus=GodAnger`: the unit panel drew the traits
   last and cut them at its bottom, and a caster's stat list (the magic lines) left one or
   two lines of room, none at smaller windows. The panel now measures the traits first and
   starts the name and stats higher, over the figure, when they would not fit; in battle the
   description takes only what is left (`unit_sheet::draw`). A token the loader does not
   know is no bonus in the original (0x48efd0) and stays unshown.

## 2026-10-06, main: an attacker comes out of nowhere

1. **"Sometimes you walk and the battle opens: an enemy ran into you, but while you walked he
   was far away. In the original you at least see him coming."** Done. The rules' order
   already matched the original (the armies walk inside the hero's step, their attack comes
   at its end, 0x4ade3c); only the drawing did not. Razdor slid the hero towards his next cell
   *before* the step was worked out and the armies' steps of that time *after* it, a window
   later, and an attack opened the battle in the frame it was decided, so the attacker's
   steps of that window were never drawn: it jumped from its old place. Now the hero's step
   and the armies' steps play over the same window (`Game::display_pos`, `hero_glide`), and
   what the step brought (its events, the battle) waits until that window has played
   (`Game::tick_shown`, `Game::step_playing`): the attacker is seen walking up to him. Running
   into an army is decided before he moves, so he no longer slides towards it first. The
   diff test and the replays call `Game::tick`, unchanged.

## 2026-10-05, main: the new game's map list does not scroll

1. **"The player cannot scroll the maps in the menu after «Новая игра»."** Done. Reproduced on
   Xvfb with four extra maps in a copy of the install and XTest wheel clicks: the wheel moved
   the list for one frame, then `wheel().signum()` (1.0 for no turn) pulled it back up every
   frame; a campaign block only partly in view was skipped, hiding it and what followed. The
   list now keeps its scroll and clips its rows to the box. With the 15 shipped maps the list
   fits (at any window size), so it showed only with more maps. The original's
   list (4d2a30) is a text list with a scroll bar; Razdor's grouped list has none.

## 2026-10-05, main: the leftovers of the Discord reports, and the scenario texts' marks

1. **The original's shipyard window.** Done (0x4bbc84, 0x4d3ec0, 0x4d1314, 0x4c60ac): on land
   a shipyard opens its own small window (picture, `AboutShipyard` with the owner, the price
   line, «Нанять корабль» and «Отмена», `NoMoneyForShip` in red when gold is short); buying
   closes it; at sea nothing opens.
2. **Arranging the army by clicks** (0x4c346c, 0x4c653c, 0x4c6f50, 0x4b0c04). Done: a press on
   another unit with one selected swaps them, on an empty cell slides the card there, both
   with `Card-Move`, ending with none selected; the hero is a unit like the others. By the
   disassembly a press on the selected unit only deselects it and leaves its tree up (the
   0.3.1 fix brought the pack back at once; the user chose the original's way); a press on an
   empty cell with nothing selected brings the pack back.
3. **A unit that cannot be promoted** (level 0 or no next type) shows every tree portrait
   locked: grey, darkened, with the vignette (0x494340); Razdor's own notes under the tree went.
4. **"| ^ @ * — these marks in the scenarios"** (the tavern channel). Done: the original reads
   them with 0x48e438 / 0x48e1cc in the event window, the tutorial offer, the restart and
   delete-save boxes (`*` white, `|` blue, `@` orange, unmarked pale yellow; `^` centred,
   else justified behind a six-`_` indent; `#\` or CR LF a break). Only the two tutorials'
   events and a few ini texts use them. The journal shows them raw in the original too.

## 2026-10-05, main: a player's reports from the Discord thread

From the forum thread «Razdor - открытый движок для "Времена Раздора"» (cheats left out).

1. **"They aren't supposed to be highlighted in red. Plus some strange flags on the map."**
   Done. The minimap coloured every building by its faction and drew Razdor's own symbols;
   now it follows Minimap_Refresh 0x49e28c and the icon tables 0x4ed520/0x4ed5d4 (only
   castles and forts by side; armies as 9×9 shields, red unless a meeting with a friendly
   army waits, none inside buildings). The flag was Razdor's pennant over buildings; the
   original draws none (0x4c9b5b). By the user's choice a ring in the owner's colour under
   castles, forts, towns and villages replaces it, as under the armies.
2. **"How do I buy a ship?"** Done. The shipyard was ill-disposed and Razdor hid the ship tab
   at such shipyards; the original's ship window (0x4bbc84) tests neither attitude nor owner.
   Left: the original's shipyard is its own small window (`AboutShipyard`, Buy, Cancel, the
   buy closes it, nothing at sea); Razdor keeps its building window with a ships tab.
3. **"Wrong model"** (Разбойники у дороги, Другой берег army 40). Done. The figure is the
   original's +0x169d from the map loader 0x4b4824 (style, undead and mage leaders, ships),
   not byte 5; this army walks as the Rogue. Left: event opcode 17 on the hero changes his
   figure in the original; Razdor draws him by class.
4. **"Levelling blocks the inventory."** Done. The tree replacing the pack for a selected unit
   is the original's (0x498d0c); Razdor lacked the deselect (0x4c346c: pressing the selected
   unit, the end of a swap or slide). Left: the original swaps two units by pressing one then
   the other, Razdor by dragging; a final-class unit's portraits are greyed with a lock in
   the original (0x494340).

## 2026-10-04, dt-feat: dt-original merged

1. **A campaign's next map that leaves the carried class out** (the dt-original note of
   2026-10-03 below asked dt-feat to treat it as an original bug). Done, as "Razdor fixes the
   original's bug": the hero keeps his class and his carried record and starts where the
   map's first offered class would, on its preset's cell with its start building and preset
   (then the carry-over as usual), not on cell (0, 0) of the empty preset
   (`world::start_preset`; saves-data.md §10.4, interface.md row 19a). Of the two options,
   switching him to an offered class would have clashed with the carried hero record (his
   unit type, level, items and book), so the class stays and only the start moves. A new
   game still offers only the classes the map defines, as the original.
2. **РК2's peasant offers after the merge.** Re-run: `rk2_the_peasant_offers_and_the_mines`
   passes on dt-feat unchanged. dt-feat's fix for repeating questions applies only to an
   asking event without a message; offers 8, 9 and 10 all have one, so their declines, the
   next visit's questions, offer 10's replacement and the quest's end play as on dt-original
   (the original's).
3. **Stacked windows** (note 1 of 2026-10-03, "what was seen on dt-feat should be checked
   again after it merges"): dt-feat now has dt-original's window order (one event window at
   a time, the events behind a window waiting for it); not re-checked by playing yet.

## 2026-10-04, dt-original: spell badges on the unit cards, items dropped on the hero

1. **"In the grid of units I don't see what buffs or debuffs (magic) they have on them."**
   Done. The original (interface.md §9.4; 493a64, 49ece8, 49d044, 49b63c), checked under Wine
   on Проклятое озеро (the archmage's «Укрепление Брони» on his army, the army window): every
   card of the army, building and battle grids shows up to four 22 px badges 23 px apart along
   the portrait's bottom, one per running spell with a mana cost, in slot order; hovering one
   shows a 420 px box with the spell's 50 px picture, its name, the effect text (blue for a
   spell on the own army, else red), "Отбирает жизнь: n %" when the unit has lost life to a
   `p-LifeLose` spell, and "Осталось времени действия: 10 час" (days and hours; «Неизвестно»
   from 40 000 minutes on; the month part's +1 quirk kept). Razdor now draws the same
   (`rules::spell_hint`, `ui::spell_badges`, the badge composed from the install's
   `Spell-*` and `si-*` art), and the battle cards' signs follow the original (potion and
   blessing top left, poison and curse top right, the curse and blessing kept to the battle's
   end); Razdor's bleed and hero badges went. Snapshots: `RAZDOR_SCENE_SPELLS`.
2. **"When I drag an item from a unit onto the hero in the grid, it must go to the overall
   inventory (the pack)."** Done. The original's army window (magic-items.md §5.2, 0x4c346c →
   0x4979c4): a potion dropped on a card is drunk; on the hero's card any other item goes to
   the pack; on another unit's the wear test runs and the item takes its first free slot; a
   refusal leaves it on the cursor. Razdor's drop on a card now follows it
   (`Game::give_item`); a drop on the unit panel still equips the selected unit, as the
   original's hero window does.

## 2026-10-04, dt-original: the wait and centre buttons, and stopping a wait

1. **"I don't see any way to centre the camera back on the hero. When I hover the bottom
   centre, three buttons must pop up."** Done. The original (interface.md §6; 0x4d46e0,
   0x4b930c, 0x4b93a8, 0x4b9448): hovering the message box (372, 684, 280×60) on the idle
   map shows `GP-ButtonLeft` (wait 1 h), `GP-ButtonCenter` (centre on the hero) and
   `GP-ButtonRight` (wait 4 h) over it, its text hidden; screenshot under Wine on РК1. Razdor
   now draws them the same (`game_bar::TimeButton`, the install's art with its alpha masks,
   placeholders without one), with the `cp_*` hints and the button sound; the centre button
   glides in the original's 900 ms cosine (`world_view::glide_ease`), as Tab now does. Keys
   1, 4, Tab and the time panel's clicks off the buttons stay (F1 list updated).
2. **"When I click to wait 4 hours, I want to be able to stop it by clicking anywhere on the
   screen or pressing any key."** Done, as a Razdor choice: checked under Wine on РК1, the
   original never stops a wait that way (a 4-hour wait ran its 240 minutes after a left
   click on the map or the bar, a right click, A or Space; the endless wait ends only by
   F5). In Razdor a left click anywhere or a key press during a 1 h, 4 h or endless wait
   ends it after the half hour under way (`Game::cut_wait`); the click or key does nothing
   else (no walk order, window or hotkey: `widgets::swallow_input`).

## 2026-10-04, dt-original: other armies move roughly

1. **"Other armies on the map move too roughly: they stay in one place too long and then
   jump too far."** Done, as far as the original goes. Checked under Wine on РК1 (two 4-hour
   waits with the armies' step clock read from memory every few ms, and screenshots): in the
   original an army's step glides over its play time and **a step never reaches into the
   next tick** (its play time is clamped to what is left of the tick, 0x4a399c; drawn by
   0x4ad660). So an army whose bank pays a step only every few ticks also stands between its
   steps there, and one fast army crosses several cells in a long tick: Razdor already drew
   the same, and keeping that is parity. Two drawing details did differ and are fixed: the
   walk frames ran on the wall clock whenever an army had a path, so figures marched in place
   while time stood still or while they waited for their next step (the original's frames 3–6
   follow the game time, engine.md §7), and a step in place took no time on the figure's walk,
   so the steps after it came too early (world.md §5). If the motion still feels rough, the
   next thing to compare is the hero's walking pace against the AI's under the same walk
   (FINDINGS §5: the original's frame rate changes the AI's details). Commit 13a024d.

## 2026-10-04, dt-original: a building won from its garrison opens from outside

1. **"When I fight an enemy garrison in a building and win, I end up standing next to the
   building, not inside it. When I then click the building, I don't walk into it, its window
   just opens; I should walk to the building's cell and then its window opens."** Done.
   Checked under Wine on РК1 (`tools/difftest/rk1-ruins-won.jsonl`: the ruins 8, 2×2 at
   (36,23), won from (34,24)): (a) **standing next to it is the original**: after the result
   box the hero is still at (34,24), the ruins are his (owner 0) but not entered (the entered
   building 0x68dc74 stays none), no window opens; (b) a click on the ruins walks him onto the
   clicked cell (36,23) and the building window opens there (entered 8). Razdor kept him
   outside too but marked the building as entered and ran its events, so the click counted as
   "the building you stand in" and opened the window at once. Now the won building is not
   entered: its events wait until he walks in, and the click walks (world.md §7.2,
   battle.md §11). Commit eebe641.

## 2026-10-04, dt-original: a quest's places shown only after leaving the building

1. **"When I take a mission in the barracks, the map with the quest's places pops up not
   during the dialog but only after I close the barracks window; it should show when
   needed."** Done. Checked under Wine on РК1 (`tools/difftest/rk1-castle-quest.jsonl`: the
   castle's main hall, «Сообщение посыльного», lantern 2): **the original flies at the
   quest's OK**: its OK queues the glide, the reveal and the glide back (Event_Finish 0x4ab1ec
   → 0x4af96c, 0x4af83c), the screen switches to the world map while they play (camera
   y 440 → 264 → 440 in about 2 s), then the building window comes back on its tab, silent;
   closing it later moves nothing (Frida hooks on 0x4af96c/0x4af83c fire only in the OK's
   step; screenshot burst). Razdor waited for the building window to close, because the map
   frame that plays the flights did not run under it. Now the building window steps aside
   for the flights (input off meanwhile) and comes back as it was (`App::fly_from_building`;
   interface.md §9.8, events.md §10). The diff test got a `take` op for the main hall on both
   sides. Commit b41b916.

## 2026-10-03, dt-original: a hero class the map leaves out

1. **A player's report: "on 'Осмотр владений' I started as the Ranger though only the Knight
   was meant to be playable."** The original does not allow this in a new game: a class is
   offered only when its preset has a start cell, its portrait is disabled otherwise and takes
   no click or key, and the window opens on the first offered class (interface.md §5,
   saves-data.md §10.4; checked under Wine on Устье Трейна, whose archmage is left out).
   Razdor's hero window let every class be picked on every map; dt-original now offers only
   what the original offers. The original's real gap is a campaign: the next map keeps the
   class without checking that the map offers it, and the hero then starts at cell (0,0) of the
   empty preset. **When dt-feat merges dt-original, treat that as an original bug to fix:
   offer only the classes the map defines**, and on a campaign's next map that leaves the
   class out, do not drop the hero at (0,0) (for example, refuse the map in the editor's
   checks or start him on the first offered class's cell).

## 2026-10-03, dt-feat with the Community Update install

1. **Windows open on top of each other.** Several windows fire at once and stack, one over the
   other. To check: which windows (event messages, building windows, battle, reports at noon),
   in what order the original shows them, and whether it queues them one at a time. See
   interface.md (message boxes, the order of the world-map windows) and events.md (the ask/OK
   flow).
   Checked (2026-10-04, dt-original): the original shows one window at a time by
   construction: the event scan opens one event's window and runs again only when it is
   finished (or answered No); the noon report is part of that scan; a building reached as a
   window opens waits for it (0x4ed42c); a battle and its report come after the windows of
   that moment. Razdor queues its dialogs the same way (one shown, the next after it). To find
   what still differs, every action list of the diff-test runs so far (28 lists, about 550
   steps) was replayed in Razdor with its screen read at each step and set against the
   original's screen (event window, village, building, battle, map). Two differences, both
   fixed: (1) an event's window that cut the walk short inside the clicked building: the
   original opens the building's window after the OK (РК1, runs r3-c004157 and
   rk1-h2-minimap; 0x4aed41 → 0x4ae5d8, 0x4aed64), Razdor left the hero on the map;
   (2) after a heal, a raise, a purchase or a sale in the building window the original checks
   the events as the window closes (0x4ed440, 0x4b8f63), Razdor only at the next step. The
   other screen differences of those runs come from AI walks that part (FINDINGS §5) or from
   `battle_auto`. No window of dt-original was found open over another one; what was seen on
   dt-feat should be checked again after it merges. Commit 50dcb52.

2. **Entering a village must not make it the hero's.** The player only takes the village's
   money, and only if nobody else has taken it that day. This **contradicts the current spec**:
   world.md §6 and economy.md ("Entering a village", 0x4bbc84) say the original captures an
   unguarded village when the hero steps on it, and Razdor follows that. To check: re-read
   0x4bbc84 and the capture in world.md against the original under Wine (enter a village, look
   at its owner and its tribute; then let an AI army take the tribute first and enter on the
   same day). Fix whichever side is wrong, on both branches.
   Checked (2026-10-04, dt-original): **the original does capture the village**, and Razdor is
   left as the original. Read live from the original's building records (owner +0x124, stock
   +0x11e) in the diff test: the hero's step into a neutral village makes it the player's (ДС1
   village 13: 255 → 0; Проклятое озеро villages 2 and 30: 255 → 0), and an AI army's capture
   is undone the same way (`tools/difftest/rk1-village-taken.jsonl`, run n2-village-taken: on
   РК1 army 9 takes the hero's start village 6 at 13:00, owner 9, gold 60 → 0, mana 15 → 0;
   the hero walks in at 18:30 the same day: owner 0, the village window pays nothing, his gold
   stays 100). The "only if nobody else took it that day" part already holds: the tribute is
   the village's stock, which the army that came first emptied and which refills at midnight
   (economy.md §3). Razdor does the same (test
   `rk1_a_village_emptied_by_an_army_pays_the_hero_nothing_that_day`; in the free run the two
   games' AI walks part before the village, FINDINGS §5, so army 9 meets Razdor's hero on the
   way). If the wish stands (villages never change hands for the player), it is a change of
   the original's rules, for dt-feat. Commit a63970b.

3. **Missing animations: units in battle and levelling up.** Fights lack the units' animations,
   and a level-up has none. To check: which battle animations the original plays (attack,
   shot, spell, hit, death) and the level-up effect, from the install's art (Graphics/Battle,
   Graphics/Spells) and interface.md / engine.md (animation timings). Presentation was left out
   of the parity pass on purpose, so this is open work, not a regression.
   Measured (tools/difftest/AV.md): the original has no animated unit figures; its battle
   effects match Razdor's one for one except the counterblow's slide back and effect, and the
   level-up shows only in the won battle's 2.5 s hold (experience cards) and the promotion
   screen; the hold is missing in Razdor.
   Done (2026-10-04, dt-original): the counterblow's lunge back with its effect and sound on
   the attacker (and the sorcery on a killer a DeathCurse unit takes along), the won battle's
   2.5 s hold with the experience on the cards and no result box, a pass's 100 ms pause, and
   no level-up sound outside the promotion screen. The AV runs now match the original's battle
   sounds and effects step for step (AV.md); unit sprites are not part of the original.

4. **No ranged defence (Защита стрелковая) on the back row.** Seen in battle: units in the back
   row show or get no ranged defence. The spec says the original adds Row2Def (+5 in the
   shipped `_Global.ini`) to a row-2 target's defence against shots, after any piercing, in the
   damage formula (battle.md, Row 2 defence, 0x485a04). To check: whether Razdor applies the +5
   in the damage (a test on a row-2 target hit by a shot), and whether the original shows it on
   the unit card and panel while Razdor does not (display only). Compare the card of the same
   back-row unit in both games.
   First finding: Razdor does apply it in the damage (`src/rules/battle.rs`, `row2_def` added for
   a row-2 target of a shot), so this is most likely the card and panel not showing the bonus.
   Done (2026-10-04, dt-original): the original adds Row2Def to what it shows of a unit in a
   back-row place (7-10) on its card strip ("D: m/r", 0x49462c) and on its panel ("v + n", n =
   building defence + Row2Def, 0x492f24 sets the row flag, 0x491fa4 writes it), in battle and
   on the army and building screens (not on a recruit offer). Seen under Xvfb in РК1's ruins
   battle: the novice and the archer of the back row show "D: 0/5", the panel "5 + 3" for a
   guard in its building. Razdor now shows the same on the card strips (battle, army, building
   windows) and in the panel's ranged defence line; the damage was already right. Commit f5ed454.

5. **Feature request: a setting for the front row's width.** In the settings, a choice between a
   wide front row (6 cells) and a short one (4 cells). With the short row, the 2 edge cells of the
   front row become inactive cells, as the back row's edge cells already are. Today the width
   comes from the install's `OptValue11` (wide by default, see the restored "wide row" choice and
   battle.md §6), with no in-game switch. To work out: where the setting lives (Razdor's
   `settings.json` vs the install's option), whether it applies to a battle or a whole game
   (saves record the row width), and how the reserve row changes with it.
   Done (2026-10-04, dt-original): the short row is the original's own 4-column formation
   (`OptValue11` = 0): front 4, back 4, reserve 4, and the original draws it on the same 2 × 6
   places (0x492940): front and back rows in the middle four, the reserve's four cells at the
   ends of both lines, so the front row's edge places are inactive reserve cells exactly as
   asked. The width is a whole game's (stored in the save at a new game, 0x4b25a2). Razdor's
   settings window now has "Front row in battle (new games)": 6 or 4 cells, kept in
   `audio.json` with the other settings (`wide_row`; until chosen the install's `OptValue11`),
   applied to games started afterwards; saves keep their width. The 4-column formation is now
   drawn as the original's places (it was three lines of four). Commit 7019060.

6. **The camera jumps back to the hero on the first click.** With the hero off screen (the map
   scrolled away), a single click on a place moves the view straight back to the hero. Wanted:
   the view stays where it is while the route is chosen, so a second click on the same place
   (the route preview's confirm, a double click) can be made there; only once the hero starts
   walking does the view go back to him. To check: what the original does (whether its camera
   follows the hero only while he walks), and in Razdor the camera-follow logic in
   `src/ui/world_view.rs` (the camera following the hero unless moved by the minimap) and the
   route preview's first click.
   Done (2026-10-04, dt-original): the original does what is wanted. Its click handler writes
   the camera only for a minimap drag and the arrow keys (0x4ccf5a-0x4cd00f); only the walk
   locks the view on the hero (0x4ae8a8). Checked under Wine on РК1: with the view scrolled
   400 px off the hero, the first click drew the route and left the camera at (210, 440); the
   second set him off and the camera went to (608, 440). Razdor reset the view on every
   click on a target; now a click leaves it and the walk brings it back (`camera_look`), so
   this is parity, not a Razdor choice. Commit 60e1eaf.

7. **Second campaign map: the "send the peasants to the mines" offers.** There are three offers to
   send a group of peasants to the mines. The player accepted two and declined one. The declined
   offer never came back, although all three should be accepted (the declined one asked again).
   After the second accepted group, the quest was reported as completed, although only two of
   three groups were sent. To check, on both branches: the events behind these offers on the
   second map (their repeat and once flags, the "No" result, the follow-up and the quest's
   completion condition), against events.md (the ask / Yes / No flow, which results apply on No,
   repeats and the once flag) and the original under Wine. Related: dt-feat fixed the original's
   bug "a repeating question without a message asks again next time" (events step), so the
   branches may differ here; and whether the quest's completion counts groups sent or fires on
   another condition.
   **To fix** (the user, 2026-10-03): the quest must follow the original's offers and completion.
   Checked (2026-10-04, dt-original): Razdor already follows the original here; no code change.
   The map's events (village building 4): offer 8 asks with the baron's promise (5) answered
   Yes; offer 9 needs Yes to 5 and 8 and, on its Yes, opens offer 10 a day later; 10 asks only
   while the army has no peasant left (three "not the player's" unit slots) and the quest's end
   (27) has not fired: it is a replacement, not a third group. 8, 9 and 10 are many-times
   events with a message. In the original a No only counts the firing (answer 1, times + 1,
   last fired = now + 1: 0x4c2320), so a declined offer is not asked again in that visit and is
   asked again when the hero next enters the village; a Yes makes it a once-event (0x4c2100).
   Each mine's fort takes three peasants (19, 24; "three per mine" in the baron's own words),
   each completing its mine's quest (18, 23), and 27 completes the campaign quest (4) only
   after both: two groups of three are the whole task, so the quest done after the second
   accepted group is the original's. The original cannot start РК2 outside the campaign (New
   game lists only first maps), so this was checked against its code (events.md §2, §6.2) and
   the map file, and played in Razdor with the replay's carry-over (the herald now carried by
   `named`): test `rk2_the_peasant_offers_and_the_mines` declines 8 and 9 and gets them back on
   the next visit, staffs the north mine (quest 18 done, 4 not), loses the other three, gets
   offer 10 a day later and staffs the south mine (27 fires, quest 4 done). What dt-feat
   changed for repeating questions should be checked against this test when it merges.
   Commit 0d4057a.

**Update 2026-10-04 (note 5):** at the player's request the setting now applies to the game under
way too, from its next battle (not during one); its saves record the new width. Units on cells
the narrower row lacks move to free cells, their own row first (`Game::set_formation`).
