use crate::packet::{self, Result};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredDevice {
    pub host: Ipv4Addr,
    pub device_id: u32,
}

/// Discovery deliberately never returns the header's possible token bytes.
pub fn discover(duration: Duration) -> Result<Vec<DiscoveredDevice>> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    socket.set_broadcast(true)?;
    socket.set_write_timeout(Some(Duration::from_secs(1)))?;
    let mut targets = vec![Ipv4Addr::BROADCAST];
    if let Ok(interfaces) = if_addrs::get_if_addrs() {
        for interface in interfaces {
            if interface.is_loopback() {
                continue;
            }
            if let if_addrs::IfAddr::V4(address) = interface.addr
                && let Some(broadcast) = address.broadcast
                && !targets.contains(&broadcast)
            {
                targets.push(broadcast);
            }
        }
    }
    let mut devices = BTreeMap::new();
    let start = Instant::now();
    let mut buffer = [0u8; 65535];
    for round in 0..3 {
        let mut sent = false;
        for host in &targets {
            sent |= socket
                .send_to(&packet::HELLO, (*host, packet::PORT))
                .is_ok();
        }
        if !sent {
            return Err(crate::Error::Validation(
                "无法发送局域网广播，请检查网络权限".into(),
            ));
        }
        let deadline = start + duration * (round + 1) / 3;
        while Instant::now() < deadline {
            socket.set_read_timeout(Some(
                deadline
                    .saturating_duration_since(Instant::now())
                    .max(Duration::from_millis(1)),
            ))?;
            match socket.recv_from(&mut buffer) {
                Ok((length, SocketAddr::V4(from))) if length == packet::HEADER_LEN => {
                    if let Ok(header) = packet::header(&buffer[..length])
                        && header.device_id != 0
                        && header.device_id != u32::MAX
                    {
                        devices.insert(
                            header.device_id,
                            DiscoveredDevice {
                                host: *from.ip(),
                                device_id: header.device_id,
                            },
                        );
                    }
                }
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    break;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
    let mut devices: Vec<_> = devices.into_values().collect();
    devices.sort_by_key(|d| d.host.octets());
    Ok(devices)
}
