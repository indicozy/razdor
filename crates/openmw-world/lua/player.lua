-- Player script of the converted map (M4 guard, M5 steps and windows; spec 3.5, 3.11, 3.12). Five
-- times a second it reads the player's tile. A change of tile is sent to the global script as a step
-- (RazdorStep: the event engine's step, from RazdorStarted on). On an open tile the guard remembers
-- the position; on a closed tile (or off the map) it asks the global script to put the player back on
-- the last open position, and logs RAZDOR_GUARD. The guard is on from RazdorStarted and off with
-- --no-guard. Tile changes are logged only when map_data.debug_tiles is set.
--
-- Windows (M5): RazdorShow {wid, kind, title, text, buttons, victory, next_map} builds a window
-- (kind 'message', 'question' or 'end'); a button sends RazdorAnswer {wid, answer} to the global
-- script; RazdorClose {wid} takes the window away.
local self = require('openmw.self')
local core = require('openmw.core')
local ui = require('openmw.ui')
local async = require('openmw.async')
local util = require('openmw.util')
local common = require('scripts.razdor.common')
local map = require('scripts.razdor.map_data')

local PERIOD = 0.2
local COOLDOWN = 0.5

local enabled = false
local acc = 0
local cooldown = 0
local last_open = nil
local last_tile = nil

local win = nil
local shown = nil -- { wid = n, kind = ... } of the open window

local function close_window()
    if win then
        win:destroy()
        win = nil
    end
    shown = nil
end

local function press(answer)
    if shown then
        core.sendGlobalEvent('RazdorAnswer', { wid = shown.wid, answer = answer })
    end
end

-- The windows use no template: the MWUI frames need the Morrowind menu textures, which a converted
-- map does not have (the Template ships none). The text is drawn over the world.
local GOLD = util.color.rgb(0.95, 0.85, 0.55)

local function button(label, answer)
    return {
        type = ui.TYPE.Text,
        props = { text = '[ ' .. label .. ' ]', textSize = 20, textColor = GOLD },
        events = {
            mouseClick = async:callback(function()
                press(answer)
            end),
        },
    }
end

local function text_line(text, size)
    return {
        type = ui.TYPE.Text,
        props = { text = text, textSize = size or 18, textColor = GOLD, multiline = true, wordWrap = true, size = util.vector2(420, 0) },
    }
end

-- The window of a message, a question or the end of the game (spec 3.11, 3.12).
local function open_window(d)
    close_window()
    shown = { wid = d.wid, kind = d.kind }
    local lines = { text_line(d.title or '', 24) }
    if d.kind == 'end' then
        local head = d.victory and 'Победа' or 'Поражение'
        table.insert(lines, text_line(head, 20))
        if d.victory and d.next_map and d.next_map ~= '' then
            table.insert(lines, text_line('Следующая карта: ' .. d.next_map, 18))
        end
    else
        table.insert(lines, text_line(d.text or '', 18))
    end
    local row = {}
    for _, b in ipairs(d.buttons or {}) do
        local label = ({ ok = 'OK', yes = 'Да', no = 'Нет', quit = 'Выход' })[b] or b
        table.insert(row, button(label, b))
    end
    table.insert(lines, {
        type = ui.TYPE.Flex,
        props = { horizontal = true, arrange = ui.ALIGNMENT.Center },
        content = ui.content(row),
    })
    win = ui.create({
        layer = 'Windows',
        type = ui.TYPE.Flex,
        props = {
            relativePosition = util.vector2(0.5, 0.5),
            anchor = util.vector2(0.5, 0.5),
            autoSize = true,
            arrange = ui.ALIGNMENT.Center,
        },
        content = ui.content(lines),
    })
end

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
    if key ~= last_tile then
        last_tile = key
        if map.debug_tiles then
            print('RAZDOR_TILE ' .. key)
        end
        if enabled and tx ~= nil then
            core.sendGlobalEvent('RazdorStep', { tx = tx, ty = ty })
        end
    end
    if not map.guard then
        return
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
            if enabled or map.debug_tiles then
                guard(dt)
            end
        end,
    },
    eventHandlers = {
        RazdorStarted = function()
            enabled = true
            last_open = nil
        end,
        RazdorShow = function(d)
            open_window(d)
        end,
        RazdorClose = function(d)
            if shown and shown.wid == d.wid then
                close_window()
            end
        end,
    },
}
