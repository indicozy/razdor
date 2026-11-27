-- Global script of the converted map (M4, M5; spec 3.8, 3.9, 3.11, 3.12): the start, the game clock,
-- the armies (spawn, attitude, patrol orders, defeat, respawn), the event engine (events.lua) and the
-- passability guard's teleport back. Written for OpenMW 0.51. Log lines (print) are the ones the smoke
-- test reads: RAZDOR_START, RAZDOR_CLOCK, RAZDOR_ARMY_SPAWNED, RAZDOR_ARMY_SKIPPED, RAZDOR_CHECK,
-- RAZDOR_FIGHT, RAZDOR_BODY_DEAD, RAZDOR_ARMY_DEFEATED, RAZDOR_MEET, RAZDOR_EVENT, RAZDOR_ANSWER,
-- RAZDOR_VICTORY, RAZDOR_DEFEAT (events.lua has the rest).
local world = require('openmw.world')
local types = require('openmw.types')
local util = require('openmw.util')
local core = require('openmw.core')
local common = require('scripts.razdor.common')
local map = require('scripts.razdor.map_data')
local events = require('scripts.razdor.events')

local ARMY_SCRIPT = 'scripts/razdor/army.lua'
local DAY_SECONDS = 86400 -- one DT day is 24 game hours (spec 3.11)
local SPACING = map.spacing
local FIGHT_HOSTILE = 100 -- the AI fight value of a hostile body (spec 3.8)
local MEET_UNITS = 1024 -- within one tile of the leader (spec 3.11, the meeting)
local HERO_NAME = 'Герой' -- the typed name is not known to the converter (events.md §6.4)

-- Saved state: numbers, booleans and tables of them only (game objects are not saved).
-- Per army: spawned, defeated, gen (the spawn generation), n (its bodies), dead (body k -> true).
-- ev: the event engine's state (events.lua); t0: the game time at the start (the DT clock).
local state = { started = false, armies = {}, ev = events.new_state(), t0 = 0 }

-- Runtime only (not saved): the body objects of each army, by body number k. The bodies report
-- themselves again on every load (RazdorBodyReady), so this table is rebuilt after a reload.
local bodies = {}
local contact = {} -- army id -> true while the player is within a tile of its leader
-- The real-time fight driver (M8, fight.lua): the sides of the bodies by object id, the test flags
-- (player = false: the player is no target, RazdorFightConfig from the smoke) and the end, logged once.
-- Runtime only: a reload rebuilds the sides from the bodies' ready events.
local fight_sides = {}
local fight_cfg = { player = true }
local fight_ended = false

local by_id = {}
for _, a in ipairs(map.armies) do
    by_id[a.id] = a
end

local acc = 0

local function army_state(id)
    local st = state.armies[id]
    if not st then
        st = { spawned = false, defeated = false, gen = 0, n = 0, dead = {} }
        state.armies[id] = st
    end
    return st
end

local function ensure_bodies(id)
    local b = bodies[id]
    if not b then
        b = { actors = {} }
        bodies[id] = b
    end
    return b
end

local function set_clock()
    local ok = pcall(world.setGameTimeScale, map.time_scale)
    print(string.format('RAZDOR_CLOCK scale=%.0f ok=%s', map.time_scale, tostring(ok)))
end

-- The DT clock (spec 3.11): DT minutes = info.start_time + (game time - game time at the start) / 60.
local function dt_now()
    return map.info.start_time + (core.getGameTime() - state.t0) / 60
end

local function player()
    return world.players[1]
end

