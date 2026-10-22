//! What the server is told at start: the command line, else the
//! environment (`AF_*` variables, so a systemd `EnvironmentFile` can hold
//! the keys).

use std::time::Duration;

/// Everything the server needs to run.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerConfig {
    /// `host:port` to listen on.
    pub bind: String,
    /// The `assets/data` folder (`None`: found like the game finds it).
    pub data_dir: Option<String>,
    /// The Supabase project for ranked play; `None` disables ranked unless
    /// `ranked_memory` is set.
    pub supabase: Option<SupabaseConfig>,
    /// Ranked play with accounts kept in memory (lost at restart): for
    /// trying things out without a Supabase project.
    pub ranked_memory: bool,
    /// Domain of the synthetic account emails.
    pub email_domain: String,
    /// Shown to every player on connect.
    pub motd: String,
    /// How long a match waits for every player to load before its clock starts.
    pub load_timeout: Duration,
    /// A ranked player gone for this long forfeits.
    pub forfeit_after: Duration,
    /// A ranked desync forfeits the desynced player instead of voiding the match.
    pub desync_forfeits: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SupabaseConfig {
    pub url: String,
    pub anon_key: String,
    pub service_key: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            bind: format!("0.0.0.0:{}", af_net::DEFAULT_PORT),
            data_dir: None,
            supabase: None,
            ranked_memory: false,
            email_domain: "aetherfall.invalid".into(),
            motd: "Welcome to Aetherfall".into(),
            load_timeout: Duration::from_secs(20),
            forfeit_after: Duration::from_secs(60),
            desync_forfeits: false,
        }
    }
}

/// The options, as `--help` prints them.
pub const USAGE: &str = "af_server: the Aetherfall multiplayer server

Options (each may instead be an environment variable):
  --bind HOST:PORT        listen address               (AF_BIND, default 0.0.0.0:7777)
  --data DIR              the assets/data folder       (AF_DATA_DIR, default: found like the game does)
  --supabase-url URL      https://<ref>.supabase.co     (AF_SUPABASE_URL)
  --supabase-anon KEY     the publishable / anon key    (AF_SUPABASE_ANON_KEY)
  --supabase-service KEY  the service-role key          (AF_SUPABASE_SERVICE_KEY)
  --email-domain DOMAIN   domain of account emails      (AF_EMAIL_DOMAIN, default aetherfall.invalid)
  --motd TEXT             message shown on connect      (AF_MOTD)
  --ranked-memory         ranked play with in-memory accounts (AF_RANKED_MEMORY=1)
  --load-timeout SECS     wait for players to load      (AF_LOAD_TIMEOUT, default 20)
  --forfeit-after SECS    ranked: a dropped player forfeits after (AF_FORFEIT_AFTER, default 60)
  --desync-forfeits       ranked: a desync forfeits instead of voiding (AF_DESYNC_FORFEITS=1)
  -h, --help

Ranked play needs all three Supabase settings (or --ranked-memory); casual rooms work without.";

impl ServerConfig {
    /// From the command line and the environment (`env` looks a variable up).
    pub fn parse(args: impl IntoIterator<Item = String>, env: impl Fn(&str) -> Option<String>) -> Result<ServerConfig, String> {
        let mut c = ServerConfig::default();
        let take = |name: &str| env(name).filter(|v| !v.trim().is_empty());
        if let Some(v) = take("AF_BIND") {
            c.bind = v;
        }
        c.data_dir = take("AF_DATA_DIR");
        if let Some(v) = take("AF_EMAIL_DOMAIN") {
            c.email_domain = v;
        }
        if let Some(v) = take("AF_MOTD") {
            c.motd = v;
        }
        let mut url = take("AF_SUPABASE_URL");
        let mut anon = take("AF_SUPABASE_ANON_KEY");
        let mut service = take("AF_SUPABASE_SERVICE_KEY");
        c.ranked_memory = take("AF_RANKED_MEMORY").is_some_and(|v| v != "0");
        c.desync_forfeits = take("AF_DESYNC_FORFEITS").is_some_and(|v| v != "0");
        let secs = |name: &str, v: String| v.parse::<u64>().map(Duration::from_secs).map_err(|e| format!("{name}: {e}"));
        if let Some(v) = take("AF_LOAD_TIMEOUT") {
            c.load_timeout = secs("AF_LOAD_TIMEOUT", v)?;
        }
        if let Some(v) = take("AF_FORFEIT_AFTER") {
            c.forfeit_after = secs("AF_FORFEIT_AFTER", v)?;
        }
        let mut it = args.into_iter();
        let next = |it: &mut dyn Iterator<Item = String>, name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--bind" => c.bind = next(&mut it, "--bind")?,
                "--data" => c.data_dir = Some(next(&mut it, "--data")?),
                "--supabase-url" => url = Some(next(&mut it, "--supabase-url")?),
                "--supabase-anon" => anon = Some(next(&mut it, "--supabase-anon")?),
                "--supabase-service" => service = Some(next(&mut it, "--supabase-service")?),
                "--email-domain" => c.email_domain = next(&mut it, "--email-domain")?,
                "--motd" => c.motd = next(&mut it, "--motd")?,
                "--ranked-memory" => c.ranked_memory = true,
                "--desync-forfeits" => c.desync_forfeits = true,
                "--load-timeout" => c.load_timeout = secs("--load-timeout", next(&mut it, "--load-timeout")?)?,
                "--forfeit-after" => c.forfeit_after = secs("--forfeit-after", next(&mut it, "--forfeit-after")?)?,
                "-h" | "--help" => return Err(USAGE.into()),
                other => return Err(format!("unknown argument `{other}` (try --help)")),
            }
        }
        c.supabase = match (url, anon, service) {
            (Some(url), Some(anon_key), Some(service_key)) => Some(SupabaseConfig { url, anon_key, service_key }),
            (None, None, None) => None,
            _ => return Err("ranked play needs --supabase-url, --supabase-anon and --supabase-service together".into()),
        };
        Ok(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str], env: &[(&str, &str)]) -> Result<ServerConfig, String> {
        let env: Vec<(String, String)> = env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        ServerConfig::parse(args.iter().map(|s| s.to_string()), |k| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()))
    }

    #[test]
    fn defaults_then_env_then_args() {
        let c = parse(&[], &[]).unwrap();
        assert_eq!(c.bind, "0.0.0.0:7777");
        assert!(c.supabase.is_none() && !c.ranked_memory);
        let c = parse(&["--bind", "127.0.0.1:9", "--ranked-memory"], &[("AF_BIND", "0.0.0.0:8"), ("AF_MOTD", "hi"), ("AF_LOAD_TIMEOUT", "5")]).unwrap();
        assert_eq!((c.bind.as_str(), c.motd.as_str(), c.load_timeout, c.ranked_memory), ("127.0.0.1:9", "hi", Duration::from_secs(5), true));
    }

    #[test]
    fn supabase_settings_come_together_or_not_at_all() {
        let c = parse(&[], &[("AF_SUPABASE_URL", "https://x.supabase.co"), ("AF_SUPABASE_ANON_KEY", "a"), ("AF_SUPABASE_SERVICE_KEY", "s")]).unwrap();
        assert_eq!(c.supabase.unwrap().service_key, "s");
        assert!(parse(&["--supabase-url", "https://x"], &[]).unwrap_err().contains("together"));
        assert!(parse(&["--bogus"], &[]).unwrap_err().contains("unknown argument"));
        assert!(parse(&["--help"], &[]).unwrap_err().starts_with("af_server"));
        assert!(parse(&["--load-timeout", "x"], &[]).is_err());
    }
}
