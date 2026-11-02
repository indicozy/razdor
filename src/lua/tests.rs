use super::*;

fn scripts(files: &[(&str, &str)]) -> Scripts {
    files.iter().map(|(p, s)| (p.to_string(), s.to_string())).collect()
}

fn rt(files: &[(&str, &str)]) -> Runtime {
    Runtime::new(7, scripts(files)).expect("the sandbox starts")
}

#[test]
fn the_sandbox_has_only_the_allowed_libraries() {
    let r = rt(&[(
        "scripts/t.lua",
        "function probe()\n return { type(io), type(os), type(package), type(debug), type(coroutine), type(dofile), type(loadfile),\n  type(string.dump), type(math.random), type(utf8), type(table), type(require), type(load), type(print), type(collectgarbage) }\nend\n",
    )]);
    r.load_script("scripts/t.lua").unwrap();
    let got: Vec<String> = r.call::<Table>("scripts/t.lua", "probe", ()).unwrap().unwrap().sequence_values::<String>().map(|v| v.unwrap()).collect();
    assert_eq!(got, ["nil", "nil", "nil", "nil", "nil", "nil", "nil", "nil", "function", "table", "table", "function", "function", "function", "function"]);
}

#[test]
fn io_and_os_are_blocked_in_use() {
    let r = rt(&[("scripts/t.lua", "function f() return os.time() end\n")]);
    r.load_script("scripts/t.lua").unwrap();
    let e = r.call::<Value>("scripts/t.lua", "f", ()).unwrap_err();
    assert!(e.message.contains("scripts/t.lua:1:"), "{e}");
    assert!(e.to_string().contains("(in f of scripts/t.lua)"), "{e}");
}

#[test]
fn binary_chunks_are_refused() {
    let r = rt(&[("scripts/t.lua", "function f() local a, b = load('\\27Lua') return a, b end\n")]);
    r.load_script("scripts/t.lua").unwrap();
    let out: (Value, String) = r.call("scripts/t.lua", "f", ()).unwrap().unwrap();
    assert!(out.0.is_nil(), "a binary chunk loads");
    assert!(out.1.contains("binary"), "{}", out.1);
}

#[test]
fn collectgarbage_is_only_count() {
    let r = rt(&[("scripts/t.lua", "function ok() return collectgarbage('count') > 0 end\nfunction bad() collectgarbage('collect') end\n")]);
    r.load_script("scripts/t.lua").unwrap();
    assert_eq!(r.call::<bool>("scripts/t.lua", "ok", ()).unwrap(), Some(true));
    let e = r.call::<()>("scripts/t.lua", "bad", ()).unwrap_err();
    assert!(e.message.contains("only \"count\""), "{e}");
}

#[test]
fn the_budget_stops_an_endless_loop() {
    let r = rt(&[("scripts/loop.lua", "function setup()\n  while true do end\nend\n")]);
    r.load_script("scripts/loop.lua").unwrap();
    let e = r.call::<()>("scripts/loop.lua", "setup", ()).unwrap_err();
    assert!(e.script == "scripts/loop.lua" && e.call == "setup", "{e:?}");
    assert!(e.message.contains("instruction budget"), "{e}");
    assert!(r.exhausted());
}

#[test]
fn pcall_cannot_swallow_the_budget() {
    let r = rt(&[("scripts/loop.lua", "function setup()\n  while true do pcall(function() while true do end end) end\nend\n")]);
    r.load_script("scripts/loop.lua").unwrap();
    let e = r.call::<()>("scripts/loop.lua", "setup", ()).unwrap_err();
    assert!(e.message.contains("instruction budget"), "{e}");
}

#[test]
fn pcall_catches_ordinary_errors() {
    let r = rt(&[("scripts/t.lua", "function f() local ok, msg = pcall(error, 'boom') return ok, msg end\n")]);
    r.load_script("scripts/t.lua").unwrap();
    let (ok, msg): (bool, String) = r.call("scripts/t.lua", "f", ()).unwrap().unwrap();
    assert!(!ok);
    assert_eq!(msg, "boom");
}

