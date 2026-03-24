use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

use agnostic_mdns::{
    QueryParam, ServerOptions, SmolStr,
    service::ServiceBuilder,
    tokio::{Server, channel, query},
};
use time::OffsetDateTime;

use crate::{
    constants::{app::MDNS_SERVICE_TYPE, timing::DISCOVERY_QUERY_TIMEOUT},
    core::{config::AppConfig, error::AppResult, model::DiscoveredPeer, security::DeviceIdentity},
};

#[derive(Debug, Clone)]
pub struct DiscoveredPeerAnnouncement {
    pub device_id: String,
    pub device_name: String,
    pub fingerprint: String,
}

pub async fn start_advertiser(config: &AppConfig, identity: &DeviceIdentity) -> AppResult<Server> {
    let ip = detect_primary_ipv4();
    let instance = sanitize_instance_name(&config.device_name);
    let service = ServiceBuilder::new(instance.as_str().into(), MDNS_SERVICE_TYPE.into())
        .with_port(config.listen_port)
        .with_ip(ip)
        .with_txt_record(SmolStr::new(format!("id={}", config.device_id)))
        .with_txt_record(SmolStr::new(format!("name={}", config.device_name)))
        .with_txt_record(SmolStr::new(format!("fp={}", identity.fingerprint)))
        .finalize()?;

    Ok(Server::new(service, ServerOptions::default()).await?)
}

pub async fn discover_once(self_device_id: &str) -> AppResult<Vec<DiscoveredPeer>> {
    let params = QueryParam::new(MDNS_SERVICE_TYPE.into())
        .with_timeout(DISCOVERY_QUERY_TIMEOUT)
        .with_disable_ipv6(true);
    let (tx, rx) = channel::unbounded();
    query(params, tx).await?;

    let mut peers = Vec::new();
    while let Ok(entry) = rx.try_recv() {
        let Some(announcement) = parse_peer_announcement(entry.txt()) else {
            continue;
        };
        if announcement.device_id == self_device_id {
            continue;
        }

        let Some(address) = entry.ipv4_addr().copied().map(IpAddr::V4) else {
            continue;
        };

        peers.push(DiscoveredPeer {
            device_id: announcement.device_id,
            device_name: announcement.device_name,
            host: entry.host().to_string(),
            address: SocketAddr::new(address, entry.port()).ip().to_string(),
            port: entry.port(),
            fingerprint: announcement.fingerprint,
            last_seen: OffsetDateTime::now_utc(),
        });
    }

    Ok(peers)
}

pub fn parse_peer_announcement(records: &[SmolStr]) -> Option<DiscoveredPeerAnnouncement> {
    let mut device_id = None;
    let mut device_name = None;
    let mut fingerprint = None;

    for record in records {
        if let Some(value) = record.strip_prefix("id=") {
            device_id = Some(value.to_string());
        } else if let Some(value) = record.strip_prefix("name=") {
            device_name = Some(value.to_string());
        } else if let Some(value) = record.strip_prefix("fp=") {
            fingerprint = Some(value.to_string());
        }
    }

    Some(DiscoveredPeerAnnouncement {
        device_id: device_id?,
        device_name: device_name?,
        fingerprint: fingerprint?,
    })
}

fn sanitize_instance_name(name: &str) -> String {
    let mut output = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == ' ' {
            output.push(ch);
        } else {
            output.push('-');
        }
    }
    output.trim().trim_matches('.').to_string()
}

fn detect_primary_ipv4() -> IpAddr {
    let probe = UdpSocket::bind("0.0.0.0:0");
    if let Ok(socket) = probe {
        let _ = socket.connect("8.8.8.8:80");
        if let Ok(local) = socket.local_addr() {
            if let IpAddr::V4(ip) = local.ip() {
                if !ip.is_loopback() {
                    return IpAddr::V4(ip);
                }
            }
        }
    }

    IpAddr::V4(Ipv4Addr::LOCALHOST)
}
