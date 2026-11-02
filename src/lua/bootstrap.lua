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

-- Backtracking items of a pattern: each quantifier (* + - ?) counts once, and so does each
-- `.` or %class item that a quantifier follows (it is the one that backtracks). A set
-- ([...]) is one class. Matching cost grows like n^(items+1) in the subject length n, and the
-- instruction hook does not run inside the matcher, so the length is bounded by that.
-- The pattern features a script may not use: back-references (%1 to %9) and the balanced
-- (%b) and frontier (%f) matches, whose cost grows with the subject in ways the count below
-- does not see. Checked after an escape `%` wherever it stands, inside a set too; `%%` is an
-- escaped percent, so `%%1` is allowed.
local function feature(where, p, pos)
  local d = p:sub(pos, pos)
  if d:match("^[1-9bf]$") then
    reject(where, "pattern feature not allowed in Razdor scripts (%" .. d .. ")")
  end
end

local function backtracking(where, p)
  local q, i, n = 0, 1, #p
  local function quant(pos)
    local c = p:byte(pos)
    return c == 42 or c == 43 or c == 45 or c == 63
  end
  while i <= n do
    local c = p:byte(i)
    local kind
    if c == 37 then -- %x: a class (or an escaped %)
      feature(where, p, i + 1)
      kind = "any"
      i = i + 2
    elseif c == 91 then -- [...]: a set; its own escapes and ] are read as Lua reads them
      kind = "any"
      local j = i + 1
      if p:byte(j) == 94 then j = j + 1 end
      if p:byte(j) == 93 then j = j + 1 end
      while j <= n and p:byte(j) ~= 93 do
        if p:byte(j) == 37 then
          feature(where, p, j + 1)
          j = j + 1
        end
        j = j + 1
      end
      i = j + 1
    elseif c == 46 then -- .
      kind = "any"
      i = i + 1
    elseif c == 40 or c == 41 or c == 94 or c == 36 then -- ( ) ^ $: no item
      i = i + 1
    elseif quant(i) then -- a quantifier with nothing before it (after a group)
      q = q + 1
      i = i + 1
    else
      kind = "lit"
      i = i + 1
    end
    if kind and i <= n and quant(i) then
      q = q + 1
      if kind == "any" then
        q = q + 1
      end
      i = i + 1
    end
  end
  return q
end

-- The longest subject a pattern may run on: 2e8 steps at most, n^(items+1) <= 2e8, and the
-- subject limit in any case.
local function longest_subject(q)
  return math.min(limits.subject, math.floor(limits.work ^ (1 / (q + 1))))
end

local function check_pattern(where, s, p)
  check_time(where)
  if type(p) == "string" then
    if #p > limits.pattern then
      reject(where, "the pattern is longer than " .. limits.pattern .. " bytes")
    end
    local q = backtracking(where, p)
    if q > limits.quantifiers then
      reject(where, "the pattern has more than " .. limits.quantifiers .. " backtracking items")
    end
    if type(s) == "string" then
      local most = longest_subject(q)
      if #s > most then
        reject(where, "a subject of " .. #s .. " bytes is too long for this pattern (the limit is " .. most .. " bytes)")
      end
    end
  elseif type(s) == "string" and #s > limits.subject then
    reject(where, "the subject is longer than " .. limits.subject .. " bytes")
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
-- Every step of the iterator checks the deadline, not only its creation.
string.gmatch = function(s, p, ...)
  check_pattern("string.gmatch", s, p)
  local it = real_gmatch(s, p, ...)
  return function()
    check_time("string.gmatch")
    return it()
  end
end
-- A function or table replacement runs Lua code between matches, and the deadline is checked
-- before each one; a string replacement is plain text.
string.gsub = function(s, p, repl, ...)
  check_pattern("string.gsub", s, p)
  local t = type(repl)
  if t == "function" then
    local f = repl
    repl = function(...)
      check_time("string.gsub")
      return f(...)
    end
  elseif t == "table" then
    local tb = repl
    repl = function(k)
      check_time("string.gsub")
      return tb[k]
    end
  end
  return real_gsub(s, p, repl, ...)
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
