-- Test-only driver of the M8 fight smoke (tools/openmw-smoke/m8; never shipped). The player is no
-- target of the fight (side 0 is not built yet: the smoke is army against army), and its health is
-- logged each second.
local core = require('openmw.core')
local self = require('openmw.self')
local types = require('openmw.types')

local sent = false
local next_log = 0

return {
    engineHandlers = {
        onUpdate = function()
            if not sent then
                sent = true
                core.sendGlobalEvent('RazdorFightConfig', { player = false })
            end
            local now = core.getRealTime()
            if now >= next_log then
                next_log = now + 1
                print(string.format('RAZDOR_FIGHT player hp=%.0f t=%.0f',
                    types.Actor.stats.dynamic.health(self).current, now))
            end
        end,
    },
}
