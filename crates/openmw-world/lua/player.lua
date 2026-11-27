-- Player script of the converted map (M4, spec 3.5): the passability guard. Five times a second
-- it reads the player's tile. On an open tile it remembers the position. On a closed tile (or off
-- the map) it asks the global script to put the player back on the last open position, and logs
-- RAZDOR_GUARD. The guard is on from RazdorStarted (sent when the start teleport is done) and
-- off with --no-guard. Tile changes are logged only when map_data.debug_tiles is set (M5 reads them).
local self = require('openmw.self')
local core = require('openmw.core')
local common = require('scripts.razdor.common')
local map = require('scripts.razdor.map_data')

local PERIOD = 0.2
local COOLDOWN = 0.5

local enabled = false
local acc = 0
local cooldown = 0
local last_open = nil
local last_tile = nil

local function guard(dt)
    cooldown = math.max(0, cooldown - dt)
    acc = acc + dt
    if acc < PERIOD then
        return
    end
    acc = 0
    local p = self.position
    local tx, ty = common.tile_of(p.x, p.y)
    local key = tostring(tx) .. ',' .. tostring(ty)
    if map.debug_tiles and key ~= last_tile then
        last_tile = key
        print('RAZDOR_TILE ' .. key)
    end
    if common.is_open(p.x, p.y) then
        last_open = { x = p.x, y = p.y, z = p.z }
        return
    end
    if last_open and cooldown <= 0 then
        cooldown = COOLDOWN
        print(string.format('RAZDOR_GUARD tile=%s,%s from=%.0f,%.0f back=%.0f,%.0f t=%.2f',
            tostring(tx), tostring(ty), p.x, p.y, last_open.x, last_open.y, core.getSimulationTime()))
        core.sendGlobalEvent('RazdorGuardReturn', last_open)
    end
end

return {
    engineHandlers = {
        onUpdate = function(dt)
            if enabled and map.guard then
                guard(dt)
            end
        end,
    },
    eventHandlers = {
        RazdorStarted = function()
            enabled = true
            last_open = nil
        end,
    },
}
