-- The real-time fight driver (M8; spec docs/specs/2026-11-27-openmw-world.md, "Lua fight as built
-- (M8)"). A plain module, required by army.lua: every hostile army body runs M.tick on its own
-- interval (combat_data.lua gives the pace), M.on_hit is its RazdorHit handler, and the player's
-- script takes M.on_hit too. Movement is OpenMW's (Combat or Travel packages, which path well); every
-- hit is computed here (M.hit) and applied by the receiver's own script, since only a local script
-- may write its own health. Test-only flags come from the global script (RazdorFightSides).
local self = require('openmw.self')
local core = require('openmw.core')
local nearby = require('openmw.nearby')
local types = require('openmw.types')
local interfaces = require('openmw.interfaces')
local combat = require('scripts.razdor.combat_data')

local M = {}

local SHOT_RANGE = 3000 -- strike and shot reach, units (spec M8)
local MELEE_RANGE = 180 -- melee reach, units (spec M8)
local HEAL_BELOW = 0.7 -- a body heals itself below this share of its health (spec M8)

local role = nil -- the body's role from army.lua; nil in the player's script
local hostile = false -- this body fights: its army is hostile to the player
local sides = {} -- object id -> { side = army id, hostile = bool }, sent by the global script
local player_target = true -- whether army bodies may target the player (test flag off in m8)
local target_id = nil -- the object id of the current target
local mode = nil -- 'hold' (a ranged body stands and shoots) or 'close' (a Combat package)
local blessed = false
local next_at = 0

local function roll(pair)
    local lo, hi = pair[1], pair[2]
    if lo > hi then
        lo, hi = hi, lo
    end
    return math.random(lo, hi)
end

-- The one place that computes damage (spec M8). M9 adds the target's defence, protections and
-- surrender here, so the target's entry is passed in already. kind: melee, shot, strike, heal or
-- bless. Returns the amount: damage, healing, or 0 (bless is logged only for now).
function M.hit(attacker, target_entry, kind)
    local _ = target_entry -- used from M9 on (defence and protections)
    if kind == 'melee' then
        return roll(attacker.melee)
    elseif kind == 'shot' then
        return roll(attacker.shot)
    elseif kind == 'strike' then
        return attacker.strike.mag
    elseif kind == 'heal' then
        return attacker.heal
    end
    return 0
end

local function label()
    if role then
        return string.format('%s#%d.%d', self.recordId, role.army, role.k)
    end
    return 'player'
end

-- The receiver's side of a hit: lower its own health and log it. A body that reaches 0 dies in the
-- engine; army.lua reports the death (M.dead).
function M.on_hit(d)
    if types.Actor.isDead(self) then
        return
    end
    local h = types.Actor.stats.dynamic.health(self)
    h.current = h.current - d.amount
    print(string.format('RAZDOR_FIGHT hit from=%s to=%s kind=%s amount=%.0f hp=%.0f',
        d.from, label(), d.kind, d.amount, math.max(0, h.current)))
end

local function deliver(kind, t, entry)
    local amount = M.hit(entry, combat[t.recordId], kind)
    t:sendEvent('RazdorHit', { from = label(), amount = amount, kind = kind })
end

local function heal_self(entry)
    local h = types.Actor.stats.dynamic.health(self)
    local amount = M.hit(entry, nil, 'heal')
    h.current = math.min(h.base, h.current + amount)
    print(string.format('RAZDOR_FIGHT hit from=%s to=%s kind=heal amount=%.0f hp=%.0f',
        label(), label(), amount, h.current))
end

local function eligible(a)
    if a.id == self.id or types.Actor.isDead(a) then
        return false
    end
    if types.Player.objectIsInstance(a) then
        return player_target
    end
    local s = sides[a.id]
    return s ~= nil and s.side ~= role.army and s.hostile
end

-- The nearest living actor of the other side, and its distance.
local function nearest()
    local best, best_d = nil, nil
    for _, a in ipairs(nearby.actors) do
        if eligible(a) then
            local d = (a.position - self.position):length()
            if best_d == nil or d < best_d then
                best, best_d = a, d
            end
        end
    end
    return best, best_d
end

-- Movement: a ranged body in reach stands still (a Travel to its own place); everyone else closes
-- in with a Combat package. Only a local script may issue it on its own body.
local function set_package(want, t)
    local ai = interfaces.AI
    if not ai then
        return
    end
    if want == 'hold' then
        ai.startPackage({ type = 'Travel', destPosition = self.position, cancelOther = true })
    else
        ai.startPackage({ type = 'Combat', target = t, cancelOther = true })
    end
end

-- One combat action per interval: heal when hurt, else strike or shoot in reach, else melee.
-- Bless is logged once, at the first contact (spec M8: logs only for now).
function M.tick()
    if not role or not hostile or types.Actor.isDead(self) then
        return
    end
    local now = core.getRealTime()
    local entry = combat[self.recordId]
    if not entry or (next_at ~= 0 and now < next_at) then
        return
    end
    -- The first action comes one interval after the body is ready: the sides and the test flags
    -- reach the body from the global script in the meantime.
    if next_at == 0 then
        next_at = now + entry.interval
        return
    end
    next_at = now + entry.interval
    local t, d = nearest()
    if not t then
        target_id, mode = nil, nil
        return
    end
    local ranged = entry.strike ~= nil or entry.shot ~= nil
    local want = (ranged and d <= SHOT_RANGE) and 'hold' or 'close'
    local ai = interfaces.AI
    if t.id ~= target_id or want ~= mode or (ai and ai.getActivePackage() == nil) then
        target_id, mode = t.id, want
        set_package(want, t)
    end
    local h = types.Actor.stats.dynamic.health(self)
    if entry.heal and h.current < HEAL_BELOW * h.base then
        heal_self(entry)
    elseif ranged and d <= SHOT_RANGE then
        deliver(entry.strike and 'strike' or 'shot', t, entry)
    elseif entry.melee and d <= MELEE_RANGE then
        deliver('melee', t, entry)
    end
    if entry.bless and not blessed and d <= SHOT_RANGE then
        blessed = true
        print(string.format('RAZDOR_FIGHT hit from=%s to=%s kind=bless amount=0 hp=%.0f',
            label(), label(), h.current))
    end
end

-- True while the body has a target: army.lua does not send a fighting body back to its leader.
function M.busy()
    return target_id ~= nil
end

function M.dead()
    print('RAZDOR_FIGHT_DEAD id=' .. label())
end

-- The body's role (army.lua onInit and onLoad).
function M.set_role(r)
    role = r
    hostile = (r ~= nil and (r.fight or 0) > 0) or false
end

-- The sides of the bodies and the test flag, from the global script (on every ready and on a change).
function M.set_sides(d)
    sides = d.sides or {}
    if d.player ~= nil then
        player_target = d.player
    end
end

return M
