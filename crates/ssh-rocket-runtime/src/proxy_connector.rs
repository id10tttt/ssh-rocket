use anyhow::{bail, Context, Result};
use std::{
    io::{self, Read, Write},
    net::{IpAddr, Shutdown, SocketAddr, TcpStream},
    time::Duration,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// 通过本地 SOCKS5 服务连接目标，并在标准输入输出与代理连接之间转发数据。
pub fn connect_stdio(socks_port: u16, target_host: &str, target_port: u16) -> Result<()> {
    let socks_address = SocketAddr::from(([127, 0, 0, 1], socks_port));
    let mut stream = TcpStream::connect_timeout(&socks_address, CONNECT_TIMEOUT)
        .with_context(|| format!("无法连接到本地 SSH 代理端口 {socks_port}"))?;
    stream
        .set_read_timeout(Some(CONNECT_TIMEOUT))
        .context("无法设置 SOCKS 读取超时")?;
    stream
        .set_write_timeout(Some(CONNECT_TIMEOUT))
        .context("无法设置 SOCKS 写入超时")?;

    establish_socks5(&mut stream, target_host, target_port)?;
    stream.set_read_timeout(None)?;
    stream.set_write_timeout(None)?;

    let mut reader = stream.try_clone().context("无法复制 SOCKS 连接")?;
    let download = std::thread::spawn(move || -> io::Result<()> {
        let mut stdout = io::stdout().lock();
        let mut buffer = [0u8; 16 * 1024];
        loop {
            let length = reader.read(&mut buffer)?;
            if length == 0 {
                break;
            }
            stdout.write_all(&buffer[..length])?;
            stdout.flush()?;
        }
        Ok(())
    });

    io::copy(&mut io::stdin().lock(), &mut stream).context("写入 SOCKS 连接失败")?;
    stream.shutdown(Shutdown::Write)?;
    download
        .join()
        .map_err(|_| anyhow::anyhow!("SOCKS 下载线程异常退出"))?
        .context("读取 SOCKS 连接失败")?;
    Ok(())
}

fn establish_socks5(stream: &mut TcpStream, target_host: &str, target_port: u16) -> Result<()> {
    stream.write_all(&[0x05, 0x01, 0x00])?;
    let mut greeting = [0u8; 2];
    stream.read_exact(&mut greeting)?;
    if greeting != [0x05, 0x00] {
        bail!("SOCKS5 代理不支持免认证连接");
    }

    let mut request = vec![0x05, 0x01, 0x00];
    match target_host.parse::<IpAddr>() {
        Ok(IpAddr::V4(address)) => {
            request.push(0x01);
            request.extend_from_slice(&address.octets());
        }
        Ok(IpAddr::V6(address)) => {
            request.push(0x04);
            request.extend_from_slice(&address.octets());
        }
        Err(_) => {
            let host = target_host.as_bytes();
            if host.is_empty() || host.len() > u8::MAX as usize {
                bail!("SSH 节点地址长度无效");
            }
            request.extend_from_slice(&[0x03, host.len() as u8]);
            request.extend_from_slice(host);
        }
    }
    request.extend_from_slice(&target_port.to_be_bytes());
    stream.write_all(&request)?;

    let mut response = [0u8; 4];
    stream.read_exact(&mut response)?;
    if response[0] != 0x05 {
        bail!("代理返回了无效的 SOCKS5 响应");
    }
    if response[1] != 0x00 {
        bail!("SOCKS5 代理连接目标失败，状态码 {}", response[1]);
    }
    let address_len = match response[3] {
        0x01 => 4,
        0x04 => 16,
        0x03 => {
            let mut length = [0u8; 1];
            stream.read_exact(&mut length)?;
            length[0] as usize
        }
        value => bail!("SOCKS5 代理返回了未知地址类型 {value}"),
    };
    let mut remainder = vec![0u8; address_len + 2];
    stream.read_exact(&mut remainder)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::establish_socks5;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    #[test]
    fn socks5_connector_sends_domain_and_port() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock SOCKS server");
        let address = listener.local_addr().expect("read mock address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept connector");
            let mut greeting = [0u8; 3];
            stream.read_exact(&mut greeting).expect("read greeting");
            assert_eq!(greeting, [0x05, 0x01, 0x00]);
            stream.write_all(&[0x05, 0x00]).expect("write greeting response");

            let mut request = [0u8; 18];
            stream.read_exact(&mut request).expect("read connect request");
            assert_eq!(&request[..5], &[0x05, 0x01, 0x00, 0x03, 11]);
            assert_eq!(&request[5..16], b"example.com");
            assert_eq!(&request[16..], &443u16.to_be_bytes());
            stream
                .write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0, 0])
                .expect("write connect response");
        });

        let mut stream = std::net::TcpStream::connect(address).expect("connect mock SOCKS server");
        establish_socks5(&mut stream, "example.com", 443).expect("complete SOCKS handshake");
        server.join().expect("join mock SOCKS server");
    }
}