#[test]
fn the_memory_limit_stops_a_growing_table() {
    let r = rt(&[("scripts/mem.lua", "function grow()\n  local t = {}\n  while true do t[#t + 1] = string.rep('x', 4096) end\nend\n")]);
    r.load_script("scripts/mem.lua").unwrap();
    let e = r.call::<()>("scripts/mem.lua", "grow", ()).unwrap_err();
    assert!(e.message.contains("not enough memory"), "{e}");
    assert_eq!(e.call, "grow");
}

#[test]
fn require_loads_the_packages_own_scripts() {
    let r = rt(&[
        ("scripts/main.lua", "local h = require('helpers')\nfunction value() return h.double(21) end\n"),
        ("scripts/helpers.lua", "local M = {}\nfunction M.double(x) return x * 2 end\nreturn M\n"),
    ]);
    r.load_script("scripts/main.lua").unwrap();
    assert_eq!(r.call::<i64>("scripts/main.lua", "value", ()).unwrap(), Some(42));
}

#[test]
fn require_refuses_a_missing_or_a_circular_module() {
    let r = rt(&[
        ("scripts/a.lua", "require('nowhere')\n"),
        ("scripts/loop.lua", "require('loop')\n"),
        ("scripts/bad name.lua", "x = 1\n"),
    ]);
    let e = r.load_script("scripts/a.lua").unwrap_err();
    assert!(e.message.contains("module 'nowhere' not found"), "{e}");
    let e = r.load_script("scripts/loop.lua").unwrap_err();
    assert!(e.message.contains("circular require") || e.message.contains("not found"), "{e}");
}

#[test]
fn require_does_not_read_the_disk() {
    let r = rt(&[("scripts/t.lua", "function f() return pcall(require, '../../etc/passwd') end\n")]);
    r.load_script("scripts/t.lua").unwrap();
    let (ok, _): (bool, Value) = r.call("scripts/t.lua", "f", ()).unwrap().unwrap();
    assert!(!ok);
}

#[test]
fn random_is_the_same_for_the_same_seed() {
    let src = "function roll() local t = {} for i = 1, 8 do t[i] = math.random(1, 6) end return t end\n";
    let a = Runtime::new(99, scripts(&[("scripts/r.lua", src)])).unwrap();
    let b = Runtime::new(99, scripts(&[("scripts/r.lua", src)])).unwrap();
    let c = Runtime::new(100, scripts(&[("scripts/r.lua", src)])).unwrap();
    for r in [&a, &b, &c] {
        r.load_script("scripts/r.lua").unwrap();
    }
    let va: Vec<i64> = a.call::<Table>("scripts/r.lua", "roll", ()).unwrap().unwrap().sequence_values().map(|v| v.unwrap()).collect();
    let vb: Vec<i64> = b.call::<Table>("scripts/r.lua", "roll", ()).unwrap().unwrap().sequence_values().map(|v| v.unwrap()).collect();
    let vc: Vec<i64> = c.call::<Table>("scripts/r.lua", "roll", ()).unwrap().unwrap().sequence_values().map(|v| v.unwrap()).collect();
    assert_eq!(va, vb);
    assert_ne!(va, vc);
    assert!(va.iter().all(|v| (1..=6).contains(v)));
}

#[test]
fn math_randomseed_is_refused() {
    let r = rt(&[("scripts/t.lua", "function f() math.randomseed(1) end\n")]);
    r.load_script("scripts/t.lua").unwrap();
    let e = r.call::<()>("scripts/t.lua", "f", ()).unwrap_err();
    assert!(e.message.contains("seed comes from the game"), "{e}");
}

#[test]
fn sorted_pairs_visits_keys_in_order() {
    let src = "function keys()\n local t = { b = 1, a = 2, [3] = 0, [1] = 0, [true] = 0 }\n local out = {}\n for k in razdor.sorted_pairs(t) do out[#out + 1] = tostring(k) end\n return table.concat(out, ',')\nend\n";
    let r = rt(&[("scripts/t.lua", src)]);
    r.load_script("scripts/t.lua").unwrap();
    assert_eq!(r.call::<String>("scripts/t.lua", "keys", ()).unwrap(), Some("true,1,3,a,b".to_string()));
}

