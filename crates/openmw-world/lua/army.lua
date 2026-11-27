-- Local script of every army body (M4, spec 3.8; M5 registry). It runs only while the body's cell
-- is active. Each body reports itself to the global script (RazdorBodyReady, on every load) and its
-- death once (RazdorBodyDead), keyed by the army, the body's number k and the spawn generation gen,
-- so the global script can count the army's dead after a reload. The leader walks to the
-- destinations the global script picks (RazdorArmyNext / RazdorArmyGoto); the other bodies follow
-- the leader (AI Follow).
local self = require('openmw.self')
local core = require('openmw.core')
local nearby = require('openmw.nearby')
local types = require('openmw.types')
local interfaces = require('openmw.interfaces')
local util = require('openmw.util')

local role = nil
local requested = false
local announced = false
local reported = false

-- The AI fight value of this body (only a local script may write its own stats). Hostile armies
-- attack on sight (spec 3.8); the global script sends RazdorFight to hold a body at 0.
local function setFight(v)
    if v == nil then
        return
    end
    local ok, err = pcall(function()
        types.Actor.stats.ai.fight(self).base = v
    end)
    if not ok then
        print('RAZDOR_FIGHT_ERR ' .. tostring(err))
    end
end

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
            announced = false
        end,
        onSave = function()
            return role
        end,
        onUpdate = function()
            if not role then
                return
            end
            if not announced then
                announced = true
                setFight(role.fight)
                core.sendGlobalEvent('RazdorBodyReady', { army = role.army, k = role.k, gen = role.gen, actor = self })
            end
            if not reported and types.Actor.isDead(self) then
                reported = true
                core.sendGlobalEvent('RazdorBodyDead', { army = role.army, k = role.k, gen = role.gen })
                return
            end
            if reported or not interfaces.AI then
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
        RazdorFight = function(d)
            setFight(d.v)
        end,
        RazdorArmyGoto = function(d)
            if not role or not role.leader or not interfaces.AI or reported then
                return
            end
            requested = false
            interfaces.AI.startPackage({
                type = 'Travel',
                destPosition = util.vector3(d.x, d.y, self.position.z),
                cancelOther = true,
            })
        end,
    },
}
