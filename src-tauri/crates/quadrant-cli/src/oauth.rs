//! The loopback OAuth sign-in `AccountPage.tsx` runs, without the Tauri
//! OAuth plugin: a one-shot HTTP listener on 127.0.0.1 receives the redirect.

use anyhow::{Result, anyhow, bail};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use url::Url;

use crate::i18n;

const AUTHORIZE_URL: &str = "https://mrquantumoff.dev/account/oauth2/authorize";
const SCOPES: &str = "profile:read profile:write sync:read sync:write share:read share:write \
     settings:read settings:write notifications:read";
/// The redirect URIs registered for the Quadrant client.
const PORTS: std::ops::RangeInclusive<u16> = 4000..=4005;
const MAX_REQUEST_HEAD: usize = 16 * 1024;

pub fn new_state() -> Result<String> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).map_err(|error| anyhow!("no randomness available: {error}"))?;
    Ok(hex::encode(bytes))
}

pub fn authorize_url(client_id: &str, redirect_uri: &str, state: &str) -> Url {
    Url::parse_with_params(
        AUTHORIZE_URL,
        [
            ("client_id", client_id),
            ("redirect_uri", redirect_uri),
            ("scope", SCOPES),
            ("response_type", "code"),
            ("state", state),
        ],
    )
    .expect("the authorize URL is a valid base")
}

/// Listens on the first free registered port.
pub async fn bind() -> Result<(TcpListener, u16)> {
    for port in PORTS {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)).await {
            return Ok((listener, port));
        }
    }
    bail!(
        "ports {}-{} are all in use; close whatever holds them and try again",
        PORTS.start(),
        PORTS.end()
    )
}

#[derive(Debug, PartialEq, Eq)]
pub enum Callback {
    /// A request that isn't the redirect, such as the browser's favicon fetch.
    Ignored,
    Code {
        code: String,
        state: Option<String>,
    },
    Denied {
        error: String,
    },
}

/// Reads the redirect out of a request target like `/?code=..&state=..`.
pub fn parse_callback(target: &str) -> Callback {
    let Ok(url) = Url::parse("http://127.0.0.1").and_then(|base| base.join(target)) else {
        return Callback::Ignored;
    };
    let param = |name: &str| {
        url.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };
    match (param("code"), param("error")) {
        (_, Some(error)) => Callback::Denied { error },
        (Some(code), None) => Callback::Code {
            code,
            state: param("state"),
        },
        (None, None) => Callback::Ignored,
    }
}

/// Answers requests until the redirect arrives and returns its code, once
/// its state matches the one this sign-in started with.
pub async fn wait_for_callback(listener: TcpListener, expected_state: &str) -> Result<String> {
    loop {
        let (mut stream, _) = listener.accept().await?;
        let Some(target) = read_request_target(&mut stream).await else {
            continue;
        };
        match parse_callback(&target) {
            Callback::Ignored => {
                respond(&mut stream, "404 Not Found", "").await;
            }
            Callback::Denied { error } => {
                respond(&mut stream, "200 OK", &done_page()).await;
                bail!("the sign-in was not completed: {error}");
            }
            Callback::Code { code, state } => {
                respond(&mut stream, "200 OK", &done_page()).await;
                if state.as_deref() != Some(expected_state) {
                    bail!("the sign-in answer doesn't match this sign-in; try again");
                }
                return Ok(code);
            }
        }
    }
}

async fn read_request_target(stream: &mut TcpStream) -> Option<String> {
    let mut head = Vec::new();
    let mut buffer = [0u8; 1024];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut buffer).await.ok()?;
        if read == 0 || head.len() > MAX_REQUEST_HEAD {
            break;
        }
        head.extend_from_slice(&buffer[..read]);
    }
    let head = String::from_utf8_lossy(&head);
    let mut request_line = head.lines().next()?.split_whitespace();
    let _method = request_line.next()?;
    request_line.next().map(str::to_string)
}

async fn respond(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

fn done_page() -> String {
    let message = i18n::translate("returnToTheApp").unwrap_or("You can close this tab.");
    format!("<html><body><h1>{message}</h1></body></html>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_are_48_hex_characters_and_differ() {
        let first = new_state().unwrap();
        assert_eq!(first.len(), 48);
        assert!(first.chars().all(|character| character.is_ascii_hexdigit()));
        assert_ne!(first, new_state().unwrap());
    }

    #[test]
    fn authorize_url_encodes_every_parameter() {
        let url = authorize_url("client", "http://127.0.0.1:4000", "abc");
        let pairs: Vec<(String, String)> = url
            .query_pairs()
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        assert_eq!(url.path(), "/account/oauth2/authorize");
        assert!(pairs.contains(&("redirect_uri".into(), "http://127.0.0.1:4000".into())));
        assert!(pairs.contains(&("scope".into(), SCOPES.into())));
        assert!(pairs.contains(&("response_type".into(), "code".into())));
        assert!(!url.as_str().contains(' '));
    }

    #[test]
    fn callbacks_without_code_or_error_are_ignored() {
        assert_eq!(parse_callback("/favicon.ico"), Callback::Ignored);
        assert_eq!(
            parse_callback("/?code=xyz&state=abc"),
            Callback::Code {
                code: "xyz".into(),
                state: Some("abc".into())
            }
        );
        assert_eq!(
            parse_callback("/?error=access_denied&state=abc"),
            Callback::Denied {
                error: "access_denied".into()
            }
        );
    }

    async fn send(port: u16, target: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        stream
            .write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    }

    #[tokio::test]
    async fn loopback_skips_stray_requests_and_returns_the_code() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let waiting = tokio::spawn(wait_for_callback(listener, "abc"));

        assert!(send(port, "/favicon.ico").await.starts_with("HTTP/1.1 404"));
        let page = send(port, "/?code=the-code&state=abc").await;
        assert!(page.starts_with("HTTP/1.1 200"));
        assert!(page.contains("<h1>"));
        assert_eq!(waiting.await.unwrap().unwrap(), "the-code");
    }

    #[tokio::test]
    async fn loopback_rejects_a_foreign_state() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let waiting = tokio::spawn(wait_for_callback(listener, "abc"));

        send(port, "/?code=the-code&state=someone-else").await;
        assert!(waiting.await.unwrap().is_err());
    }
}
