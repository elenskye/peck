//! `web_fetch` 渲染时给 WebKit 的代理（v1.2.1）：只听 127.0.0.1 的 SOCKS5，渲染完就关。
//!
//! WebKit 的每条连接（页面、图片、脚本、fetch、WebSocket）都经过这里：Peck 自己解析主机名，解析出来有一个本机、内网地址
//! 就拒（和 `web.rs` 的 `check_host` 同一条规矩），连的是查过的那个 IP（DNS 重绑换不了）。只认 CONNECT，UDP 不转。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// 在跑的代理；丢掉就关（连着的连接一起断）。
pub(super) struct Proxy {
    pub port: u16,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// 起一个代理。`allow`：另外放行的那一个地址（只有测试用）。
pub(super) async fn start(allow: Option<SocketAddr>) -> std::io::Result<Proxy> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let port = listener.local_addr()?.port();
    let task = tokio::spawn(async move {
        let mut conns = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                accepted = listener.accept() => match accepted {
                    Ok((stream, _)) => {
                        conns.spawn(serve(stream, allow));
                    }
                    Err(_) => break,
                },
                Some(_) = conns.join_next(), if !conns.is_empty() => {}
            }
        }
    });
    Ok(Proxy { port, task })
}

/// SOCKS5 的回复码。
const NOT_ALLOWED: u8 = 2;
const UNREACHABLE: u8 = 4;
const COMMAND_NOT_SUPPORTED: u8 = 7;
const ADDRESS_NOT_SUPPORTED: u8 = 8;

async fn reply(stream: &mut TcpStream, code: u8) -> std::io::Result<()> {
    stream.write_all(&[5, code, 0, 1, 0, 0, 0, 0, 0, 0]).await
}

async fn serve(mut stream: TcpStream, allow: Option<SocketAddr>) -> std::io::Result<()> {
    let Ok(target) = tokio::time::timeout(Duration::from_secs(10), handshake(&mut stream, allow)).await else { return Ok(()) };
    let Some(addrs) = target? else { return Ok(()) };
    let mut upstream = None;
    for addr in addrs {
        if let Ok(Ok(s)) = tokio::time::timeout(Duration::from_secs(15), TcpStream::connect(addr)).await {
            upstream = Some(s);
            break;
        }
    }
    let Some(mut upstream) = upstream else { return reply(&mut stream, UNREACHABLE).await };
    reply(&mut stream, 0).await?;
    tokio::io::copy_bidirectional(&mut stream, &mut upstream).await.map(|_| ())
}

/// 握手到 CONNECT：给回查过、能连的地址；拒了（已经回过拒绝）是 `None`。
async fn handshake(stream: &mut TcpStream, allow: Option<SocketAddr>) -> std::io::Result<Option<Vec<SocketAddr>>> {
    let mut head = [0u8; 2];
    stream.read_exact(&mut head).await?;
    let mut methods = vec![0u8; head[1] as usize];
    stream.read_exact(&mut methods).await?;
    if head[0] != 5 || !methods.contains(&0) {
        stream.write_all(&[5, 0xff]).await?;
        return Ok(None);
    }
    stream.write_all(&[5, 0]).await?;
    let mut req = [0u8; 4];
    stream.read_exact(&mut req).await?;
    let host = match req[3] {
        1 => {
            let mut ip = [0u8; 4];
            stream.read_exact(&mut ip).await?;
            Err(IpAddr::from(ip))
        }
        4 => {
            let mut ip = [0u8; 16];
            stream.read_exact(&mut ip).await?;
            Err(IpAddr::V6(Ipv6Addr::from(ip)))
        }
        3 => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len).await?;
            let mut name = vec![0u8; len[0] as usize];
            stream.read_exact(&mut name).await?;
            Ok(String::from_utf8_lossy(&name).trim_end_matches('.').to_ascii_lowercase())
        }
        _ => {
            reply(stream, ADDRESS_NOT_SUPPORTED).await?;
            return Ok(None);
        }
    };
    let mut port = [0u8; 2];
    stream.read_exact(&mut port).await?;
    let port = u16::from_be_bytes(port);
    if req[1] != 1 {
        reply(stream, COMMAND_NOT_SUPPORTED).await?;
        return Ok(None);
    }
    let addrs: Vec<SocketAddr> = match host {
        Err(ip) => vec![SocketAddr::new(ip, port)],
        Ok(name) if super::web::local_name(&name) => {
            reply(stream, NOT_ALLOWED).await?;
            return Ok(None);
        }
        Ok(name) => tokio::net::lookup_host((name.as_str(), port)).await.map(Iterator::collect).unwrap_or_default(),
    };
    let ok = |a: &SocketAddr| super::web::is_public(a.ip()) || allow == Some(*a);
    if addrs.is_empty() || !addrs.iter().all(ok) {
        reply(stream, if addrs.is_empty() { UNREACHABLE } else { NOT_ALLOWED }).await?;
        return Ok(None);
    }
    Ok(Some(addrs))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 经代理连一个地址：给回 SOCKS5 的回复码。
    async fn connect(proxy: u16, target: &[u8], port: u16) -> u8 {
        let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, proxy)).await.unwrap();
        s.write_all(&[5, 1, 0]).await.unwrap();
        let mut hello = [0u8; 2];
        s.read_exact(&mut hello).await.unwrap();
        assert_eq!(hello, [5, 0]);
        let mut req = vec![5, 1, 0];
        req.extend_from_slice(target);
        req.extend_from_slice(&port.to_be_bytes());
        s.write_all(&req).await.unwrap();
        let mut answer = [0u8; 10];
        s.read_exact(&mut answer).await.unwrap();
        answer[1]
    }

    #[tokio::test]
    async fn local_targets_are_refused_and_the_allowed_one_goes_through() {
        let server = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let fixture = server.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = server.accept().await {
                let _ = s.write_all(b"hi").await;
            }
        });
        let proxy = start(Some(fixture)).await.unwrap();
        let ip = |a: [u8; 4]| [&[1u8][..], &a[..]].concat();
        let name = |n: &str| [&[3u8, n.len() as u8][..], n.as_bytes()].concat();
        assert_eq!(connect(proxy.port, &ip([127, 0, 0, 1]), fixture.port()).await, 0, "夹具放行");
        assert_eq!(connect(proxy.port, &ip([127, 0, 0, 1]), 9).await, NOT_ALLOWED, "别的本机端口拒");
        assert_eq!(connect(proxy.port, &ip([10, 0, 0, 1]), 80).await, NOT_ALLOWED);
        assert_eq!(connect(proxy.port, &name("localhost"), fixture.port()).await, NOT_ALLOWED, "本机的名字拒");
        assert_eq!(connect(proxy.port, &name("printer.local"), 80).await, NOT_ALLOWED);
        let v6 = [&[4u8][..], &Ipv6Addr::LOCALHOST.octets()[..]].concat();
        assert_eq!(connect(proxy.port, &v6, 80).await, NOT_ALLOWED);
        // 关掉以后连不上。
        let port = proxy.port;
        drop(proxy);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await.is_err());
    }
}
