-- The siege of the abbey (chapter 3а, docs/ideas/pepelnaya-marka.md): a field battle on a 12 × 8
-- board. A wall runs down column 6 with a gate in the middle (HP 60); two battlements stand behind
-- the wall on column 5. The defenders (the hero's army and the abbey's garrison) are on the west,
-- behind the wall; the dead attack from the open east side.
--
-- Cover: a shot at a unit on a battlement, or behind the wall beside it (the defenders' side), does half damage.
-- Battlement shooters reach 2 tiles further (the rules do that for a battlement tile).
-- The dead go for the gate. On turn 4, if the gate stands, the wall beside it (6,5) falls to rubble,
-- and from then on the dead walk through the gap or the broken gate into the courtyard.
-- The defenders win when the dead are destroyed, or when 12 turns pass. The dead win when the hero's
-- units are all dead, or when they hold the courtyard (x <= 1) for two turns in a row.
--
-- The hooks are the battle's (see docs/reference/rzpack-format.md, «Field battles»). The defenders'
-- AI is the rules' own when this script says nothing (ai_turn returns nil for them).

local GATE = { x = 6, y = 3 }
local COLLAPSE_TURN = 4
local COLLAPSE_CELL = { x = 6, y = 5 }
local COURTYARD_X = 1
local TURNS = 12
local SIEGE_TURNS_HELD = 2

local function tile_at(s, x, y)
  if x < 0 or y < 0 or x >= s.board.w or y >= s.board.h then return nil end
  return s.board.tiles[y * s.board.w + x + 1]
end

local function gate_hp(s, x, y)
  for _, g in ipairs(s.board.gates) do
    if g.x == x and g.y == y then return g.hp end
  end
  return 0
end

local function standing(s, side)
  local out = {}
  for _, u in ipairs(s.units) do
    if u.alive and u.at and u.side == side then out[#out + 1] = u end
  end
  return out
end

local function occupied(s, x, y)
  for _, u in ipairs(s.units) do
    if u.alive and u.at and u.at.x == x and u.at.y == y then return true end
  end
  return false
end

-- A shot at a unit on a battlement, or behind the wall (on the defenders' side, beside the wall or a
-- standing gate), does half damage. The dead in front of the wall have no cover.
local function covered(s, at)
  if tile_at(s, at.x, at.y) == "battlement" then return true end
  if at.x >= GATE.x then return false end
  local t = tile_at(s, at.x + 1, at.y)
  return t == "wall" or (t == "gate" and gate_hp(s, at.x + 1, at.y) > 0)
end

-- Whether unit u can hit unit t from where it stands (the rules' own test, with the battlement's reach).
local function reaches(s, u, t)
  if not u.at or not t.at then return false end
  local d = razdor.distance(u.at, t.at)
  if d == 1 then return true end
  if u.shot_range <= 0 then return false end
  local range = u.shot_range
  if tile_at(s, u.at.x, u.at.y) == "battlement" then range = range + 2 end
  return d <= range and razdor.line_of_fire(u.at, t.at)
end

-- The reachable enemy with the lowest HP (the lowest id on a tie), or nil.
local function best_target(s, u, foes)
  local best
  for _, t in ipairs(foes) do
    if reaches(s, u, t) and (best == nil or t.hp < best.hp or (t.hp == best.hp and t.id < best.id)) then
      best = t
    end
  end
  return best
end

-- A rubble tile in the wall line (column 6), if the wall has fallen beside the gate: the way in.
local function breach(s)
  for y = 0, s.board.h - 1 do
    if tile_at(s, GATE.x, y) == "rubble" then return { x = GATE.x, y = y } end
  end
  return nil
end

function setup(s)
  local tiles = {}
  for y = 0, s.board.h - 1 do
    if y == GATE.y then
      tiles[#tiles + 1] = { x = GATE.x, y = y, tile = "gate", hp = 60 }
    else
      tiles[#tiles + 1] = { x = GATE.x, y = y, tile = "wall" }
    end
  end
  tiles[#tiles + 1] = { x = 5, y = 1, tile = "battlement" }
  tiles[#tiles + 1] = { x = 5, y = 6, tile = "battlement" }
  return { tiles = tiles, max_turns = TURNS, vars = { courtyard_turns = 0 } }
end

function on_turn_start(s)
  local log, tiles = {}, {}
  if s.round == COLLAPSE_TURN and gate_hp(s, GATE.x, GATE.y) > 0 then
    tiles[#tiles + 1] = { x = COLLAPSE_CELL.x, y = COLLAPSE_CELL.y, tile = "rubble" }
    log[#log + 1] = "abbey.siege.wall_falls"
  end
  local inside = 0
  for _, u in ipairs(standing(s, "attackers")) do
    if u.at.x <= COURTYARD_X then inside = inside + 1 end
  end
  local held = 0
  if inside > 0 then
    held = (s.vars.courtyard_turns or 0) + 1
    log[#log + 1] = "abbey.siege.courtyard_warning"
  end
  return { tiles = tiles, log = log, vars = { courtyard_turns = held } }
end

function on_damage(s, d)
  if d.shot then
    local t = s.units[d.target + 1]
    if t and t.at and covered(s, t.at) then
      return d.amount // 2
    end
  end
  return nil
end

function check_end(s)
  if #standing(s, "attackers") == 0 then return "defenders" end
  if #standing(s, "defenders") == 0 then return "attackers" end
  if (s.vars.courtyard_turns or 0) >= SIEGE_TURNS_HELD then return "attackers" end
  return nil
end

function ai_turn(s)
  local u = s.units[s.active + 1]
  if u.side == "defenders" then
    local t = best_target(s, u, standing(s, "attackers"))
    if t then return { attack = { unit = t.id } } end
    return nil
  end
  local t = best_target(s, u, standing(s, "defenders"))
  if t then return { attack = { unit = t.id } } end
  -- Through the gap the wall collapsed into (rubble on column 6): stand in it and fight whoever is
  -- next to it, then push on into the courtyard.
  local gap = breach(s)
  if gap then
    if u.at.x == gap.x and u.at.y == gap.y then
      return { move = { x = COURTYARD_X, y = gap.y } }
    end
    return { move = gap }
  end
  if gate_hp(s, GATE.x, GATE.y) > 0 then
    if razdor.distance(u.at, GATE) == 1 then
      return { attack = { gate = { x = GATE.x, y = GATE.y } } }
    end
    -- The gate's front (the one cell east of it that touches the gate) is for one unit at a time:
    -- the others wait where they stand.
    local front = { x = GATE.x + 1, y = GATE.y }
    if occupied(s, front.x, front.y) then
      return {}
    end
    if razdor.distance(u.at, front) <= u.manevres then
      return { move = front, attack = { gate = { x = GATE.x, y = GATE.y } } }
    end
    return { move = front }
  end
  return { move = { x = COURTYARD_X, y = GATE.y } }
end
