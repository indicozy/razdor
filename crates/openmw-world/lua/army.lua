-- Local script of every army body (M4, spec 3.8). It runs only while the body's cell is active.
-- The leader walks to the destinations the global script picks (RazdorArmyNext / RazdorArmyGoto);
-- the other bodies follow the leader (AI Follow). Attitude and fights are not set here (M4).
local self = require('openmw.self')
local core = require('openmw.core')
local nearby = require('openmw.nearby')
local interfaces = require('openmw.interfaces')

local role = nil
local requested = false

local function find_leader(record)
    for _, actor in ipairs(nearby.actors) do
        if actor.recordId == record then
            return actor
        end
    end
    return nil
end

return {
    engineHandlers = {
        onInit = function(data)
            role = data
        end,
        onLoad = function(data)
            role = data
        end,
        onSave = function()
            return role
        end,
        onUpdate = function()
            if not role or not interfaces.AI then
                return
            end
            local ai = interfaces.AI
            if role.leader then
                if role.patrols and not requested and ai.getActivePackage() == nil then
                    requested = true
                    core.sendGlobalEvent('RazdorArmyNext', { army = role.army, actor = self })
                end
            elseif ai.getActivePackage() == nil then
                local leader = find_leader(role.leader_record)
                if leader then
                    ai.startPackage({ type = 'Follow', target = leader, cancelOther = true })
                end
            end
        end,
    },
    eventHandlers = {
        RazdorArmyGoto = function(d)
            if not role or not role.leader or not interfaces.AI then
                return
            end
            requested = false
            interfaces.AI.startPackage({
                type = 'Travel',
                destPosition = require('openmw.util').vector3(d.x, d.y, self.position.z),
                cancelOther = true,
            })
        end,
    },
}
