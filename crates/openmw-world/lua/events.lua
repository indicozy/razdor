-- The event engine of the converted map (M5, spec 3.11, 3.12; events.md §1-§10). Pure logic: it
-- reads map_data.lua and the saved state `s`, and reaches the world only through `env`, which the
-- global script builds (clock, armies, windows, journal, end of game). Written for Lua 5.1 (OpenMW
-- 0.51): no integer division, no goto.
--
-- Log lines (print): RAZDOR_EVENT <id> at every firing, in order; RAZDOR_ANSWER <id> <yes|no|ok>;
-- RAZDOR_NOTE <id> <what> for a result this version does not play; RAZDOR_FLAG for a flag action;
-- RAZDOR_VICTORY and RAZDOR_DEFEAT when the map's end event fires.
local map = require('scripts.razdor.map_data')

local M = {}

-- Event kinds (the exporter's `kind`): 1 global, 2 local, 3 quest, 4 rumour.
local KIND_GLOBAL, KIND_LOCAL, KIND_QUEST, KIND_RUMOUR = 1, 2, 3, 4
-- Building kinds whose every event counts (village 2, shipyard 9, events.md §2).
local BUILDING_ALL_EVENTS = { [2] = true, [9] = true }
local JOURNAL_TAKEN, JOURNAL_FINISHED = 10, 100
local STEP_LIMIT = 200

local by_id = {}
local point_by_id = {}
local point_at = {}
local zone_at = {}
local zone_by_index = {}
local army_by_id = {}

for _, ev in ipairs(map.events) do
    by_id[ev.id] = ev
end
for _, p in ipairs(map.points) do
    point_by_id[p.id] = p
    point_at[p.x .. ',' .. p.y] = p
end
for _, z in ipairs(map.zones) do
    zone_by_index[z.index] = z
    for _, t in ipairs(z.tiles) do
        zone_at[t[1] .. ',' .. t[2]] = z
    end
end
for _, a in ipairs(map.armies) do
    army_by_id[a.id] = a
end

local function num(v)
    return v or 0
end

-- The saved state of the engine, set up once when the game starts (M.init).
function M.new_state()
    return {
        ready = false,
        fired = {},      -- event id -> times fired
        last = {},       -- event id -> last fired (DT minutes)
        answer = {},     -- event id -> 0 (Yes or none) or 1 (No), cleared on opening
        once = {},       -- event id -> once override set by a Yes
        start = {},      -- event id -> start override (relative events)
        radius = {},     -- army id -> patrol radius, once a patrol result changed it (tiles)
        hostile = {},    -- army id -> true once a battle made it hostile (spec 3.8)
        artifacts = {},  -- the hero's pack: artifact ids, one entry per copy (events.md 7.1)
        spells = {},     -- spell ids the hero has learned (events.md 7.1)
        class_unit = nil, -- the unit id the hero's class was changed to (events.md 7.2, 16)
        flags = ' ',     -- the flag string: names followed by a space (events.md §5)
        gold = 0,
        mana = 0,
        exp = 0,
        level = 0,
        party = {},      -- { unit, level, count, named } placeholders for the hero's units
        owner = {},      -- building index -> 'player' when the player owns it
        point = nil,     -- the point id under the player (noted on each step)
        inside = nil,    -- the building index the player stands in
        offer = false,   -- the building's events are offered until the scan goes idle
        met = nil,       -- the army met right now (cleared when the scan goes idle)
        held = {},       -- army id -> true while a meet event waits for it
        next_scan = nil, -- DT minutes of the next clock scan
        wid = 0,         -- the last window id
        pending = nil,   -- the open window: { id, stage, wid }
        ended = false,
    }
end

-- Sets up the state at the game start: gold, mana, the party and the start building (spec 3.9).
function M.init(s, now)
    if s.ready then
        return
    end
    s.ready = true
    local h = map.hero
    s.gold = h.gold
    s.mana = h.mana
    for _, u in ipairs(h.party) do
        table.insert(s.party, { unit = u.unit, level = u.level, count = u.count, named = u.named })
    end
    if h.start_building ~= 0 then
        s.owner[h.start_building] = 'player'
    end
    s.next_scan = math.floor(now / 30) * 30 + 30
end

-- The flag test of events.md §5: a substring of the flag string, with the empty test passing.
local function occurs(s, text)
    return string.find(s.flags, text, 1, true) ~= nil
end

local function flag_test_ok(s, ev)
    local f = ev.flags
    if not f then
        return true
    end
    if f.require_set and f.require_set ~= '' and f.require_set:find('end_tutorial', 1, true) == nil then
        if not occurs(s, f.require_set) then
            return false
        end
    end
    if f.require_unset and f.require_unset ~= '' and f.require_unset:find('end_tutorial', 1, true) == nil then
        if occurs(s, f.require_unset) then
            return false
        end
    end
    return true
end

-- The flag action of a finished event (events.md §5, 0x4ab2a3). Counters (`^`) are not played.
local function flag_action(s, ev)
    local f = ev.flags
    if not f then
        return
    end
    if f.set and f.set ~= '' then
        if string.find(f.set, '^', 1, true) then
            print('RAZDOR_NOTE ' .. ev.id .. ' counter flags are not played')
        elseif not occurs(s, f.set) then
            s.flags = s.flags .. f.set .. ' '
            print('RAZDOR_FLAG ' .. ev.id .. ' set ' .. f.set)
        end
    end
    if f.clear and f.clear ~= '' then
        if string.find(f.clear, '^', 1, true) then
            print('RAZDOR_NOTE ' .. ev.id .. ' counter flags are not played')
        else
            local i = string.find(s.flags, f.clear, 1, true)
            if i then
                s.flags = s.flags:sub(1, i - 1) .. s.flags:sub(i + #f.clear + 1)
                print('RAZDOR_FLAG ' .. ev.id .. ' clear ' .. f.clear)
            end
        end
    end
end

-- A signed numeric test (events.md §4, 11): v > 0 needs at least v, v < 0 at most -v.
local function signed_ok(v, actual)
    if v > 0 then
        return actual >= v
    elseif v < 0 then
        return actual <= -v
    end
    return true
end

local function party_has(s, unit, named)
    for _, u in ipairs(s.party) do
        if u.unit == unit and (named == 0 or u.named == named) then
            return true
        end
    end
    return false
end

local function party_count(s)
    local n = 0
    for _, u in ipairs(s.party) do
        n = n + num(u.count)
    end
    return n
end

-- The artifacts condition (events.md §4, 14): each slot with an id looks in the tracked pack; a copy
-- found is taken aside for the rest of the check. Code 1 needs it, 6 needs none. Codes 2-5 (the AI
-- armies' worn items) and code 0 fail: those items are not tracked.
local function artifacts_ok(s, c)
    local taken = {}
    for i = 1, 3 do
        local id = num(c.artifacts[i])
        if id ~= 0 then
            local code = num(c.artifacts_owner[i])
            local found = nil
            for k, v in ipairs(s.artifacts) do
                if v == id and not taken[k] then
                    found = k
                    break
                end
            end
            if code == 1 then
                if not found then
                    return false
                end
                taken[found] = true
            elseif code == 6 then
                if found then
                    return false
                end
            else
                return false
            end
        end
    end
    return true
end

-- Whether an army is hostile now: the map's attitude, or a battle that made it so (spec 3.8).
function M.is_hostile(s, a)
    if s.hostile[a.id] then
        return true
    end
    return a.hostile and true or false
end

-- An army's patrol radius now: the map's, or the one a patrol result set (events.md 7.2, 8).
function M.patrol_radius(s, a)
    return s.radius[a.id] or a.patrol_radius
end

-- The units condition of one slot (events.md §4, 13): code 1 needs the unit, 6 needs none, 2-5
-- look in the armies of faction code - 1 (their start units). Code 0 fails.
local function unit_slot_ok(s, c, i)
    local unit = num(c.units[i])
    if unit == 0 then
        return true
    end
    local named = num(c.units_named[i])
    local code = num(c.units_owner[i])
    if code == 1 then
        return party_has(s, unit, named)
    elseif code == 6 then
        return not party_has(s, unit, named)
    elseif code >= 2 and code <= 5 then
        for _, au in pairs(map.army_units) do
            if au.faction == code - 1 then
                for _, u in ipairs(au.units) do
                    if u == unit then
                        return true
                    end
                end
            end
        end
        return false
    end
    return false
end

-- The building owner slot (events.md §4, 12): code 1 the player owns it, 6 he does not, 2-5 the
-- building's faction is code - 1 (its start faction). A slot with code 0 is skipped.
local function building_slot_ok(s, c, i)
    local idx = num(c.buildings[i])
    local code = num(c.buildings_owner[i])
    if idx == 0 or code == 0 then
        return true
    end
    local owned = s.owner[idx] == 'player'
    if code == 1 then
        return owned
    elseif code == 6 then
        return not owned
    elseif code >= 2 and code <= 5 then
        local z = zone_by_index[idx]
        return z ~= nil and z.faction == code - 1
    end
    return true
end

-- The conditions of an event (events.md §4). `skip_meet` leaves out the meet-army test, for the
-- "meeting event waiting" mark (§9). The army strength is not tracked yet (the unit costs are not in
-- the plugin's data); the artifacts are (the tracked pack).
local function conditions_ok(env, s, ev, skip_meet)
    local c = ev.conditions or {}
    if not flag_test_ok(s, ev) then
        return false
    end
    if num(c.defeated_check) ~= 0 then
        for _, a in ipairs(c.defeated_armies) do
            if a ~= 0 and env.army_status(a) ~= 'defeated' then
                return false
            end
        end
    end
    if num(c.beaten_check) ~= 0 then
        for _, a in ipairs(c.beaten_armies) do
            if a ~= 0 and env.army_status(a) ~= 'defeated' then
                return false
            end
        end
    end
    if num(c.happened_yes_check) ~= 0 then
        for _, id in ipairs(c.happened_yes) do
            if id ~= 0 and not ((s.fired[id] or 0) > 0 and num(s.answer[id]) ~= 1) then
                return false
            end
        end
    end
    if num(c.happened_no_check) ~= 0 then
        for _, id in ipairs(c.happened_no) do
            if id ~= 0 and num(s.answer[id]) ~= 1 then
                return false
            end
        end
    end
    if num(c.not_happened_check) ~= 0 then
        for _, id in ipairs(c.not_happened) do
            if id ~= 0 and (s.fired[id] or 0) ~= 0 then
                return false
            end
        end
    end
    if num(c.army_active) ~= 0 and env.army_status(c.army_active) ~= 'active' then
        return false
    end
    if num(c.army_inactive) ~= 0 and env.army_status(c.army_inactive) ~= 'inactive' then
        return false
    end
    if num(c.army_at_home) ~= 0 and not env.army_at_home(c.army_at_home) then
        return false
    end
    if not skip_meet and num(c.meet_army) ~= 0 and s.met ~= c.meet_army then
        return false
    end
    if num(c.stats_check) ~= 0 then
        if not signed_ok(num(c.level), s.level) or not signed_ok(num(c.gold), s.gold)
            or not signed_ok(num(c.holiness_mana), s.mana) or not signed_ok(num(c.squad_count), party_count(s)) then
            return false
        end
    end
    if num(c.buildings_check) ~= 0 then
        for i = 1, 3 do
            if not building_slot_ok(s, c, i) then
                return false
            end
        end
    end
    if num(c.units_check) ~= 0 then
        for i = 1, 3 do
            if not unit_slot_ok(s, c, i) then
                return false
            end
        end
    end
    if num(c.artifacts_check) ~= 0 and not artifacts_ok(s, c) then
        return false
    end
    return true
end

-- The time window and the firing guard (events.md §3). `now` is DT minutes.
local function eligible(env, s, ev)
    if num(ev.subordinate) ~= 0 then
        return false
    end
    local now = env.now()
    local id = ev.id
    local dur = num(ev.duration)
    local last = s.last[id] or 0
    if dur ~= 0 then
        if last > now then
            return false
        end
    elseif last >= now - 60 then
        return false
    end
    local start = s.start[id] or num(ev.start_time)
    if start < 0 then
        start = 0
    end
    if start > now then
        return false
    end
    local fired = s.fired[id] or 0
    local hours = math.max(dur, 1)
    local repeat_min = num(ev['repeat'])
    if repeat_min ~= 0 then
        local days = math.floor(repeat_min / 1440)
        if days < 1 then
            days = 1
        end
        local k = math.floor((now - start) / 1440)
        local day = start + 1440 * k
        if k % days ~= 0 or now < day or now > day + 60 * hours then
            return false
        end
    elseif dur == 0 then
        if fired ~= 0 then
            return false
        end
    elseif now > start + 60 * hours then
        return false
    end
    local once = s.once[id]
    if once == nil then
        once = num(ev.once)
    end
    if once ~= 0 and fired ~= 0 then
        return false
    end
    if num(ev.archetype) ~= 0 and ev.archetype ~= map.hero.class then
        return false
    end
    return true
end

-- The events a scan may take, in order (events.md §2): the global events in file order, the
-- events of the point under the player, then the events of the building he stands in.
local function candidates(s)
    local list = {}
    for _, ev in ipairs(map.events) do
        if ev.kind == KIND_GLOBAL then
            table.insert(list, ev.id)
        end
    end
    if s.point then
        local p = point_by_id[s.point]
        for _, id in ipairs(p and p.events or {}) do
            table.insert(list, id)
        end
    end
    if s.offer and s.inside then
        local z = zone_by_index[s.inside]
        for _, id in ipairs(z and z.events or {}) do
            local ev = by_id[id]
            if ev and (ev.kind == KIND_LOCAL or BUILDING_ALL_EVENTS[z.kind]) then
                table.insert(list, id)
            end
        end
    end
    return list
end

local finish, take

-- Opens a window (message or question) and stops the scan until it closes.
local function show(env, s, ev, stage, text, buttons)
    s.wid = s.wid + 1
    s.pending = { id = ev.id, stage = stage, wid = s.wid }
    env.show({ wid = s.wid, kind = stage, title = env.title(ev), text = env.text(text), buttons = buttons })
    return 'shown'
end

-- The hall of an ordinary building (events.md §10, spec 3.11 "Hall"): its quests and rumours that pass
-- the full check, in list order. Quests are listed first; a rumour is listed too and opens its text
-- when picked, as a quest does (a rumour has no journal entry).
local function hall_list(env, s, z)
    local quests, rumours = {}, {}
    for _, id in ipairs(z.events or {}) do
        local ev = by_id[id]
        if ev and (ev.kind == KIND_QUEST or ev.kind == KIND_RUMOUR) and eligible(env, s, ev)
            and conditions_ok(env, s, ev, false) then
            table.insert(ev.kind == KIND_QUEST and quests or rumours, ev)
        end
    end
    return quests, rumours
end

-- Opens the hall window: one button per quest ("Взять") and per rumour ("Слух"), then Close. The
-- answer 'take:<id>' takes the event at once, with no new check (events.md §10).
local function show_hall(env, s, z, quests, rumours)
    s.wid = s.wid + 1
    s.pending = { id = nil, stage = 'hall', wid = s.wid }
    local buttons = {}
    for _, ev in ipairs(quests) do
        table.insert(buttons, { answer = 'take:' .. ev.id, label = 'Взять: ' .. env.title(ev) })
    end
    for _, ev in ipairs(rumours) do
        table.insert(buttons, { answer = 'take:' .. ev.id, label = 'Слух: ' .. env.title(ev) })
    end
    table.insert(buttons, { answer = 'close', label = 'Закрыть' })
    print(string.format('RAZDOR_HALL %d quests=%d rumours=%d', z.index, #quests, #rumours))
    env.show({ wid = s.wid, kind = 'hall', title = 'Главный зал', text = '', buttons = buttons })
    return 'shown'
end

-- The dialog-time results (events.md §7.1): the units added and removed. The artifacts and the
-- spells are not played (v1).
local function index_of(list, id)
    for k, v in ipairs(list) do
        if v == id then
            return k
        end
    end
    return nil
end

local function dialog_results(s, ev)
    local r = ev.results or {}
    -- DT's order (events.md 7.1): artifacts gained, artifacts lost, units added, units removed, spells.
    for i = 1, 4 do
        local id = num(r.artifacts_add and r.artifacts_add[i])
        if id ~= 0 then
            table.insert(s.artifacts, id)
            print('RAZDOR_HERO artifact+ ' .. id)
        end
    end
    for i = 1, 4 do
        local id = num(r.artifacts_remove and r.artifacts_remove[i])
        local k = id ~= 0 and index_of(s.artifacts, id) or nil
        if k then
            table.remove(s.artifacts, k)
            print('RAZDOR_HERO artifact- ' .. id)
        end
    end
    for i = 1, 4 do
        local unit = num(r.units_add and r.units_add[i])
        if unit ~= 0 then
            table.insert(s.party, { unit = unit, level = 0, count = 1, named = num(r.units_add_named[i]) })
            print('RAZDOR_UNIT_ADD ' .. ev.id .. ' unit=' .. unit)
        end
    end
    local removed = false
    for i = 1, 4 do
        local unit = num(r.units_remove and r.units_remove[i])
        if unit ~= 0 then
            local named = num(r.units_remove_named[i])
            for j = #s.party, 1, -1 do
                local u = s.party[j]
                if u.unit == unit and (named == 0 or u.named == named) then
                    table.remove(s.party, j)
                    removed = true
                    break
                end
            end
        end
    end
    if removed then
        print('RAZDOR_UNIT_REMOVE ' .. ev.id)
    end
    -- Spells are learned once each (events.md 7.1: an already known one is not appended again).
    for i = 1, 4 do
        local id = num(r.spells_learned and r.spells_learned[i])
        if id ~= 0 and not index_of(s.spells, id) then
            table.insert(s.spells, id)
            print('RAZDOR_HERO spell+ ' .. id)
        end
    end
end

-- Opens an event (events.md §6.1): a question first when it asks, else its results and its message.
-- A silent event is finished at once. Returns 'shown' or 'done'.
take = function(env, s, ev)
    print('RAZDOR_EVENT ' .. ev.id)
    s.answer[ev.id] = nil
    local asks = num(ev.conditions and ev.conditions.confirm_question) ~= 0 and ev.question ~= ''
    local text = asks and ev.question or ev.message
    if text == '' then
        dialog_results(s, ev)
        return finish(env, s, ev)
    end
    if asks then
        return show(env, s, ev, 'question', ev.question, { 'yes', 'no' })
    end
    dialog_results(s, ev)
    return show(env, s, ev, 'message', ev.message, { 'ok' })
end

-- The finish-time results (events.md §7.2) in the file's order, then the victory and defeat check
-- and the chain (§8). Returns 'shown' or 'done'.
finish = function(env, s, ev)
    local r = ev.results or {}
    local now = env.now()
    local id = ev.id
    if num(r.no_meeting) ~= 0 then
        s.met = nil
    end
    s.last[id] = now + 1
    s.fired[id] = (s.fired[id] or 0) + 1
    flag_action(s, ev)
    s.exp = s.exp + num(r.experience)
    s.gold = math.max(0, s.gold + num(r.gold))
    s.mana = math.max(0, s.mana + num(r.mana))
    for i = 1, 2 do
        local a = num(r.activate_armies and r.activate_armies[i])
        if a ~= 0 then
            env.activate(a)
        end
    end
    if num(r.deactivate_army) ~= 0 then
        env.deactivate(r.deactivate_army)
    end
    if num(r.patrol_army) ~= 0 then
        M.patrol(s, num(r.patrol_army), num(r.patrol_delta))
    end
    if num(r.show_army) ~= 0 then
        env.show_army(num(r.show_army))
    end
    if num(r.move_to_hero) ~= 0 then
        env.move_to_hero(num(r.move_to_hero))
    end
    if num(r.new_hero_class) ~= 0 then
        s.class_unit = num(r.new_hero_class)
        print('RAZDOR_HERO class unit=' .. s.class_unit)
    end
    if num(r.start_battle_with) ~= 0 then
        M.battle(env, s, num(r.start_battle_with))
    end
    if num(r.cast_spell) ~= 0 then
        print('RAZDOR_NOTE ' .. id .. ' cast_spell is not played')
    end
    if num(r.light_lanterns and r.light_lanterns[1]) ~= 0 then
        print('RAZDOR_NOTE ' .. id .. ' lanterns are not lit (Q7)')
    end
    if ev.kind == KIND_QUEST then
        env.journal(id, JOURNAL_TAKEN)
    end
    if num(r.completes_quest) ~= 0 then
        env.journal(r.completes_quest, JOURNAL_FINISHED)
    end
    if num(r.relative_event) ~= 0 then
        s.start[r.relative_event] = now + 60 * num(r.relative_delay_hours)
    end
    local info = map.info
    if id == info.victory or id == info.defeat then
        local victory = id == info.victory
        s.ended = true
        print((victory and 'RAZDOR_VICTORY ' or 'RAZDOR_DEFEAT ') .. id)
        local next_name = victory and info.next_map or ''
        s.wid = s.wid + 1
        s.pending = { id = id, stage = 'end', wid = s.wid }
        env.show({ wid = s.wid, kind = 'end', victory = victory, title = env.title(ev), next_map = next_name, buttons = { 'quit' } })
        return 'shown'
    end
    if num(r.chained_event) ~= 0 then
        local c = by_id[r.chained_event]
        if c then
            -- A chained event is opened as it is: its guards and conditions are not checked.
            return take(env, s, c)
        end
    end
    return 'done'
end

-- Runs the scan (events.md §2): the first passing candidate is taken; a silent one is finished
-- and the scan starts again; a window stops it until its answer. `s.met` is cleared when it goes
-- idle, as are the building offer and the held marks are recomputed.
function M.scan(env, s)
    if s.pending or s.ended then
        return
    end
    for _ = 1, STEP_LIMIT do
        local taken = false
        for _, id in ipairs(candidates(s)) do
            local ev = by_id[id]
            if ev and eligible(env, s, ev) and conditions_ok(env, s, ev, false) then
                if take(env, s, ev) == 'shown' then
                    return
                end
                taken = true
                break
            end
        end
        if not taken then
            break
        end
    end
    -- Idle, in an ordinary building the player just entered: its hall is offered (once, since the offer ends here).
    if s.offer and s.inside then
        local z = zone_by_index[s.inside]
        if z and not BUILDING_ALL_EVENTS[z.kind] then
            local quests, rumours = hall_list(env, s, z)
            if #quests + #rumours > 0 then
                s.offer = false
                s.met = nil
                return show_hall(env, s, z, quests, rumours)
            end
        end
    end
    s.met = nil
    s.offer = false
    M.refresh_holds(env, s)
end

-- The answer of a window (from the player's buttons, or a test driver): 'yes', 'no' or 'ok'.
-- Returns true when the scan may run again.
function M.answer(env, s, wid, answer)
    local p = s.pending
    if not p or p.wid ~= wid then
        return false
    end
    s.pending = nil
    env.close(wid)
    if p.stage == 'hall' then
        -- A quest or rumour picked in the hall is taken at once; Close (or anything else) ends the hall.
        local id = tonumber(string.match(answer, '^take:(%d+)$') or '')
        local picked = id and by_id[id]
        if picked then
            print('RAZDOR_HALL take ' .. id)
            if take(env, s, picked) ~= 'shown' then
                M.scan(env, s)
            end
        else
            M.scan(env, s)
        end
        return
    end
    local ev = by_id[p.id]
    print('RAZDOR_ANSWER ' .. p.id .. ' ' .. answer)
    if p.stage == 'end' then
        env.quit()
        return false
    end
    local result
    if p.stage == 'question' then
        if answer == 'no' then
            -- A No is a firing with no results and no chain (events.md §6.2).
            s.answer[p.id] = 1
            s.last[p.id] = env.now() + 1
            s.fired[p.id] = (s.fired[p.id] or 0) + 1
            M.scan(env, s)
            return
        end
        s.answer[p.id] = 0
        s.once[p.id] = (num(ev.results and ev.results.repeat_after_yes) ~= 0) and 0 or 1
        if ev.message == '' then
            result = finish(env, s, ev)
        else
            dialog_results(s, ev)
            result = show(env, s, ev, 'message', ev.message, { 'ok' })
        end
    else
        result = finish(env, s, ev)
    end
    if result ~= 'shown' then
        M.scan(env, s)
    end
end

-- The player stepped onto a tile (spec 3.11, the step): notes the point and the building under
-- him, then scans. A building's events are offered when he steps into its footprint.
function M.step(env, s, tx, ty)
    local key = tx .. ',' .. ty
    local p = point_at[key]
    s.point = p and p.id or nil
    local z = zone_at[key]
    local zi = z and z.index or nil
    if zi ~= s.inside then
        s.inside = zi
        s.offer = zi ~= nil
    end
    M.scan(env, s)
end

-- The clock: a scan every 30 DT minutes (spec 3.11, the clock).
function M.tick(env, s)
    local now = env.now()
    if s.next_scan and now >= s.next_scan then
        s.next_scan = math.floor(now / 30) * 30 + 30
        M.scan(env, s)
    end
end

-- A meeting: the player came within a tile of an army's leader (spec 3.11, 9).
function M.meet(env, s, army)
    s.met = army
    M.scan(env, s)
end

-- Whether a meet-army event that could fire now still waits for this army (events.md §9): every
-- condition but the meeting itself holds and the event has not fired.
local function meet_waiting(env, s, army)
    for _, ev in ipairs(map.events) do
        local c = ev.conditions or {}
        if num(c.meet_army) == army and num(ev.subordinate) == 0 and (s.fired[ev.id] or 0) == 0 then
            if conditions_ok(env, s, ev, true) then
                return true
            end
        end
    end
    return false
end

-- A patrol result (events.md 7.2, 8): the radius changes by the delta, never below 0.
function M.patrol(s, id, delta)
    local a = army_by_id[id]
    if not a then
        return
    end
    local radius = math.max(0, M.patrol_radius(s, a) + delta)
    s.radius[id] = radius
    print(string.format('RAZDOR_PATROL %d radius=%d', id, radius))
end

-- A battle result (events.md 7.2, 15), played at once: the army is made hostile (saved), activated
-- if it is off the map, and moved next to the hero. A waiting meet event still holds its fire at 0.
function M.battle(env, s, id)
    if not army_by_id[id] then
        return
    end
    s.hostile[id] = true
    print('RAZDOR_BATTLE ' .. id)
    env.battle(id)
    local held = meet_waiting(env, s, id)
    s.held[id] = held or nil
    env.fight(id, held and 0 or 100)
end

-- The hostile armies that a waiting meet event holds at fight 0 (spec 3.8, the held fire).
function M.refresh_holds(env, s)
    for _, a in ipairs(map.armies) do
        if M.is_hostile(s, a) then
            local held = meet_waiting(env, s, a.id)
            if held ~= (s.held[a.id] or false) then
                s.held[a.id] = held or nil
                env.fight(a.id, held and 0 or 100)
            end
        end
    end
end

-- The windows' text: the hero's placeholder is filled (events.md §6.4). The DT line break `#\`
-- and the colour and centring markers are not shown here.
function M.plain_text(text, hero_name)
    local t = text:gsub('#HERONAME', hero_name)
    t = t:gsub('#\\', '\n')
    return t
end

return M
