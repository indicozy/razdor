-- Test-only driver of the M5 synthetic scenario (tools/openmw-smoke/m5; never shipped). It answers
-- the windows (the questions in order: Yes to event 2, No to event 5; OK to messages; Quit to the end
-- window), walks the player onto the scenario's tiles and kills army 2's bodies, so that the events
-- fire in the order of EXPECTED.txt. Tiles are the synthetic map's (16 x 16, make_m5_map.py).
local core = require('openmw.core')
local self = require('openmw.self')
local nearby = require('openmw.nearby')
local async = require('openmw.async')
local types = require('openmw.types')

local H = 16
local TILE = 1024
local QUESTION_ANSWERS = { 'yes', 'no' }
local next_question = 1

local function centre(tx, ty)
    return (tx + 0.5) * TILE, (H - ty - 0.5) * TILE
end

local function walk(label, tx, ty)
    local x, y = centre(tx, ty)
    core.sendGlobalEvent('RazdorSmokeTeleport', { x = x, y = y })
    print(string.format('RAZDOR_DRIVER walk %s %d,%d', label, tx, ty))
end

local function kill_army_two()
    local n = 0
    for _, actor in ipairs(nearby.actors) do
        if actor.recordId == 'rz_army_2' or actor.recordId == 'rz_u9_l0' then
            actor:sendEvent('RazdorTestKill', {})
            n = n + 1
        end
    end
    print(string.format('RAZDOR_DRIVER kill army2 bodies=%d', n))
end

local function at(secs, fn)
    async:newUnsavableSimulationTimer(secs, fn)
end

at(1, function()
    print('RAZDOR_DRIVER begin')
end)
at(12, function() walk('meet army 1', 10, 5) end)
at(16, function() walk('building 1', 5, 7) end)
at(19, function() walk('point 1', 12, 12) end)
at(22, function() walk('army 2', 12, 3) end)
at(26, kill_army_two)
at(90, function()
    print('RAZDOR_DRIVER timeout')
end)

return {
    eventHandlers = {
        RazdorShow = function(d)
            print(string.format('RAZDOR_DRIVER show wid=%d kind=%s', d.wid, d.kind))
            local quest = types.Player.quests(self)['rz_q10']
            print(string.format('RAZDOR_DRIVER quest rz_q10 stage=%s', tostring(quest and quest.stage)))
            local answer = 'ok'
            if d.kind == 'question' then
                answer = QUESTION_ANSWERS[next_question] or 'no'
                next_question = next_question + 1
            elseif d.kind == 'end' then
                answer = 'quit'
            end
            -- The end window stays up for 20 s, so a screenshot taken during it shows it.
            at(d.kind == 'end' and 20 or 0.3, function()
                core.sendGlobalEvent('RazdorAnswer', { wid = d.wid, answer = answer })
            end)
        end,
    },
}
