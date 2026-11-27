-- Test-only player script for tools/openmw-smoke. Prints one line to the log, shows a Russian
-- message (font check), optionally moves the camera and teleports to probe points, then quits.
-- Settings come from config.lua, which run.sh writes into the run's own data folder.
local self = require('openmw.self')
local core = require('openmw.core')
local ui = require('openmw.ui')
local camera = require('openmw.camera')
local util = require('openmw.util')
local async = require('openmw.async')
local config = require('scripts.razdor_smoke.config')

local smokeAt = config.seconds or 10
local probes = config.probes or {}

local function probe(i)
    local p = probes[i]
    if not p then return end
    core.sendGlobalEvent('RazdorSmokeTeleport', { x = p[1], y = p[2] })
    async:newUnsavableSimulationTimer(2, function()
        local pos = self.position
        print(string.format('RAZDOR_PROBE i=%d x=%d y=%d z=%.1f', i, p[1], p[2], pos.z))
        probe(i + 1)
    end)
end

-- The camera angles are forced only for the frame they are set in (camera.lua), so they are
-- set on every frame once the static camera is on.
local staticCamera = false
async:newUnsavableSimulationTimer(0.5, function()
    if config.camera then
        camera.setMode(camera.MODE.Static)
        staticCamera = true
    end
end)

local engineHandlers = {
    onFrame = function()
        local c = config.camera
        if not staticCamera or not c then return end
        camera.setStaticPosition(util.vector3(c.x, c.y, c.z))
        camera.setPitch(c.pitch)
        camera.setYaw(c.yaw)
    end,
}

if #probes > 0 then
    async:newUnsavableSimulationTimer(1, function() probe(1) end)
end

async:newUnsavableSimulationTimer(smokeAt, function()
    local pos = self.position
    local cell = self.cell
    print(string.format('RAZDOR_SMOKE cell=%d,%d pos=%.1f,%.1f,%.1f',
        cell.gridX, cell.gridY, pos.x, pos.y, pos.z))
    ui.showMessage('Тест кириллицы: Раздор, Пепельная Марка, ёжик 123')
end)

async:newUnsavableSimulationTimer(smokeAt + (config.quitAfter or 5), function()
    core.quit()
end)

return { engineHandlers = engineHandlers }
