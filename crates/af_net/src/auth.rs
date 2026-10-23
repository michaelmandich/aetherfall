//! The account service: Supabase Auth over HTTPS.
//!
//! Players sign up and log in with a username and a password. Supabase
//! wants an email, so the username becomes `<username>@<email domain>`
//! (the domain is the server's; it never receives mail). The client talks
//! to Supabase directly, so the password only ever travels over TLS, and
//! hands the server the short-lived access token it gets back; the server
//! asks Supabase who that token belongs to ([`get_user`]).
//!
//! Everything here blocks: call it from a thread, never from a Bevy system.

use std::time::Duration;

use af_core::ext::net::{valid_username, PASSWORD_MIN};
use serde::Deserialize;
use ureq::Agent;

/// Where and how to reach the project's auth endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SupabaseAuth {
    /// `https://<ref>.supabase.co`
    pub url: String,
    /// The project's publishable (anon) key.
    pub anon_key: String,
    /// Domain of the synthetic emails.
    pub email_domain: String,
}

/// Who an access token belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthUser {
    /// The Supabase user id (a UUID).
    pub id: String,
    pub username: String,
}

fn agent() -> Agent {
    Agent::config_builder().timeout_global(Some(Duration::from_secs(15))).http_status_as_error(false).build().into()
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    user: Option<UserResponse>,
    // Error shapes Supabase uses, old and new.
    error: Option<String>,
    error_description: Option<String>,
    msg: Option<String>,
    message: Option<String>,
    error_code: Option<String>,
}

#[derive(Deserialize)]
struct UserResponse {
    id: String,
    #[serde(default)]
    user_metadata: serde_json::Value,
    #[serde(default)]
    email: Option<String>,
}

/// The message in a Supabase error body, whatever shape it took.
fn error_text(status: u16, body: &str) -> String {
    let parsed: Option<TokenResponse> = serde_json::from_str(body).ok();
    let text = parsed.and_then(|p| p.error_description.or(p.msg).or(p.message).or(p.error).or(p.error_code));
    match text {
        Some(t) => friendly(&t),
        None if body.trim().is_empty() => format!("the account service answered {status}"),
        None => format!("the account service answered {status}: {}", body.chars().take(120).collect::<String>()),
    }
}

/// Supabase's wording, turned into the game's.
fn friendly(t: &str) -> String {
    let lower = t.to_ascii_lowercase();
    if lower.contains("already registered") || lower.contains("already been registered") || lower.contains("user_already_exists") {
        "that username is taken".into()
    } else if lower.contains("invalid login credentials") || lower.contains("invalid_credentials") {
        "wrong username or password".into()
    } else if lower.contains("email not confirmed") || lower.contains("email_not_confirmed") {
        "the account service wants emails confirmed: turn off \"Confirm email\" in its Auth settings".into()
    } else if lower.contains("signups not allowed") || lower.contains("signup_disabled") {
        "the account service does not allow sign-ups".into()
    } else if lower.contains("email address") && lower.contains("invalid") {
        "the account service refused the username".into()
    } else {
        t.to_string()
    }
}

impl SupabaseAuth {
    /// The synthetic email of `username` (case-insensitive).
    pub fn email_for(&self, username: &str) -> String {
        format!("{}@{}", username.to_ascii_lowercase(), self.email_domain)
    }

    fn check(username: &str, password: &str) -> Result<(), String> {
        if !valid_username(username) {
            return Err("a username is 3 to 16 letters, digits or underscores".into());
        }
        if password.chars().count() < PASSWORD_MIN {
            return Err(format!("a password has at least {PASSWORD_MIN} characters"));
        }
        Ok(())
    }

    fn token_call(&self, path: &str, body: serde_json::Value) -> Result<String, String> {
        let url = format!("{}{}", self.url.trim_end_matches('/'), path);
        let mut response = agent()
            .post(&url)
            .header("apikey", &self.anon_key)
            .header("Content-Type", "application/json")
            .send(body.to_string().as_bytes())
            .map_err(|e| format!("could not reach the account service: {e}"))?;
        let status = response.status().as_u16();
        let text = response.body_mut().read_to_string().map_err(|e| format!("account service: {e}"))?;
        if !(200..300).contains(&status) {
            return Err(error_text(status, &text));
        }
        let parsed: TokenResponse = serde_json::from_str(&text).map_err(|e| format!("account service: unexpected answer ({e})"))?;
        match parsed.access_token {
            Some(token) => Ok(token),
            // Sign-up with email confirmation on: a user (nested, or the
            // whole answer), no session.
            None if parsed.user.is_some() || serde_json::from_str::<UserResponse>(&text).is_ok() => Err(friendly("email not confirmed")),
            None => Err(error_text(status, &text)),
        }
    }

    /// Create the account and log in. Returns the access token.
    pub fn sign_up(&self, username: &str, password: &str) -> Result<String, String> {
        Self::check(username, password)?;
        let body = serde_json::json!({
            "email": self.email_for(username),
            "password": password,
            "data": { "username": username },
        });
        self.token_call("/auth/v1/signup", body)
    }

    /// Log in. Returns the access token.
    pub fn sign_in(&self, username: &str, password: &str) -> Result<String, String> {
        Self::check(username, password)?;
        let body = serde_json::json!({ "email": self.email_for(username), "password": password });
        self.token_call("/auth/v1/token?grant_type=password", body)
    }
}

