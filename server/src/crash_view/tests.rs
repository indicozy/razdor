use super::*;
use crate::civil::ymd;

const LOC: &str = "battle.rs:10";
const MSG: &str = "index out of bounds";

fn db() -> Connection {
    let c = Connection::open_in_memory().expect("opens");
    c.execute_batch(crate::schema::STATS).expect("schema");
    c
}

fn crash(c: &Connection, id: &str, install: &str, day: i64, loc: &str, msg: &str, quarantined: i64) {
    c.execute(
        "INSERT INTO crashes (id, install_id, received_at, day, razdor, platform, game, screen, panic_loc,
           panic_msg, has_save, report_file, quarantined)
         VALUES (?1, ?2, ?3, ?4, '1.5.5', 'linux x86_64 64', 'base', 'battle', ?5, ?6, 0, ?1 || '.json.zst', ?7)",
        params![id, install, day * DAY + 3723, day, loc, msg, quarantined],
    )
    .expect("inserts a crash");
}

fn perf(c: &Connection, install: &str, received: i64, phone: &str, gpu: &str, quarantined: i64) {
    c.execute(
        "INSERT INTO perf (install_id, received_at, day, razdor, gpu, phone, screen, median_ms, p99_ms, payload, quarantined)
         VALUES (?1, ?2, 1, '1.5.5', ?3, ?4, '1920x1080', 16.0, 30.0, '{}', ?5)",
        params![install, received, gpu, phone, quarantined],
    )
    .expect("inserts a perf report");
}

#[test]
fn join_takes_the_latest_perf_report_of_the_same_install() {
    let c = db();
    crash(&c, "c1", "inst-a", 100, LOC, MSG, 0);
    perf(&c, "inst-a", 10, "Old phone", "Old GPU", 0);
    perf(&c, "inst-a", 50, "Pixel 8", "Mali-G715", 0);
    perf(&c, "inst-a", 40, "Middle", "Middle GPU", 0);
    perf(&c, "inst-a", 99, "Quarantined", "Quarantined GPU", 1);
    let (items, total) = group_items(&c, LOC, MSG, 0).expect("reads");
    assert_eq!(total, 1);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].device, Device { phone: Some("Pixel 8".into()), gpu: Some("Mali-G715".into()) });
    assert_eq!(items[0].received_at, 100 * DAY + 3723);
}

#[test]
fn join_reads_unknown_without_a_perf_report() {
    let c = db();
    crash(&c, "c1", "inst-b", 100, LOC, MSG, 0);
    // A desktop install sends no phone name: the phone is unknown, the GPU is known.
    crash(&c, "c2", "inst-c", 100, LOC, MSG, 0);
    perf(&c, "inst-c", 5, "", "RTX 5060 Ti", 0);
    let (items, _) = group_items(&c, LOC, MSG, 0).expect("reads");
    let device_of = |id: &str| items.iter().find(|i| i.id == id).expect("listed").device.clone();
    assert_eq!(device_of("c1"), Device::default());
    assert_eq!(device_of("c2"), Device { phone: None, gpu: Some("RTX 5060 Ti".into()) });
}

#[test]
fn group_filters_window_quarantine_and_other_groups() {
    let c = db();
    crash(&c, "in", "i", 100, LOC, MSG, 0);
    crash(&c, "old", "i", 10, LOC, MSG, 0);
    crash(&c, "quar", "i", 100, LOC, MSG, 1);
    crash(&c, "other", "i", 100, LOC, "other msg", 0);
    let (items, total) = group_items(&c, LOC, MSG, 50).expect("reads");
    assert_eq!(total, 1);
    assert_eq!(items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), vec!["in"]);
}

#[test]
fn group_href_percent_encodes_the_key() {
    assert_eq!(query_escape("a b&c=d/\u{e9}"), "a%20b%26c%3Dd%2F%C3%A9");
    assert_eq!(query_escape("az09-_.~"), "az09-_.~");
    assert_eq!(group_href("x.rs:1", "a&b"), "/admin/crash?loc=x.rs%3A1&msg=a%26b");
}

#[test]
fn utc_time_formats_day_and_clock() {
    assert_eq!(utc_time(0), "1970-01-01 00:00:00");
    assert_eq!(utc_time(DAY + 3723), format!("{} 01:02:03", ymd(1)));
}

fn sample_item(platform: &str, phone: Option<&str>) -> CrashItem {
    CrashItem {
        id: "c".into(),
        received_at: 0,
        razdor: "1.5.5".into(),
        platform: platform.into(),
        game: "base".into(),
        screen: "<screen>".into(),
        report_file: "c.json.zst".into(),
        has_save: false,
        device: Device { phone: phone.map(String::from), gpu: None },
    }
}

#[test]
fn render_escapes_player_text_and_marks_unknown_devices() {
    let item = sample_item("<script>p()</script>", Some("<b>Phone</b>"));
    let html = render(LOC, MSG, &[Detail { item, report: None }], 1);
    assert!(!html.contains("<script>p()"));
    assert!(!html.contains("<b>Phone"));
    assert!(html.contains("&lt;b&gt;Phone&lt;/b&gt;"));
    assert!(html.contains("&lt;screen&gt;"));
    assert!(html.contains("<span class=\"muted\">unknown</span>"));
    assert!(html.contains("The stored report is missing or unreadable."));
}

#[test]
fn render_shows_settings_and_only_the_log_tail() {
    let report: CrashReport = serde_json::from_value(serde_json::json!({
        "schema": 1,
        "install_id": "0123456789abcdef0123456789abcdef",
        "razdor": "1.5.5",
        "platform": "linux",
        "game": "base",
        "screen": "battle",
        "at": 0,
        "settings": {"quality": "<i>high</i>"},
        "panic_text": "panicked at x\n<tag>",
        "log": (1..=100).map(|i| format!("line {i}")).collect::<Vec<_>>(),
        "has_save": false
    }))
    .expect("parses");
    let html = render(LOC, MSG, &[Detail { item: sample_item("linux", None), report: Some(report) }], 1);
    assert!(html.contains("&lt;i&gt;high&lt;/i&gt;"));
    assert!(html.contains("&lt;tag&gt;"));
    assert!(html.contains("line 100"));
    assert!(html.contains("line 41\n"));
    assert!(!html.contains("line 40\n"));
    assert!(html.contains("Log tail (60 of 100 lines)"));
}

#[test]
fn unreadable_report_file_reads_as_none() {
    assert!(read_report(std::path::Path::new("/nonexistent/razdor-test/x.json.zst")).is_none());
    let dir = std::env::temp_dir().join(format!("razdor-crash-view-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let file = dir.join("bad.json.zst");
    std::fs::write(&file, b"not zstd").expect("writes");
    assert!(read_report(&file).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
