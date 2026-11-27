-- Test-only (tools/openmw-smoke/m5; never shipped): answers every window of a converted map (Yes to a
-- question, OK to a message, Quit to the end window), so a run with nobody at the screen goes on.
local core = require('openmw.core')
local async = require('openmw.async')

return {
    eventHandlers = {
        RazdorShow = function(d)
            print(string.format('RAZDOR_AUTO show wid=%d kind=%s', d.wid, d.kind))
            local answer = d.kind == 'question' and 'yes' or 'ok'
            if d.kind == 'end' then
                answer = 'quit'
            end
            async:newUnsavableSimulationTimer(0.5, function()
                core.sendGlobalEvent('RazdorAnswer', { wid = d.wid, answer = answer })
            end)
        end,
    },
}
