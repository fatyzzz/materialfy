use std::io;

use base64::Engine as _;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use url::Url;

/// `Proxy-Authorization` value for a proxy URL with a user name (and password), percent-decoded.
pub fn basic_auth(proxy: &Url) -> Option<String> {
    if proxy.username().is_empty() {
        return None;
    }
    let decode = |part: &str| percent_decode(part);
    let pair = format!(
        "{}:{}",
        decode(proxy.username()),
        decode(proxy.password().unwrap_or(""))
    );
    Some(format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(pair)
    ))
}

/// `%XX` → byte (URL userinfo is percent-encoded); anything malformed stays as is.
fn percent_decode(part: &str) -> String {
    let bytes = part.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub async fn proxy_connect<T: AsyncRead + AsyncWrite + Unpin>(
    mut proxy_connection: T,
    connect_host: &str,
    connect_port: &str,
    authorization: Option<&str>,
) -> io::Result<T> {
    // HTTP/1.1 requires Host; strict proxies refuse a CONNECT without it
    let target = format!("{connect_host}:{connect_port}");
    let mut request = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n");
    if let Some(authorization) = authorization {
        request.push_str(&format!("Proxy-Authorization: {authorization}\r\n"));
    }
    request.push_str("\r\n");
    let mut buffer = request.into_bytes();

    proxy_connection.write_all(buffer.as_ref()).await?;

    buffer.resize(buffer.capacity(), 0);

    let mut offset = 0;
    loop {
        let bytes_read = proxy_connection.read(&mut buffer[offset..]).await?;
        if bytes_read == 0 {
            return Err(io::Error::other("Early EOF from proxy"));
        }
        offset += bytes_read;

        let mut headers = [httparse::EMPTY_HEADER; 16];
        let mut response = httparse::Response::new(&mut headers);

        let status = response
            .parse(&buffer[..offset])
            .map_err(io::Error::other)?;

        if status.is_complete() {
            return match response.code {
                Some(200) => Ok(proxy_connection), // Proxy says all is well
                Some(code) => {
                    let reason = response.reason.unwrap_or("no reason");
                    let msg = format!("Proxy responded with {code}: {reason}");
                    Err(io::Error::other(msg))
                }
                None => Err(io::Error::other("Malformed response from proxy")),
            };
        }

        if offset >= buffer.len() {
            buffer.resize(buffer.len() + 100, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[test]
    fn basic_auth_decodes_the_url_login() {
        let url = Url::parse("http://us%40er:p%3Ass@127.0.0.1:10808").unwrap();
        let expected = base64::engine::general_purpose::STANDARD.encode("us@er:p:ss");
        assert_eq!(basic_auth(&url), Some(format!("Basic {expected}")));
        assert_eq!(
            basic_auth(&Url::parse("http://127.0.0.1:10808").unwrap()),
            None
        );
    }

    #[tokio::test]
    async fn connect_sends_host_and_proxy_authorization() {
        let (client, mut proxy) = duplex(4096);
        let server = tokio::spawn(async move {
            let mut seen = Vec::new();
            let mut buf = [0u8; 512];
            while !seen.ends_with(b"\r\n\r\n") {
                let n = proxy.read(&mut buf).await.unwrap();
                seen.extend_from_slice(&buf[..n]);
            }
            proxy
                .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .await
                .unwrap();
            String::from_utf8(seen).unwrap()
        });
        proxy_connect(client, "ap.spotify.com", "443", Some("Basic eA=="))
            .await
            .unwrap();
        let request = server.await.unwrap();
        assert!(request.starts_with("CONNECT ap.spotify.com:443 HTTP/1.1\r\n"));
        assert!(request.contains("\r\nHost: ap.spotify.com:443\r\n"));
        assert!(request.contains("\r\nProxy-Authorization: Basic eA==\r\n"));
    }
}