#[test]
fn print_goes_to_the_log_not_stdout() {
    let r = rt(&[("scripts/t.lua", "print('hello', 2, nil)\n")]);
    r.load_script("scripts/t.lua").unwrap();
    assert_eq!(r.take_log(), vec!["hello\t2\tnil".to_string()]);
}

#[test]
fn a_syntax_error_names_its_line_and_calls_nothing() {
    let files = scripts(&[("scripts/ok.lua", "function f() return 1 end\n"), ("scripts/bad.lua", "local x = 1\nlocal y = = 2\n")]);
    let errs = check_scripts(&files);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert_eq!(errs[0].script, "scripts/bad.lua");
    assert!(errs[0].message.starts_with("scripts/bad.lua:2:"), "{}", errs[0].message);
}

#[test]
fn a_missing_hook_is_not_an_error() {
    let r = rt(&[("scripts/t.lua", "x = 1\n")]);
    r.load_script("scripts/t.lua").unwrap();
    assert_eq!(r.call::<()>("scripts/t.lua", "setup", ()).unwrap(), None);
    assert!(!r.defines("setup"));
}

#[test]
fn the_budget_resets_for_each_call() {
    let r = rt(&[("scripts/t.lua", "function f() local s = 0 for i = 1, 200000 do s = s + i end return s end\n")]);
    r.load_script("scripts/t.lua").unwrap();
    for _ in 0..5 {
        assert_eq!(r.call::<i64>("scripts/t.lua", "f", ()).unwrap(), Some(20000100000));
    }
}

#[test]
fn print_copies_a_bounded_part_of_a_huge_string() {
    // Fifty 1 MB strings (the sandbox's own string.rep limit), each printed, and 40 unpacked.
    let src = "function f()\n local big = {}\n for i = 1, 50 do big[i] = string.rep('x', 1 << 20) end\n for i = 1, 50 do print(big[i]) end\n print(table.unpack(big, 1, 40))\nend\n";
    let r = rt(&[("scripts/t.lua", src)]);
    r.load_script("scripts/t.lua").unwrap();
    r.call::<()>("scripts/t.lua", "f", ()).unwrap();
    let log = r.take_log();
    let total: usize = log.iter().map(|l| l.len()).sum();
    assert!(total <= 64 << 10, "the log holds {total} bytes");
    assert!(log.iter().all(|l| l.len() <= 4 << 10), "a line is longer than 4 KB");
    assert!(log.len() <= 200);
    assert!(!r.exhausted());
    assert!(r.take_log().is_empty());
}

#[test]
fn a_catastrophic_pattern_is_rejected_at_once() {
    let src = "function f()\n local ok, e = pcall(string.find, string.rep('a', 60000), '.-.-.-.-.-.-.-.-.-x')\n return ok, e\nend\n";
    let r = rt(&[("scripts/t.lua", src)]);
    r.load_script("scripts/t.lua").unwrap();
    let started = std::time::Instant::now();
    let (ok, msg): (bool, String) = r.call("scripts/t.lua", "f", ()).unwrap().unwrap();
    assert!(started.elapsed() < std::time::Duration::from_millis(500), "took {:?}", started.elapsed());
    assert!(!ok);
    assert!(msg.contains("backtracking items"), "{msg}");
}

#[test]
fn a_nested_quantifier_is_bounded_by_the_subject_length() {
    // (a*)*b has two backtracking items: a subject of 60 KB would take about n^3 steps.
    let src = "function f()\n return pcall(string.find, string.rep('a', 60000), '(a*)*b')\nend\nfunction g()\n return string.find(string.rep('a', 583) .. 'b', '(a*)*b') == nil\nend\n";
    let r = rt(&[("scripts/t.lua", src)]);
    r.load_script("scripts/t.lua").unwrap();
    let (ok, msg): (bool, String) = r.call("scripts/t.lua", "f", ()).unwrap().unwrap();
    assert!(!ok);
    assert!(msg.contains("too long for this pattern"), "{msg}");
    assert_eq!(r.call::<bool>("scripts/t.lua", "g", ()).unwrap(), Some(true), "a pattern within the bound runs");
}

