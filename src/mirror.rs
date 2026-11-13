//! The addresses Razdor reads and downloads from, each list in the order it is tried: GitVerse
//! first (players in Russia reach it), GitHub as the fallback. See
//! `docs/specs/2026-11-13-gitverse-mirrors.md`. Nothing here needs macroquad.

/// The update feed: the release list in the GitHub API's shape. GitVerse's copy is a static file
/// on the community's Pages; GitHub's is the API.
pub const UPDATE_FEED: [&str; 2] = [
    "https://indicozy.gitverse.site/razdor-community/releases.json",
    "https://api.github.com/repos/indicozy/razdor/releases?per_page=30",
];

/// The hub catalog's folder: its `index.json` names the files, resolved against the folder that
/// gave the catalog. GitHub's is `hub::DEFAULT_SOURCE`.
pub const HUB: [&str; 2] = ["https://indicozy.gitverse.site/razdor-community", crate::hub::DEFAULT_SOURCE];

/// The game archive (`gdrive::GAME`): GitVerse's attachment, then GitHub's release file.
pub const GAME: [&str; 2] = [
    "https://gitverse.ru/api/attachments/36c22adf-f3f3-4286-a474-9b6a00a5abc5",
    "https://github.com/indicozy/razdor-community/releases/download/game-cu-1.2/discord-times-cu.zip",
];

/// Tries the addresses in order: `attempt` reads what one of them gives, or says why not. The first
/// `Ok` is the answer. Each failure is logged with its address and reason; when none answers, the
/// last reason is the error.
pub fn first_ok<T>(what: &str, addresses: &[&str], mut attempt: impl FnMut(&str) -> Result<T, String>) -> Result<T, String> {
    let mut last = String::from("no address");
    for &address in addresses {
        match attempt(address) {
            Ok(value) => {
                crate::diag!("{what}: from {address}");
                return Ok(value);
            }
            Err(why) => {
                crate::diag!("{what}: {address}: {why}");
                last = why;
            }
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn the_first_answer_wins_and_later_addresses_are_not_asked() {
        let asked = RefCell::new(Vec::new());
        let got = first_ok("test", &["a", "b", "c"], |a| {
            asked.borrow_mut().push(a.to_string());
            if a == "a" { Err("connection refused".into()) } else { Ok(a.to_uppercase()) }
        });
        assert_eq!(got, Ok("B".to_string()));
        assert_eq!(asked.borrow().as_slice(), &["a", "b"], "c is never asked");
    }

    #[test]
    fn every_fallback_reason_moves_on_and_the_last_reason_is_the_error() {
        let reasons = ["curl: (6) could not resolve", "HTTP 404", "stalled", "reply: expected value at line 1", "size is wrong"];
        let addresses = ["a1", "a2", "a3", "a4", "a5"];
        let asked = RefCell::new(0usize);
        let got: Result<(), String> = first_ok("test", &addresses, |_| {
            let n = *asked.borrow();
            *asked.borrow_mut() += 1;
            Err(reasons[n].to_string())
        });
        assert_eq!(*asked.borrow(), 5, "every address is tried");
        assert_eq!(got, Err("size is wrong".to_string()));
    }

    #[test]
    fn the_addresses_are_gitverse_first_and_github_last() {
        assert!(UPDATE_FEED[0].starts_with("https://indicozy.gitverse.site/"));
        assert!(UPDATE_FEED[1].starts_with("https://api.github.com/"));
        assert!(HUB[0].starts_with("https://indicozy.gitverse.site/"));
        assert_eq!(HUB[1], crate::hub::DEFAULT_SOURCE);
        assert!(GAME[0].starts_with("https://gitverse.ru/api/attachments/"));
        assert!(GAME[1].starts_with("https://github.com/"));
    }
}
