//! Razdor Studio's MCP server (Model Context Protocol, spec 2025-06-18), so an agent can see and
//! test a package while it edits it: the studio's state, the checks, the story's trace, the
//! references of a name, the focus, and a picture of the canvas. The agent edits the files itself;
//! there is no edit tool (the studio's watcher picks the files up).
//!
//! The parts: `rpc` (JSON-RPC 2.0 and the MCP methods), `tools` (the six tools, their schemas and
//! replies), `server` (the TCP listener on 127.0.0.1 with a random token, the runtime file), `bridge`
//! (`razdor --studio-mcp`, the stdio bridge an agent starts), `image` (the canvas picture). The tools
//! that need the window's state go through [`Host`]: the window answers them on its next frame. This
//! module has no window and no UI toolkit.

pub mod bridge;
pub mod host;
pub mod image;
pub mod rpc;
pub mod server;
pub mod tools;

#[cfg(test)]
mod tests;

pub use host::{FocusTarget, Frame, Host, SnapTarget};

/// The MCP protocol version the server speaks.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// The server's name and version in `initialize`.
pub const SERVER_NAME: &str = "razdor-studio";