#[test]
fn a_normal_pattern_works_on_a_10_kb_subject() {
    let src = "function f()\n local s = string.rep('x', 10000) .. 'needle'\n return string.find(s, 'needle'), string.match(s, 'x-n')\nend\n";
    let r = rt(&[("scripts/t.lua", src)]);
    r.load_script("scripts/t.lua").unwrap();
    let (at, m): (i64, String) = r.call("scripts/t.lua", "f", ()).unwrap().unwrap();
    assert_eq!(at, 10001);
    assert_eq!(m.len(), 10001);
    assert!(m.ends_with('n') && m.starts_with('x'));
}

#[test]
fn gmatch_checks_the_deadline_on_every_step() {
    let src = "function make(s)\n G = string.gmatch(s, '%a')\nend\nfunction step()\n return G()\nend\n";
    let r = rt(&[("scripts/t.lua", src)]);
    r.load_script("scripts/t.lua").unwrap();
    r.call::<()>("scripts/t.lua", "make", "abc").unwrap();
    assert_eq!(r.call::<String>("scripts/t.lua", "step", ()).unwrap(), Some("a".to_string()));
    r.time_limit.set(std::time::Duration::ZERO);
    let e = r.call::<Value>("scripts/t.lua", "step", ()).unwrap_err();
    assert!(e.message.contains("time limit"), "{e}");
    assert!(r.exhausted());
}

#[test]
fn pattern_features_outside_the_subset_are_refused() {
    let src = "function try(p) return pcall(string.find, 'abc abc', p) end\n";
    let r = rt(&[("scripts/t.lua", src)]);
    r.load_script("scripts/t.lua").unwrap();
    for p in ["(a)%1", "%9", "%b()", "%f[%w]", "[%1]", "x[%b]y"] {
        let (ok, msg): (bool, String) = r.call("scripts/t.lua", "try", p).unwrap().unwrap();
        assert!(!ok, "{p} was allowed");
        assert!(msg.contains("pattern feature not allowed in Razdor scripts"), "{p}: {msg}");
    }
}

#[test]
fn an_escaped_percent_and_normal_patterns_work() {
    let src = "function f() return string.find('x%1', '%%1') end\nfunction g() return string.find('hello world', 'w%a+') end\nfunction h() return string.find(string.rep('a', 60000), '[*+]') end\n";
    let r = rt(&[("scripts/t.lua", src)]);
    r.load_script("scripts/t.lua").unwrap();
    assert_eq!(r.call::<Option<i64>>("scripts/t.lua", "f", ()).unwrap(), Some(Some(2)), "%%1 is a literal percent and a 1");
    assert_eq!(r.call::<Option<i64>>("scripts/t.lua", "g", ()).unwrap(), Some(Some(7)));
    // A quantifier inside a set is a set character, not a backtracking item: the long subject is allowed.
    assert_eq!(r.call::<Option<i64>>("scripts/t.lua", "h", ()).unwrap(), Some(None));
}

#[test]
fn an_oversized_rep_is_rejected() {
    let r = rt(&[("scripts/t.lua", "function f() return string.rep('x', 2000000) end\nfunction g() return #string.rep('x', 1 << 20) end\n")]);
    r.load_script("scripts/t.lua").unwrap();
    let e = r.call::<Value>("scripts/t.lua", "f", ()).unwrap_err();
    assert!(e.message.contains("longer than"), "{e}");
    assert_eq!(r.call::<i64>("scripts/t.lua", "g", ()).unwrap(), Some(1 << 20));
}

#[test]
fn oversized_subjects_patterns_and_concats_are_rejected() {
    let src = "function subj() return string.find(string.rep('a', 65537), 'b') end\nfunction pat() return string.find('abc', string.rep('a', 257)) end\nfunction cat() local t = {} for i = 1, 40 do t[i] = string.rep('x', 60000) end return table.concat(t) end\n";
    let r = rt(&[("scripts/t.lua", src)]);
    r.load_script("scripts/t.lua").unwrap();
    assert!(r.call::<Value>("scripts/t.lua", "subj", ()).unwrap_err().message.contains("subject"));
    assert!(r.call::<Value>("scripts/t.lua", "pat", ()).unwrap_err().message.contains("pattern"));
    assert!(r.call::<Value>("scripts/t.lua", "cat", ()).unwrap_err().message.contains("longer than"));
}
