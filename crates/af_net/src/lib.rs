//! Aetherfall multiplayer: the wire protocol shared with `af_server`, TCP
//! framing, the lockstep client plugin and the account-service client.
//!
//! * [`protocol`] — every message, and commands in wire form.
//! * [`framing`] — length-prefixed frames on a TCP stream.
//! * [`lockstep`] — turn buffer and clock pacing (pure logic).
//! * [`client`] — [`NetClientPlugin`]: connects, keeps `NetSession` for the
//!   UI, drives the networked match (turns in, commands and hashes out).
//! * [`auth`] — sign up / log in against Supabase Auth; token lookup.
//! * [`config`] — the remembered server and username.
//!
//! The UI never sees this crate: it reads `af_core::ext::net::NetSession`
//! and pushes `af_core::ext::net::NetAction`s.

pub mod auth;
pub mod client;
pub mod config;
pub mod framing;
pub mod lockstep;
pub mod protocol;

pub use client::NetClientPlugin;

/// The port the server listens on unless told otherwise.
pub const DEFAULT_PORT: u16 = 7777;

/// `host:port` for what the player typed: a bare host or IP gets
/// [`DEFAULT_PORT`]; whitespace is trimmed.
pub fn server_addr(input: &str) -> String {
    let s = input.trim();
    let has_port = match s.rsplit_once(':') {
        // An IPv6 literal without brackets has several colons and no port.
        Some((host, port)) => !host.contains(':') && port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty(),
        None => false,
    };
    if has_port || s.starts_with('[') { s.to_string() } else { format!("{s}:{DEFAULT_PORT}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_hosts_get_the_default_port() {
        assert_eq!(server_addr(" pi.local "), "pi.local:7777");
        assert_eq!(server_addr("192.168.1.20"), "192.168.1.20:7777");
        assert_eq!(server_addr("example.net:9000"), "example.net:9000");
        assert_eq!(server_addr("[::1]:7777"), "[::1]:7777");
    }
}
