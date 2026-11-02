-- Razdor's sandbox, run once in every Lua state (`src/lua/mod.rs`). The chunk's varargs are the
-- host's checks (`exhausted()`: the budget or memory is spent; `overdue()`: the wall-clock
-- deadline has passed) and the size bounds. A script cannot reach them: they are locals of
-- this chunk.
local exhausted, overdue, limits = ...

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

-- Library calls written in C do not count instructions, so the budget cannot stop one. The
-- wrappers below refuse the calls that could run long: a subject or a pattern too big, a
-- pattern with too many quantifiers, a string or a concatenation too long. A call that is
-- still slow is stopped by the wall-clock deadline at its next instruction.
local function reject(where, what)
  error("razdor: " .. where .. ": " .. what, 3)
end

local function check_time(where)
  if overdue() then
    reject(where, "the time limit is spent")
  end
end

local function check_pattern(where, s, p)
  check_time(where)
  if type(s) == "string" and #s > limits.subject then
    reject(where, "the subject is longer than " .. limits.subject .. " bytes")
  end
  if type(p) == "string" then
    if #p > limits.pattern then
      reject(where, "the pattern is longer than " .. limits.pattern .. " bytes")
    end
    local q = 0
    for i = 1, #p do
      local c = p:byte(i)
      if c == 42 or c == 43 or c == 45 or c == 63 then -- * + - ?
        q = q + 1
      end
    end
    if q > limits.quantifiers then
      reject(where, "the pattern has more than " .. limits.quantifiers .. " quantifiers")
    end
  end
end

local real_find, real_match, real_gmatch, real_gsub = string.find, string.match, string.gmatch, string.gsub
string.find = function(s, p, ...)
  check_pattern("string.find", s, p)
  return real_find(s, p, ...)
end
string.match = function(s, p, ...)
  check_pattern("string.match", s, p)
  return real_match(s, p, ...)
end
string.gmatch = function(s, p, ...)
  check_pattern("string.gmatch", s, p)
  return real_gmatch(s, p, ...)
end
string.gsub = function(s, p, ...)
  check_pattern("string.gsub", s, p)
  return real_gsub(s, p, ...)
end

local real_rep = string.rep
string.rep = function(s, n, sep)
  check_time("string.rep")
  if type(n) == "number" and n > 0 then
    local unit = #tostring(s) + (sep and #tostring(sep) or 0)
    if unit * n > limits.rep then
      reject("string.rep", "the result would be longer than " .. limits.rep .. " bytes")
    end
  end
  return real_rep(s, n, sep)
end

local real_concat = table.concat
table.concat = function(t, sep, i, j)
  check_time("table.concat")
  local from, to = i or 1, j or #t
  local total = 0
  if type(sep) == "string" then
    total = #sep * math.max(0, to - from)
  end
  for k = from, to do
    local v = t[k]
    local tv = type(v)
    if tv == "string" then
      total = total + #v
    elseif tv == "number" then
      total = total + 32
    end
    if total > limits.concat then
      reject("table.concat", "the result would be longer than " .. limits.concat .. " bytes")
    end
  end
  return real_concat(t, sep, i, j)
end
