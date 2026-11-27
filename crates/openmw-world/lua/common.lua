-- Shared helpers of the converted map's runtime (M4). A plain module: required by the global,
-- player and army scripts. Tiles use DT's coordinates: x from the west, y from the north (row 0).
local map = require('scripts.razdor.map_data')

local M = {}

M.tile = map.tile
M.width = map.width
M.height = map.height

-- The tile (x, y) under a world point, or nil off the map.
function M.tile_of(x, y)
    local tx = math.floor(x / map.tile)
    local ty = map.height - 1 - math.floor(y / map.tile)
    if tx < 0 or tx >= map.width or ty < 0 or ty >= map.height then
        return nil
    end
    return tx, ty
end

-- The world point of the centre of tile (tx, ty).
function M.tile_centre(tx, ty)
    return (tx + 0.5) * map.tile, (map.height - ty - 0.5) * map.tile
end

-- Whether a tile is open on foot (the passability rows: one hex digit per tile, 0 closed).
function M.tile_open(tx, ty)
    local digit = tonumber(map.passable[ty + 1]:sub(tx + 1, tx + 1), 16)
    return digit ~= nil and digit ~= 0
end

-- Whether a world point is on an open tile. Off the map is closed.
function M.is_open(x, y)
    local tx, ty = M.tile_of(x, y)
    return tx ~= nil and M.tile_open(tx, ty)
end

-- Whether a world point lies inside a building's footprint box (spec 3.7, 3.8).
function M.in_building(x, y)
    for _, b in ipairs(map.buildings) do
        if x >= b.x0 and x <= b.x1 and y >= b.y0 and y <= b.y1 then
            return true
        end
    end
    return false
end

-- A deterministic generator: Park-Miller (16807, modulus 2^31 - 1) in plain integer arithmetic.
-- rng(n) gives an integer in 0..n-1. It never uses math.random (spec 4, determinism).
function M.rng(seed)
    local s = seed % 2147483647
    if s == 0 then
        s = 1
    end
    return function(n)
        s = (s * 16807) % 2147483647
        return s % n
    end
end

return M
