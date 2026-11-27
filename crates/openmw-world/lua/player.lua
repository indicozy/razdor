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
-- map does not have (the Template ships none). Each window is a dark backdrop (textures/razdor/panel.tga,
-- written by the converter, M6) with the text on it.
local GOLD = util.color.rgb(0.95, 0.85, 0.55)
local PANEL_PATH = 'textures/razdor/panel.tga'

local function button(label, answer)
    return {
        type = ui.TYPE.Text,
        props = { text = '[ ' .. label .. ' ]', textSize = 22, textColor = GOLD },
        events = {
            mouseClick = async:callback(function()
                press(answer)
            end),
        },
    }
end

-- A line of text in a box of the window's width and the given height (a Flex child needs both).
local WIDTH = 440
local PAD = 18 -- the text's padding inside the backdrop
local WINDOW_H = 300

local function text_line(text, size, height)
    return {
        type = ui.TYPE.Text,
        props = {
            text = text,
            textSize = size or 18,
            textColor = GOLD,
            multiline = true,
            wordWrap = true,
            autoSize = false,
            size = util.vector2(WIDTH, height or 40),
        },
    }
end

-- The window of a message, a question or the end of the game (spec 3.11, 3.12). The backdrop is a
-- dark panel, 85% opaque, stretched over the whole window; the text sits inside its padding.
local function open_window(d)
    close_window()
    shown = { wid = d.wid, kind = d.kind }
    local lines = { text_line(d.title or '', 26, 36) }
    if d.kind == 'end' then
        local head = d.victory and 'Победа' or 'Поражение'
        table.insert(lines, text_line(head, 22, 30))
        if d.victory and d.next_map and d.next_map ~= '' then
            table.insert(lines, text_line('Следующая карта: ' .. d.next_map, 19, 30))
        end
    elseif d.kind ~= 'hall' then
        table.insert(lines, text_line(d.text or '', 19, 170))
    end
    -- A button is an answer string (OK, Yes, ...) or { answer, label } (the hall's entries).
    local row = {}
    for _, b in ipairs(d.buttons or {}) do
        if type(b) == 'table' then
            table.insert(row, button(b.label, b.answer))
        else
            local label = ({ ok = 'OK', yes = 'Да', no = 'Нет', quit = 'Выход' })[b] or b
            table.insert(row, button(label, b))
        end
    end
    local height = WINDOW_H
    if d.kind == 'hall' then
        -- The hall stacks its entries, one per row, and the window grows with them.
        for _, btn in ipairs(row) do
            table.insert(lines, {
                type = ui.TYPE.Flex,
                props = { horizontal = true, arrange = ui.ALIGNMENT.Center, size = util.vector2(WIDTH, 34) },
                content = ui.content({ btn }),
            })
        end
        height = math.max(WINDOW_H, 70 + 36 * #row + 2 * PAD)
    else
        table.insert(lines, {
            type = ui.TYPE.Flex,
            props = { horizontal = true, arrange = ui.ALIGNMENT.Center, size = util.vector2(WIDTH, 34) },
            content = ui.content(row),
        })
    end
    local backdrop = {
        type = ui.TYPE.Image,
        props = {
            resource = ui.texture({ path = PANEL_PATH }),
            relativeSize = util.vector2(1, 1),
            alpha = 0.85,
        },
    }
    local body = {
        type = ui.TYPE.Flex,
        props = {
            position = util.vector2(PAD, PAD),
            size = util.vector2(WIDTH, height - 2 * PAD),
            arrange = ui.ALIGNMENT.Center,
        },
        content = ui.content(lines),
    }
    win = ui.create({
        layer = 'Windows',
        type = ui.TYPE.Widget,
        props = {
            relativePosition = util.vector2(0.5, 0.5),
            anchor = util.vector2(0.5, 0.5),
            size = util.vector2(WIDTH + 2 * PAD, height),
        },
        content = ui.content({ backdrop, body }),
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
