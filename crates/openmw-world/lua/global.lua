-- Global script of the converted map (M4, spec 3.8, 3.9, 3.11): the start, the game clock, the
-- armies (spawn, attitude, patrol orders, defeat, respawn) and the passability guard's teleport back.
-- Written for OpenMW 0.51. Log lines (print) are the ones the smoke test reads: RAZDOR_START,
-- RAZDOR_CLOCK, RAZDOR_ARMY_SPAWNED, RAZDOR_ARMY_SKIPPED, RAZDOR_CHECK, RAZDOR_FIGHT,
-- RAZDOR_BODY_DEAD, RAZDOR_ARMY_DEFEATED.
local world = require('openmw.world')
local types = require('openmw.types')
local util = require('openmw.util')
local core = require('openmw.core')
local common = require('scripts.razdor.common')
local map = require('scripts.razdor.map_data')

local ARMY_SCRIPT = 'scripts/razdor/army.lua'
local DAY_SECONDS = 86400 -- one DT day is 24 game hours (spec 3.11)
local SPACING = map.spacing
local FIGHT_HOSTILE = 100 -- the AI fight value of a hostile body (spec 3.8)

-- Saved state: numbers, booleans and tables of numbers and booleans only (game objects are not saved).
-- Per army: spawned, defeated, gen (the spawn generation), n (its bodies), dead (body k -> true).
local state = { started = false, armies = {} }

-- Runtime only (not saved): the body objects of each army, by body number k. The bodies report
-- themselves again on every load (RazdorBodyReady), so this table is rebuilt after a reload.
local bodies = {}

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

-- Where follower k (1-based) stands: the first free offset, in order, that is an open tile outside
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
    return a.hostile and FIGHT_HOSTILE or 0
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
    local k = 0
    for _, u in ipairs(a.units) do
        for _ = 1, u.count do
            total = total + 1
            local x, y
            if u.leader then
                x, y = a.spawn.x, a.spawn.y
            else
                k = k + 1
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
    print(string.format('RAZDOR_FIGHT %d hostile=%s fight=%d', a.id, tostring(a.hostile), fight_of(a)))
    print(string.format('RAZDOR_ARMY_SPAWNED %d actors=%d', a.id, total))
    if a.ship then
        print(string.format('RAZDOR_SHIP %d z=%.0f', a.id, a.spawn.z))
    end
end

-- The check after a spawn: no body inside a building box or on a closed tile. Ship bodies are
-- exempt by design (spec 3.8, Q4) and counted apart.
local function check_army(a, b)
    local bad, exempt, checked = 0, 0, 0
    for _, obj in ipairs(b.actors) do
        if obj:isValid() then
            checked = checked + 1
            local p = obj.position
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
    local r = a.patrol_radius
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

local function start_game()
    local s = map.start
    local tx, ty = s.tile[1], s.tile[2]
    local player = world.players[1]
    if not player then
        return false
    end
    state.started = true
    local x, y = common.tile_centre(tx, ty)
    player:teleport('', util.vector3(x, y, 2000), { onGround = true })
    print(string.format('RAZDOR_START tile=%d,%d', tx, ty))
    set_clock()
    for _, a in ipairs(map.armies) do
        if a.inactive then
            print(string.format('RAZDOR_ARMY_SKIPPED %d reason=inactive', a.id))
        else
            spawn_army(a)
        end
    end
    player:sendEvent('RazdorStarted', {})
    return true
end

-- One tick a second: the post-spawn checks, the deaths (the reports, plus a read of each body the
-- registry holds) and the respawn.
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
        end,
        -- A body reports its death once (army.lua).
        RazdorBodyDead = function(d)
            local st = state.armies[d.army]
            if not st or st.gen ~= d.gen or st.dead[d.k] then
                return
            end
            st.dead[d.k] = true
            print(string.format('RAZDOR_BODY_DEAD %d k=%d', d.army, d.k))
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
            local player = world.players[1]
            if player then
                player:teleport('', util.vector3(d.x, d.y, d.z))
            end
        end,
        -- razdor_activate_army(id) for M5: spawns an inactive army once.
        RazdorActivateArmy = function(d)
            local a = by_id[d.id]
            if not a or army_state(a.id).spawned then
                return
            end
            a.inactive = false
            spawn_army(a)
        end,
    },
}
