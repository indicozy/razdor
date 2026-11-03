//! The Claude dock's launch, without a window: the program (the user's own `claude`, found on PATH),
//! its arguments (the studio's MCP server registered with `--mcp-config`, the `razdor-studio` skill
//! text in `--append-system-prompt`), and the MCP config file in the temporary folder. Nothing here
//! starts a process: the dock does, when the author presses «Запустить Claude».

use std::path::{Path, PathBuf};

/// The skill the agent gets with the studio: how to work on a package with the MCP tools.
pub const SKILL: &str = include_str!("../../data/studio/claude-skill.md");

/// Where Claude Code is installed from (shown when it is not on PATH).
pub const INSTALL_URL: &str = "https://claude.com/claude-code";

/// The MCP server's name in the config (the agent's tools are `mcp__razdor-studio__<tool>`).
pub const SERVER_NAME: &str = "razdor-studio";

/// Test-only: a program the dock runs instead of `claude`, with no arguments.
pub const PROGRAM_OVERRIDE_ENV: &str = "RAZDOR_STUDIO_CLAUDE";

/// The program the dock starts and its arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// Where the `claude` program is: the override (tests), else the first `claude` on PATH. `None`
/// when there is none (the dock then says how to install it).
pub fn find_program() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(PROGRAM_OVERRIDE_ENV).map(PathBuf::from).filter(|p| !p.as_os_str().is_empty()) {
        return Some(p);
    }
    find_on_path("claude", &std::env::var_os("PATH")?)
}

/// The first executable named `name` in the `PATH` value (with `.exe` and `.cmd` on Windows).
pub fn find_on_path(name: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
    let names: Vec<String> = if cfg!(windows) { vec![format!("{name}.exe"), format!("{name}.cmd"), name.to_string()] } else { vec![name.to_string()] };
    std::env::split_paths(path).flat_map(|dir| names.iter().map(move |n| dir.join(n))).find(|p| is_file(p))
}

fn is_file(p: &Path) -> bool {
    p.is_file()
}

/// The MCP config that registers the studio's server: `razdor-studio` runs this executable with
/// `--studio-mcp` (the stdio bridge, which finds the running studio by its runtime file).
pub fn mcp_config(exe: &Path) -> String {
    serde_json::json!({
        "mcpServers": {
            SERVER_NAME: { "command": exe.display().to_string(), "args": ["--studio-mcp"] }
        }
    })
    .to_string()
}

/// The launch of `claude` for a package: the MCP config (its JSON text, which `--mcp-config` takes
/// as it is: no file is left behind) and the skill, with no prompt (the session starts and waits for
/// the author). `program` is what [`find_program`] found.
pub fn launch(program: PathBuf, mcp_config_json: &str) -> Launch {
    if std::env::var_os(PROGRAM_OVERRIDE_ENV).is_some_and(|v| !v.is_empty()) {
        return Launch { program, args: Vec::new() };
    }
    Launch {
        program,
        args: vec![
            "--mcp-config".into(),
            mcp_config_json.to_string(),
            "--append-system-prompt".into(),
            SKILL.to_string(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn the_mcp_config_registers_the_bridge_under_the_server_name() {
        let json: serde_json::Value = serde_json::from_str(&mcp_config(Path::new("/opt/razdor/razdor"))).unwrap();
        let server = &json["mcpServers"][SERVER_NAME];
        assert_eq!(server["command"], "/opt/razdor/razdor");
        assert_eq!(server["args"], serde_json::json!(["--studio-mcp"]));
    }

    #[test]
    fn the_launch_registers_the_server_and_the_skill_without_a_prompt() {
        let config = mcp_config(Path::new("/opt/razdor/razdor"));
        let launch = launch(PathBuf::from("/usr/bin/claude"), &config);
        assert_eq!(launch.args[0], "--mcp-config");
        assert_eq!(launch.args[1], config);
        assert_eq!(launch.args[2], "--append-system-prompt");
        assert_eq!(launch.args[3], SKILL);
        assert_eq!(launch.args.len(), 4, "no prompt argument: the session waits for the author");
    }

    #[test]
    fn the_skill_is_the_text_the_binary_ships() {
        assert!(SKILL.starts_with("# Razdor Studio"), "{}", &SKILL[..SKILL.len().min(80)]);
        assert!(SKILL.lines().count() <= 170, "the skill is about 150 lines");
    }

    #[test]
    fn a_program_is_found_on_the_given_path_only() {
        let dir = std::env::temp_dir().join(format!("razdor-claude-path-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let name = if cfg!(windows) { "claude.exe" } else { "claude" };
        assert_eq!(find_on_path("claude", std::ffi::OsStr::new(dir.to_str().unwrap())), None);
        fs::write(dir.join(name), "").unwrap();
        assert_eq!(find_on_path("claude", std::ffi::OsStr::new(dir.to_str().unwrap())), Some(dir.join(name)));
        let _ = fs::remove_dir_all(&dir);
    }
}
