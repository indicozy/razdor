-- Global script of the converted map (M4, spec 3.8, 3.9, 3.11): the start, the game clock, the
-- armies (spawn, patrol orders, defeat, respawn) and the passability guard's teleport back.
-- Written for OpenMW 0.51. Log lines (print) are the ones the smoke test reads: RAZDOR_START,
-- RAZDOR_CLOCK, RAZDOR_ARMY_SPAWNED, RAZDOR_ARMY_SKIPPED, RAZDOR_CHECK, RAZDOR_ARMY_DEFEATED.
local world = require('openmw.world')
local types = require('openmw.types')
local util = require('openmw.util')
local core = require('openmw.core')
local common = require('scripts.razdor.common')
local map = require('scripts.razdor.map_data')

local ARMY_SCRIPT = 'scripts/razdor/army.lua'
local DAY_SECONDS = 86400 -- one DT day is 24 game hours (spec 3.11)
local RING = map.ring_units

-- Saved state: numbers and booleans only (game objects are not saved).
local state = { started = false, armies = {} }

-- Runtime only (not saved): the bodies of each spawned army. After a reload they are empty, so
-- deaths are not counted until the army is spawned again (a known gap, spec 3.8 and M5).
local bodies = {}

local by_id = {}
for _, a in ipairs(map.armies) do
    by_id[a.id] = a
end

local acc = 0

local function army_state(id)
    local st = state.armies[id]
    if not st then
        st = { spawned = false, defeated = false }
        state.armies[id] = st
    end
    return st
end

local function set_clock()
    local ok = pcall(world.setGameTimeScale, map.time_scale)
    print(string.format('RAZDOR_CLOCK scale=%.0f ok=%s', map.time_scale, tostring(ok)))
end

-- Where follower number k (of n) stands: on a ring of RING units around the leader, on an open
-- tile outside every building. Ships stand on the raft and are not tested.
local function body_point(a, k, n)
    local cx, cy = a.spawn.x, a.spawn.y
    local share = math.max(n, 1)
    if a.ship then
        local ang = 2 * math.pi * k / share
        return cx + RING * math.cos(ang), cy + RING * math.sin(ang)
    end
    for try = 0, 23 do
        local ang = 2 * math.pi * (k / share + try / 24)
        local r = RING * (1 + math.floor(try / 12) * 0.5)
        local x, y = cx + r * math.cos(ang), cy + r * math.sin(ang)
        if common.is_open(x, y) and not common.in_building(x, y) then
            return x, y
        end
    end
    return cx, cy
end

local function spawn_army(a)
    army_state(a.id).spawned = true
    local b = { actors = {}, leader = nil, rng = common.rng(a.id * 7919 + 17), check_in = 2 }
    bodies[a.id] = b
    local followers = 0
    for _, u in ipairs(a.units) do
        if not u.leader then
            followers = followers + u.count
        end
    end
    local k = 0
    for _, u in ipairs(a.units) do
        for _ = 1, u.count do
            local x, y
            if u.leader then
                x, y = a.spawn.x, a.spawn.y
            else
                x, y = body_point(a, k, followers)
                k = k + 1
            end
            local obj = world.createObject(u.record, 1)
            obj:teleport('', util.vector3(x, y, a.spawn.z))
            obj:addScript(ARMY_SCRIPT, {
                army = a.id,
                leader = u.leader,
                patrols = a.patrols,
                leader_record = 'rz_army_' .. a.id,
            })
            if u.leader then
                b.leader = obj
            end
            table.insert(b.actors, obj)
        end
    end
    print(string.format('RAZDOR_ARMY_SPAWNED %d actors=%d', a.id, #b.actors))
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

-- A body counts as alive unless the game says it is dead. A body that is not loaded (its cell is
-- far away) cannot be read, so it counts as alive.
local function body_alive(obj)
    if not obj:isValid() then
        return true
    end
    return not types.Actor.isDead(obj)
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

-- One tick a second: the post-spawn checks, the death count and the respawn.
local function tick()
    local now = core.getGameTime()
    for _, a in ipairs(map.armies) do
        local st = army_state(a.id)
        local b = bodies[a.id]
        if b then
            if b.check_in then
                b.check_in = b.check_in - 1
                if b.check_in <= 0 then
                    b.check_in = nil
                    check_army(a, b)
                end
            end
            if not st.defeated then
                local alive = 0
                for _, obj in ipairs(b.actors) do
                    if body_alive(obj) then
                        alive = alive + 1
                    end
                end
                if alive == 0 then
                    st.defeated = true
                    print(string.format('RAZDOR_ARMY_DEFEATED %d', a.id))
                    if a.respawn_days > 0 then
                        st.respawn_at = now + a.respawn_days * DAY_SECONDS
                    end
                end
            end
        end
        if st.defeated and st.respawn_at and now >= st.respawn_at then
            st.respawn_at = nil
            st.defeated = false
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
