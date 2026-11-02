-- Razdor's sandbox, run once in every Lua state (`src/lua/mod.rs`). The chunk's vararg is the
-- host's check whether the state's budget or memory is spent (`exhausted()`); a script cannot
-- reach it, because it is a local of this chunk.
local exhausted = ...

-- Binary chunks are refused: `load` is text only.
local real_load = load
load = function(chunk, name, mode, ...)
  if select("#", ...) > 0 then
    return real_load(chunk, name, "t", ...)
  end
  return real_load(chunk, name, "t")
end

-- Only the memory count of the garbage collector is allowed; a script cannot change its collection.
local real_collectgarbage = collectgarbage
collectgarbage = function(option)
  if option == "count" then
    return real_collectgarbage("count")
  end
  error('collectgarbage: only "count" is allowed in a package script', 2)
end

-- A script may catch an error with pcall, but not the budget's or the memory's: those are
-- raised again, so a loop cannot go on after its budget is spent.
local function settle(ok, ...)
  if not ok and exhausted() then
    error((...), 0)
  end
  return ok, ...
end
local real_pcall, real_xpcall = pcall, xpcall
pcall = function(f, ...)
  return settle(real_pcall(f, ...))
end
xpcall = function(f, handler, ...)
  return settle(real_xpcall(f, handler, ...))
end

-- Keys in a fixed order: numbers, strings and booleans, each in its own order (false first).
local function key_before(a, b)
  local ta, tb = type(a), type(b)
  if ta ~= tb then
    return ta < tb
  end
  if ta == "boolean" then
    return b and not a
  end
  return a < b
end

-- `pairs` visits keys in an order that is not specified and can differ between runs; a
-- package that needs the same order every time uses this one.
function razdor.sorted_pairs(t)
  local keys, n = {}, 0
  for k in next, t do
    local tk = type(k)
    if tk ~= "number" and tk ~= "string" and tk ~= "boolean" then
      error("razdor.sorted_pairs: keys must be numbers, strings or booleans", 2)
    end
    n = n + 1
    keys[n] = k
  end
  table.sort(keys, key_before)
  local i = 0
  return function()
    i = i + 1
    local k = keys[i]
    if k ~= nil then
      return k, t[k]
    end
  end
end
