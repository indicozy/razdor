-- Test-only: kills a body when the M5 driver asks (tools/openmw-smoke/m5; never shipped).
local self = require('openmw.self')
local types = require('openmw.types')

return {
    eventHandlers = {
        RazdorTestKill = function()
            types.Actor.stats.dynamic.health(self).current = 0
        end,
    },
}