/// Who `access_token` belongs to, asked of the auth endpoint at `url` with
/// `apikey` (the server's key). The username comes from the sign-up
/// metadata, else from the email's local part.
pub fn get_user(url: &str, apikey: &str, access_token: &str) -> Result<AuthUser, String> {
    let endpoint = format!("{}/auth/v1/user", url.trim_end_matches('/'));
    let mut response = agent()
        .get(&endpoint)
        .header("apikey", apikey)
        .header("Authorization", &format!("Bearer {access_token}"))
        .call()
        .map_err(|e| format!("could not reach the account service: {e}"))?;
    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string().map_err(|e| format!("account service: {e}"))?;
    if !(200..300).contains(&status) {
        return Err(error_text(status, &text));
    }
    let user: UserResponse = serde_json::from_str(&text).map_err(|e| format!("account service: unexpected answer ({e})"))?;
    let from_meta = user.user_metadata.get("username").and_then(|v| v.as_str()).map(str::to_string);
    let from_email = user.email.as_deref().and_then(|e| e.split('@').next()).map(str::to_string);
    let username = from_meta.or(from_email).filter(|u| valid_username(u)).ok_or("the account has no usable username")?;
    Ok(AuthUser { id: user.id, username })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Serves one HTTP request with `status` and `body`, returning what it got.
    fn one_shot_server(status: u16, body: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 65536];
            let mut got = Vec::new();
            loop {
                let n = s.read(&mut buf).unwrap();
                got.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&got).to_string();
                if let Some(i) = text.find("\r\n\r\n") {
                    let len = text[..i]
                        .lines()
                        .find_map(|l| l.strip_prefix("Content-Length: ").or_else(|| l.strip_prefix("content-length: ")))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if got.len() >= i + 4 + len {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let reply = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            s.write_all(reply.as_bytes()).unwrap();
            String::from_utf8_lossy(&got).to_string()
        });
        (url, handle)
    }

    fn auth(url: &str) -> SupabaseAuth {
        SupabaseAuth { url: url.into(), anon_key: "anon".into(), email_domain: "aetherfall.invalid".into() }
    }

    #[test]
    fn usernames_become_lowercase_emails() {
        assert_eq!(auth("x").email_for("Fin_42"), "fin_42@aetherfall.invalid");
    }

    #[test]
    fn sign_in_posts_the_password_grant_and_returns_the_token() {
        let (url, server) = one_shot_server(200, r#"{"access_token":"tok123","token_type":"bearer","user":{"id":"u1"}}"#);
        let token = auth(&url).sign_in("Fin", "longpassword").unwrap();
        assert_eq!(token, "tok123");
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /auth/v1/token?grant_type=password HTTP/1.1"), "{request}");
        assert!(request.contains("apikey: anon"), "{request}");
        assert!(request.contains(r#""email":"fin@aetherfall.invalid""#), "{request}");
    }

    #[test]
    fn sign_up_sends_the_username_as_metadata() {
        let (url, server) = one_shot_server(200, r#"{"access_token":"t","user":{"id":"u1"}}"#);
        auth(&url).sign_up("Fin", "longpassword").unwrap();
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /auth/v1/signup HTTP/1.1"), "{request}");
        assert!(request.contains(r#""data":{"username":"Fin"}"#), "{request}");
    }

    #[test]
    fn errors_are_turned_into_plain_words() {
        let (url, server) = one_shot_server(400, r#"{"error_code":"invalid_credentials","msg":"Invalid login credentials"}"#);
        assert_eq!(auth(&url).sign_in("Fin", "longpassword").unwrap_err(), "wrong username or password");
        server.join().unwrap();
        let (url, server) = one_shot_server(422, r#"{"code":422,"error_code":"user_already_exists","msg":"User already registered"}"#);
        assert_eq!(auth(&url).sign_up("Fin", "longpassword").unwrap_err(), "that username is taken");
        server.join().unwrap();
        let (url, server) = one_shot_server(200, r#"{"id":"u1","aud":"authenticated","email":"fin@aetherfall.invalid","user_metadata":{}}"#);
        assert_eq!(auth(&url).sign_up("Fin", "longpassword").unwrap_err(), friendly("email not confirmed"));
        server.join().unwrap();
        assert_eq!(auth("http://127.0.0.1:1").sign_in("ab", "x").unwrap_err(), "a username is 3 to 16 letters, digits or underscores");
        assert_eq!(auth("http://127.0.0.1:1").sign_in("abc", "short").unwrap_err(), "a password has at least 8 characters");
    }

    #[test]
    fn get_user_reads_the_token_owner() {
        let (url, server) = one_shot_server(200, r#"{"id":"abc-123","email":"fin@aetherfall.invalid","user_metadata":{"username":"Fin"}}"#);
        let user = get_user(&url, "service", "tok").unwrap();
        assert_eq!(user, AuthUser { id: "abc-123".into(), username: "Fin".into() });
        let request = server.join().unwrap();
        assert!(request.starts_with("GET /auth/v1/user HTTP/1.1"), "{request}");
        assert!(request.contains("authorization: Bearer tok") || request.contains("Authorization: Bearer tok"), "{request}");
        let (url, server) = one_shot_server(401, r#"{"code":401,"msg":"invalid JWT"}"#);
        assert_eq!(get_user(&url, "service", "tok").unwrap_err(), "invalid JWT");
        server.join().unwrap();
    }
}
