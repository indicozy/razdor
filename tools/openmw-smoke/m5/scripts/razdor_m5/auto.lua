-- Test-only (tools/openmw-smoke/m5; never shipped): answers every window of a converted map (Yes to a
-- question, OK to a message, Quit to the end window), so a run with nobody at the screen goes on.
-- The window stops the world (spec 3.11), so the delay runs on real time (onFrame), not on the
-- simulation's timers, which stop with the world.
local core = require('openmw.core')

local due = {} -- { at = real seconds, fn = function }

local function later(secs, fn)
    table.insert(due, { at = core.getRealTime() + secs, fn = fn })
end

return {
    engineHandlers = {
        onFrame = function()
            local now = core.getRealTime()
            for i = #due, 1, -1 do
                if due[i].at <= now then
                    local fn = due[i].fn
                    table.remove(due, i)
                    fn()
                end
            end
        end,
    },
    eventHandlers = {
        RazdorShow = function(d)
            print(string.format('RAZDOR_AUTO show wid=%d kind=%s', d.wid, d.kind))
            local answer = d.kind == 'question' and 'yes' or 'ok'
            if d.kind == 'end' then
                answer = 'quit'
            end
            later(0.5, function()
                core.sendGlobalEvent('RazdorAnswer', { wid = d.wid, answer = answer })
            end)
        end,
    },
}
