-- Test-only global script: moves the player to an exact exterior point (world units) on the ground.
local world = require('openmw.world')
local util = require('openmw.util')

return {
    eventHandlers = {
        RazdorSmokeTeleport = function(d)
            local player = world.players[1]
            if not player then return end
            player:teleport('', util.vector3(d.x, d.y, 2000), { onGround = true })
        end,
    },
}