-- Body offsets: the cells around the leader at Chebyshev distance 1, 2, ..., in a fixed order, so
-- every body stands SPACING units from its neighbours (spec 3.8, M4 follow-up).
local OFFSETS = {}
for r = 1, 8 do
    for dy = -r, r do
        for dx = -r, r do
            if math.max(math.abs(dx), math.abs(dy)) == r then
                OFFSETS[#OFFSETS + 1] = { dx, dy }
            end
        end
    end
end

-- Where the next follower stands: the first free offset, in order, that is an open tile outside
-- every building. `taken` holds the offsets already used by this army. Ships stand on the raft and
-- are not tested.
local function body_point(a, taken)
    local cx, cy = a.spawn.x, a.spawn.y
    for i = 1, #OFFSETS do
        if not taken[i] then
            local o = OFFSETS[i]
            local x, y = cx + o[1] * SPACING, cy + o[2] * SPACING
            if a.ship or (common.is_open(x, y) and not common.in_building(x, y)) then
                taken[i] = true
                return x, y
            end
        end
    end
    return cx, cy
end

-- The AI fight value of an army's bodies: hostile armies attack on sight (spec 3.8). Friendly and
-- neutral ones stay at 0. Only a body's own script may write it, so the value goes in its data.
local function fight_of(a)
    return events.is_hostile(state.ev, a) and FIGHT_HOSTILE or 0
end

-- Sends every body the sides of all bodies and the test flags (fight.lua targets by them).
local function broadcast_fight()
    for _, b in pairs(bodies) do
        for _, obj in pairs(b.actors) do
            pcall(function()
                obj:sendEvent('RazdorFightSides', { sides = fight_sides, player = fight_cfg.player })
            end)
        end
    end
end

local function dead_count(id)
    local n = 0
    for _ in pairs(state.armies[id].dead) do
        n = n + 1
    end
    return n
end

-- The fight ends when an army is wholly dead and another army still has a living body (the other
-- side wins; the player is not a side yet, so no army left means no line).
local function check_fight_end(id)
    local st = state.armies[id]
    if fight_ended or not st or not st.n or dead_count(id) < st.n then
        return
    end
    for oid, ost in pairs(state.armies) do
        if oid ~= id and ost.spawned and ost.n and dead_count(oid) < ost.n then
            fight_ended = true
            print(string.format('RAZDOR_FIGHT_END winner=%d reason=wipe t=%.0f', oid, core.getRealTime()))
            return
        end
    end
end

local function spawn_army(a)
    local st = army_state(a.id)
    st.gen = st.gen + 1
    st.spawned = true
    st.defeated = false
    st.respawn_at = nil
    st.dead = {}
    local b = { actors = {}, rng = common.rng(a.id * 7919 + 17), check_in = 2, taken = {} }
    bodies[a.id] = b
    local total = 0
    for _, u in ipairs(a.units) do
        for _ = 1, u.count do
            total = total + 1
            local x, y
            if u.leader then
                x, y = a.spawn.x, a.spawn.y
            else
                x, y = body_point(a, b.taken)
            end
            local obj = world.createObject(u.record, 1)
            obj:teleport('', util.vector3(x, y, a.spawn.z))
            obj:addScript(ARMY_SCRIPT, {
                army = a.id,
                k = total,
                gen = st.gen,
                leader = u.leader,
                patrols = a.patrols,
                leader_record = 'rz_army_' .. a.id,
                fight = fight_of(a),
            })
            if u.leader then
                b.leader = obj
            end
            b.actors[total] = obj
        end
    end
    st.n = total
    print(string.format('RAZDOR_FIGHT %d hostile=%s fight=%d', a.id, tostring(events.is_hostile(state.ev, a)), fight_of(a)))
    print(string.format('RAZDOR_ARMY_SPAWNED %d actors=%d', a.id, total))
    if a.ship then
        print(string.format('RAZDOR_SHIP %d z=%.0f', a.id, a.spawn.z))
    end
end

-- Takes an army off the map (the deactivate result, events.md §7.2): its bodies are removed.
local function deactivate_army(a)
    local b = bodies[a.id]
    if b then
        for _, obj in pairs(b.actors) do
            if obj:isValid() then
                obj:remove()
            end
        end
    end
    bodies[a.id] = nil
    local st = army_state(a.id)
    st.spawned = false
    st.defeated = false
    st.dead = {}
    st.n = 0
    a.inactive = true
    print(string.format('RAZDOR_ARMY_DEACTIVATED %d', a.id))
end

-- The check after a spawn: no body inside a building box or on a closed tile. Ship bodies are
-- exempt by design (spec 3.8, Q4) and counted apart.
local function check_army(a, b)
    local bad, exempt, checked = 0, 0, 0
    for k, obj in ipairs(b.actors) do
        if obj:isValid() then
            checked = checked + 1
            local p = obj.position
            print(string.format("RAZDOR_SQUAD %d k=%d x=%.0f y=%.0f", a.id, k, p.x, p.y))
            local inside = common.in_building(p.x, p.y)
            local closed = not common.is_open(p.x, p.y)
            if a.ship then
                exempt = exempt + 1
            elseif inside or closed then
                bad = bad + 1
                print(string.format('RAZDOR_CHECK_BAD %d x=%.0f y=%.0f inside=%s closed=%s',
                    a.id, p.x, p.y, tostring(inside), tostring(closed)))
            end
        end
    end
    print(string.format('RAZDOR_CHECK army=%d bodies=%d checked=%d bad=%d ship_exempt=%d',
        a.id, #b.actors, checked, bad, exempt))
end

-- A random open tile within the patrol radius of the army's home tile, as a world point, or nil.
local function pick_destination(a, b)
    local r = events.patrol_radius(state.ev, a)
    if r <= 0 then
        return nil
    end
    for _ = 1, 60 do
        local tx = a.tile[1] + b.rng(2 * r + 1) - r
        local ty = a.tile[2] + b.rng(2 * r + 1) - r
        if tx >= 0 and ty >= 0 and tx < map.width and ty < map.height then
            local x, y = common.tile_centre(tx, ty)
            if common.is_open(x, y) and not common.in_building(x, y) then
                return x, y
            end
        end
    end
    return nil
end

-- The engine's environment (events.lua reaches the world only through it).
local env = {}

function env.now()
    return dt_now()
end

function env.title(ev)
    return (ev.title:match('^[^%%#]*') or ''):gsub('%s+$', '')
end

function env.text(text)
    return events.plain_text(text, HERO_NAME)
end

-- An event window stops the world (the clock, the armies and the scans) until it closes (spec 3.11).
-- The tag is held by the window only; a saved game does not keep it.
local PAUSE_TAG = 'razdor_window'

function env.show(d)
    world.pause(PAUSE_TAG)
    local p = player()
    if p then
        p:sendEvent('RazdorShow', d)
    end
end

function env.close(wid)
    world.unpause(PAUSE_TAG)
    local p = player()
    if p then
        p:sendEvent('RazdorClose', { wid = wid })
    end
end

function env.quit()
    local p = player()
    if p then
        types.Player.sendMenuEvent(p, 'RazdorQuit', {})
    end
end

function env.journal(id, stage)
    local p = player()
    local q = p and types.Player.quests(p)['rz_q' .. id]
    if q then
        q:addJournalEntry(stage)
        print(string.format('RAZDOR_JOURNAL rz_q%d stage=%d', id, stage))
    else
        print(string.format('RAZDOR_JOURNAL_MISSING rz_q%d', id))
    end
end

function env.activate(id)
    local a = by_id[id]
    if not a or army_state(a.id).spawned then
        return
    end
    a.inactive = false
    spawn_army(a)
    print(string.format('RAZDOR_ARMY_ACTIVATED %d', id))
end

function env.deactivate(id)
    local a = by_id[id]
    if a then
        deactivate_army(a)
    end
end

function env.army_status(id)
    local st = state.armies[id]
    if st and st.defeated then
        return 'defeated'
    end
    if st and st.spawned then
        return 'active'
    end
    return 'inactive'
end

function env.army_at_home(id)
    local a = by_id[id]
    local b = bodies[id]
    local leader = b and b.leader
    if not a or not leader or not leader:isValid() then
        return false
    end
    local tx, ty = common.tile_of(leader.position.x, leader.position.y)
    return tx ~= nil and tx == a.tile[1] and ty == a.tile[2]
end

function env.fight(id, v)
    local b = bodies[id]
    if not b then
        return
    end
    for _, obj in pairs(b.actors) do
        if obj:isValid() then
            obj:sendEvent('RazdorFight', { v = v })
        end
    end
    print(string.format('RAZDOR_HOLD %d fight=%d', id, v))
end

-- Moves a spawned army next to the hero (events.md 7.2, 14 and 15): the leader takes the first open
-- tile around the hero (outside buildings), the other bodies the offsets around it. Ships are not
-- moved. Returns whether it moved.
local function move_next_to_hero(a)
    local b = bodies[a.id]
    local p = player()
    if not b or not p or a.ship then
        return false
    end
    local px, py, pz = p.position.x, p.position.y, p.position.z
    local lx, ly = nil, nil
    for i = 1, #OFFSETS do
        local x, y = px + OFFSETS[i][1] * SPACING, py + OFFSETS[i][2] * SPACING
        if common.is_open(x, y) and not common.in_building(x, y) then
            lx, ly = x, y
            break
        end
    end
    if not lx then
        return false
    end
    local spot = { spawn = { x = lx, y = ly, z = pz } }
    local taken = {}
    for k = 1, #b.actors do
        local obj = b.actors[k]
        if obj and obj:isValid() then
            local x, y = lx, ly
            if obj ~= b.leader then
                x, y = body_point(spot, taken)
            end
            obj:teleport('', util.vector3(x, y, pz))
        end
    end
    return true
end

-- The environment's results that move armies (events.lua, finish and battle).
function env.show_army(id)
    local a = by_id[id]
    if not a then
        return
    end
    if not army_state(id).spawned then
        env.activate(id)
    end
    print(string.format('RAZDOR_SHOW_ARMY %d spawned=%s', id, tostring(army_state(id).spawned)))
end

function env.move_to_hero(id)
    local a = by_id[id]
    local moved = a ~= nil and army_state(id).spawned and move_next_to_hero(a) or false
    print(string.format('RAZDOR_MOVE_TO_HERO %d moved=%s', id, tostring(moved)))
end

function env.battle(id)
    local a = by_id[id]
    if not a then
        return
    end
    if not army_state(id).spawned then
        env.activate(id)
    end
    move_next_to_hero(a)
end

local function start_game()
    local s = map.start
    local tx, ty = s.tile[1], s.tile[2]
    local p = player()
    if not p then
        return false
    end
    state.started = true
    state.t0 = core.getGameTime()
    local x, y = common.tile_centre(tx, ty)
    p:teleport('', util.vector3(x, y, 2000), { onGround = true })
    print(string.format('RAZDOR_START tile=%d,%d', tx, ty))
    set_clock()
    for _, a in ipairs(map.armies) do
        if a.inactive then
            print(string.format('RAZDOR_ARMY_SKIPPED %d reason=inactive', a.id))
        else
            spawn_army(a)
        end
    end
    p:sendEvent('RazdorStarted', {})
    events.init(state.ev, dt_now())
    events.refresh_holds(env, state.ev)
    events.scan(env, state.ev)
    return true
end

-- The meeting (spec 3.11): an army's leader within a tile of the player starts a scan with that army.
local function check_meetings()
    local p = player()
    if not p then
        return
    end
    for _, a in ipairs(map.armies) do
        local st = army_state(a.id)
        local b = bodies[a.id]
        local leader = b and b.leader
        local near = false
        if st.spawned and not st.defeated and leader and leader:isValid() and not a.ship then
            local d = leader.position - p.position
            near = math.sqrt(d.x * d.x + d.y * d.y) < MEET_UNITS
        end
        if near and not contact[a.id] then
            contact[a.id] = true
            print(string.format('RAZDOR_MEET %d', a.id))
            events.meet(env, state.ev, a.id)
        elseif not near then
            contact[a.id] = nil
        end
    end
end

-- One tick a second: the post-spawn checks, the deaths (the reports, plus a read of each body the
-- registry holds), the respawn, the meetings and the clock of the event engine.
local function tick()
    local now = core.getGameTime()
    for _, a in ipairs(map.armies) do
        local st = army_state(a.id)
        local b = ensure_bodies(a.id)
        if st.spawned and st.n > 0 then
            if b.check_in then
                b.check_in = b.check_in - 1
                if b.check_in <= 0 then
                    b.check_in = nil
                    check_army(a, b)
                end
            end
            if not st.defeated then
                local dead = 0
                for k = 1, st.n do
                    if not st.dead[k] then
                        local obj = b.actors[k]
                        if obj and obj:isValid() and types.Actor.isDead(obj) then
                            st.dead[k] = true
                        end
                    end
                    if st.dead[k] then
                        dead = dead + 1
                    end
                end
                if dead == st.n then
                    st.defeated = true
                    print(string.format('RAZDOR_ARMY_DEFEATED %d', a.id))
                    if a.respawn_days > 0 then
                        st.respawn_at = now + a.respawn_days * DAY_SECONDS
                    end
                end
            end
        end
        if st.defeated and st.respawn_at and now >= st.respawn_at then
            spawn_army(a)
            print(string.format('RAZDOR_ARMY_RESPAWNED %d', a.id))
        end
    end
    if state.ev.ready and not state.ev.ended then
        check_meetings()
        events.tick(env, state.ev)
    end
end

return {
    engineHandlers = {
        onUpdate = function(dt)
            if not state.started then
                if world.players[1] then
                    start_game()
                end
                return
            end
            acc = acc + dt
            if acc < 1 then
                return
            end
            acc = 0
            tick()
        end,
        onLoad = function(data)
            if data then
                state = data
            end
            if state.started then
                set_clock()
            end
        end,
        onSave = function()
            return state
        end,
    },
    eventHandlers = {
        -- A body reports itself (army.lua) on every load; the registry takes it if its generation
        -- is the army's current one.
        RazdorBodyReady = function(d)
            local st = state.armies[d.army]
            if not st or st.gen ~= d.gen or not d.actor then
                return
            end
            ensure_bodies(d.army).actors[d.k] = d.actor
            if d.k == 1 then
                ensure_bodies(d.army).leader = d.actor
            end
            fight_sides[d.actor.id] = { side = d.army, hostile = d.hostile and true or false }
            broadcast_fight()
        end,
        -- A body reports its death once (army.lua).
        RazdorBodyDead = function(d)
            local st = state.armies[d.army]
            if not st or st.gen ~= d.gen or st.dead[d.k] then
                return
            end
            st.dead[d.k] = true
            print(string.format('RAZDOR_BODY_DEAD %d k=%d', d.army, d.k))
            check_fight_end(d.army)
        end,
        -- The test flags of the fight (tools/openmw-smoke/m8): the player is no target.
        RazdorFightConfig = function(d)
            if d.player ~= nil then
                fight_cfg.player = d.player and true or false
            end
            broadcast_fight()
        end,
        -- A patrolling leader asks for its next destination (sent by its own script).
        RazdorArmyNext = function(d)
            local a = by_id[d.army]
            local b = bodies[d.army]
            if not a or not b or not a.patrols or a.ship or not d.actor then
                return
            end
            local x, y = pick_destination(a, b)
            if x then
                d.actor:sendEvent('RazdorArmyGoto', { x = x, y = y })
            end
        end,
        -- The passability guard (player.lua) puts the player back on his last open position.
        RazdorGuardReturn = function(d)
            local p = player()
            if p then
                p:teleport('', util.vector3(d.x, d.y, d.z))
            end
        end,
        -- The player stepped onto a tile (player.lua): the event engine's step.
        RazdorStep = function(d)
            if not state.ev.ready then
                return
            end
            events.step(env, state.ev, d.tx, d.ty)
        end,
        -- A window's answer (the player's buttons, or a test driver): 'yes', 'no' or 'ok'.
        RazdorAnswer = function(d)
            events.answer(env, state.ev, d.wid, d.answer)
        end,
        -- razdor_activate_army(id): spawns an inactive army once (the same as the deactivate result's
        -- counterpart, events.md §7.2).
        RazdorActivateArmy = function(d)
            env.activate(d.id)
        end,
    },
}
