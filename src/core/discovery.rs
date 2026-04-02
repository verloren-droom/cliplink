use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket as StdUdpSocket},
};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::net::UdpSocket;
use uuid::Uuid;

use crate::{
    constants::discovery::{
        DISCOVERY_MULTICAST_GROUP, DISCOVERY_PACKET_MAGIC, DISCOVERY_PORT,
        DISCOVERY_PROTOCOL_VERSION, MAX_DISCOVERY_PACKET_BYTES,
    },
    core::{
        config::AppConfig,
        error::{AppError, AppResult},
        model::DiscoveredPeer,
        security::{
            DeviceIdentity, MessageSignatureScheme, certificate_fingerprint, sign_detached_message,
            verify_detached_message_signature,
        },
    },
};

/// Local peer identity announced on the LAN discovery channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryConfig {
    pub device_id: String,
    pub device_name: String,
    pub listen_port: u16,
    pub fingerprint: String,
    pub certificate_der: Vec<u8>,
    pub private_key_der: Vec<u8>,
}

/// Open UDP discovery socket used for multicast and broadcast peer presence exchange.
#[derive(Debug)]
pub struct DiscoverySocket {
    socket: UdpSocket,
    multicast_enabled: bool,
}

/// Parsed discovery datagram emitted by the UDP listener.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceivedDiscoveryMessage {
    Probe {
        source: SocketAddr,
        device_id: String,
        fingerprint: String,
    },
    Announce(DiscoveredPeer),
    TrustRequest {
        source: SocketAddr,
        request_id: Uuid,
        peer: DiscoveredPeer,
    },
    TrustDecision {
        request_id: Uuid,
        peer: DiscoveredPeer,
        accepted: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct UnsignedDiscoveryEnvelope {
    protocol_version: u8,
    message: DiscoveryMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct DiscoveryEnvelope {
    protocol_version: u8,
    message: DiscoveryMessage,
    certificate_der_hex: String,
    signature_scheme: String,
    signature_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DiscoveryMessage {
    Probe {
        device_id: String,
        fingerprint: String,
    },
    Announce {
        device_id: String,
        device_name: String,
        listen_port: u16,
        fingerprint: String,
    },
    TrustRequest {
        request_id: Uuid,
        device_id: String,
        device_name: String,
        listen_port: u16,
        fingerprint: String,
    },
    TrustDecision {
        request_id: Uuid,
        accepted: bool,
        device_id: String,
        device_name: String,
        listen_port: u16,
        fingerprint: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiscoveryTarget {
    Lan,
    Unicast(SocketAddr),
}

pub fn build_discovery_config(config: &AppConfig, identity: &DeviceIdentity) -> DiscoveryConfig {
    DiscoveryConfig {
        device_id: config.device_id.clone(),
        device_name: config.device_name.clone(),
        listen_port: config.listen_port,
        fingerprint: identity.fingerprint.clone(),
        certificate_der: identity.certificate_der.clone(),
        private_key_der: identity.private_key_der.clone(),
    }
}

pub async fn open_discovery_socket() -> AppResult<DiscoverySocket> {
    let interface = detect_primary_ipv4()?;
    let std_socket = StdUdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT))?;
    std_socket.set_nonblocking(true)?;
    std_socket.set_broadcast(true)?;
    std_socket.set_multicast_loop_v4(false)?;
    std_socket.set_multicast_ttl_v4(1)?;

    let multicast_group = multicast_group();
    let multicast_enabled = std_socket
        .join_multicast_v4(&multicast_group, &interface)
        .is_ok();

    let socket = UdpSocket::from_std(std_socket)?;
    Ok(DiscoverySocket {
        socket,
        multicast_enabled,
    })
}

impl DiscoverySocket {
    pub async fn send_probe(&self, config: &DiscoveryConfig) -> AppResult<()> {
        self.send_message(
            DiscoveryMessage::Probe {
                device_id: config.device_id.clone(),
                fingerprint: config.fingerprint.clone(),
            },
            config,
            DiscoveryTarget::Lan,
        )
        .await
    }

    pub async fn send_announce(&self, config: &DiscoveryConfig) -> AppResult<()> {
        self.send_message(
            DiscoveryMessage::Announce {
                device_id: config.device_id.clone(),
                device_name: config.device_name.clone(),
                listen_port: config.listen_port,
                fingerprint: config.fingerprint.clone(),
            },
            config,
            DiscoveryTarget::Lan,
        )
        .await
    }

    pub async fn reply_with_announce(
        &self,
        config: &DiscoveryConfig,
        target: SocketAddr,
    ) -> AppResult<()> {
        self.send_message(
            DiscoveryMessage::Announce {
                device_id: config.device_id.clone(),
                device_name: config.device_name.clone(),
                listen_port: config.listen_port,
                fingerprint: config.fingerprint.clone(),
            },
            config,
            DiscoveryTarget::Unicast(target),
        )
        .await
    }

    pub async fn send_trust_request(
        &self,
        config: &DiscoveryConfig,
        request_id: Uuid,
        target: SocketAddr,
    ) -> AppResult<()> {
        self.send_message(
            DiscoveryMessage::TrustRequest {
                request_id,
                device_id: config.device_id.clone(),
                device_name: config.device_name.clone(),
                listen_port: config.listen_port,
                fingerprint: config.fingerprint.clone(),
            },
            config,
            DiscoveryTarget::Unicast(target),
        )
        .await
    }

    pub async fn send_trust_decision(
        &self,
        config: &DiscoveryConfig,
        request_id: Uuid,
        accepted: bool,
        target: SocketAddr,
    ) -> AppResult<()> {
        self.send_message(
            DiscoveryMessage::TrustDecision {
                request_id,
                accepted,
                device_id: config.device_id.clone(),
                device_name: config.device_name.clone(),
                listen_port: config.listen_port,
                fingerprint: config.fingerprint.clone(),
            },
            config,
            DiscoveryTarget::Unicast(target),
        )
        .await
    }

    pub async fn recv_message(
        &self,
        buffer: &mut [u8],
        self_device_id: &str,
        self_fingerprint: &str,
    ) -> AppResult<Option<ReceivedDiscoveryMessage>> {
        let (bytes_read, source) = self
            .socket
            .recv_from(buffer)
            .await
            .map_err(|error| AppError::Network(error.to_string()))?;

        let Some(message) = decode_message(&buffer[..bytes_read]) else {
            return Ok(None);
        };

        let parsed = match message {
            DiscoveryMessage::Probe {
                device_id,
                fingerprint,
            } => {
                if device_id == self_device_id || fingerprint == self_fingerprint {
                    return Ok(None);
                }

                Some(ReceivedDiscoveryMessage::Probe {
                    source,
                    device_id,
                    fingerprint,
                })
            }
            DiscoveryMessage::Announce {
                device_id,
                device_name,
                listen_port,
                fingerprint,
            } => {
                if device_id == self_device_id || fingerprint == self_fingerprint {
                    return Ok(None);
                }

                Some(ReceivedDiscoveryMessage::Announce(DiscoveredPeer {
                    device_id,
                    device_name,
                    host: source.ip().to_string(),
                    address: source.ip().to_string(),
                    port: listen_port,
                    fingerprint,
                    last_seen: OffsetDateTime::now_utc(),
                }))
            }
            DiscoveryMessage::TrustRequest {
                request_id,
                device_id,
                device_name,
                listen_port,
                fingerprint,
            } => {
                if device_id == self_device_id || fingerprint == self_fingerprint {
                    return Ok(None);
                }

                Some(ReceivedDiscoveryMessage::TrustRequest {
                    source,
                    request_id,
                    peer: DiscoveredPeer {
                        device_id,
                        device_name,
                        host: source.ip().to_string(),
                        address: source.ip().to_string(),
                        port: listen_port,
                        fingerprint,
                        last_seen: OffsetDateTime::now_utc(),
                    },
                })
            }
            DiscoveryMessage::TrustDecision {
                request_id,
                accepted,
                device_id,
                device_name,
                listen_port,
                fingerprint,
            } => {
                if device_id == self_device_id || fingerprint == self_fingerprint {
                    return Ok(None);
                }

                Some(ReceivedDiscoveryMessage::TrustDecision {
                    request_id,
                    accepted,
                    peer: DiscoveredPeer {
                        device_id,
                        device_name,
                        host: source.ip().to_string(),
                        address: source.ip().to_string(),
                        port: listen_port,
                        fingerprint,
                        last_seen: OffsetDateTime::now_utc(),
                    },
                })
            }
        };

        Ok(parsed)
    }

    async fn send_message(
        &self,
        message: DiscoveryMessage,
        config: &DiscoveryConfig,
        target: DiscoveryTarget,
    ) -> AppResult<()> {
        let payload = encode_message(config, &message)?;
        match target {
            DiscoveryTarget::Lan => {
                if self.multicast_enabled {
                    self.socket
                        .send_to(
                            &payload,
                            SocketAddr::V4(SocketAddrV4::new(multicast_group(), DISCOVERY_PORT)),
                        )
                        .await
                        .map_err(|error| AppError::Network(error.to_string()))?;
                }

                self.socket
                    .send_to(
                        &payload,
                        SocketAddr::V4(SocketAddrV4::new(
                            Ipv4Addr::new(255, 255, 255, 255),
                            DISCOVERY_PORT,
                        )),
                    )
                    .await
                    .map_err(|error| AppError::Network(error.to_string()))?;
            }
            DiscoveryTarget::Unicast(target) => {
                self.socket
                    .send_to(&payload, target)
                    .await
                    .map_err(|error| AppError::Network(error.to_string()))?;
            }
        }

        Ok(())
    }
}

pub fn merge_announced_peers(
    discovered: &mut HashMap<String, DiscoveredPeer>,
    peers: impl IntoIterator<Item = DiscoveredPeer>,
    now: OffsetDateTime,
) {
    for mut peer in peers {
        peer.last_seen = now;
        if !peer.fingerprint.is_empty() {
            let stale_keys = discovered
                .iter()
                .filter(|(device_id, existing)| {
                    existing.fingerprint == peer.fingerprint && *device_id != &peer.device_id
                })
                .map(|(device_id, _)| device_id.clone())
                .collect::<Vec<_>>();
            for device_id in stale_keys {
                discovered.remove(device_id.as_str());
            }
        }
        discovered.insert(peer.device_id.clone(), peer);
    }
}

pub fn detect_primary_ipv4() -> AppResult<Ipv4Addr> {
    let socket = StdUdpSocket::bind("0.0.0.0:0")?;
    let _ = socket.connect("192.0.2.1:9");
    if let Ok(local) = socket.local_addr() {
        if let IpAddr::V4(ip) = local.ip() {
            if !ip.is_loopback() {
                return Ok(ip);
            }
        }
    }

    Err(AppError::Network(
        "No active non-loopback IPv4 interface is available for LAN discovery.".to_string(),
    ))
}

fn multicast_group() -> Ipv4Addr {
    let [a, b, c, d] = DISCOVERY_MULTICAST_GROUP;
    Ipv4Addr::new(a, b, c, d)
}

fn encode_message(config: &DiscoveryConfig, message: &DiscoveryMessage) -> AppResult<Vec<u8>> {
    let unsigned = UnsignedDiscoveryEnvelope {
        protocol_version: DISCOVERY_PROTOCOL_VERSION,
        message: message.clone(),
    };
    let unsigned_json = serde_json::to_vec(&unsigned)?;
    let signature = sign_detached_message(
        &DeviceIdentity {
            certificate_der: config.certificate_der.clone(),
            private_key_der: config.private_key_der.clone(),
            fingerprint: config.fingerprint.clone(),
        },
        &unsigned_json,
    )?;
    let envelope = DiscoveryEnvelope {
        protocol_version: unsigned.protocol_version,
        message: unsigned.message,
        certificate_der_hex: encode_hex(&signature.certificate_der),
        signature_scheme: signature.scheme.as_str().to_string(),
        signature_hex: encode_hex(&signature.signature),
    };
    let json = serde_json::to_vec(&envelope)?;
    let frame_len = DISCOVERY_PACKET_MAGIC.len() + json.len();
    if frame_len > MAX_DISCOVERY_PACKET_BYTES {
        return Err(AppError::Network(format!(
            "Discovery datagram size {frame_len} exceeds the supported limit."
        )));
    }

    let mut payload = Vec::with_capacity(frame_len);
    payload.extend_from_slice(DISCOVERY_PACKET_MAGIC);
    payload.extend_from_slice(&json);
    Ok(payload)
}

fn decode_message(bytes: &[u8]) -> Option<DiscoveryMessage> {
    if bytes.len() <= DISCOVERY_PACKET_MAGIC.len()
        || !bytes.starts_with(DISCOVERY_PACKET_MAGIC)
        || bytes.len() > MAX_DISCOVERY_PACKET_BYTES
    {
        return None;
    }

    let envelope =
        serde_json::from_slice::<DiscoveryEnvelope>(&bytes[DISCOVERY_PACKET_MAGIC.len()..]).ok()?;
    if envelope.protocol_version != DISCOVERY_PROTOCOL_VERSION {
        return None;
    }

    let certificate_der = decode_hex(&envelope.certificate_der_hex)?;
    let signature = decode_hex(&envelope.signature_hex)?;
    let signature_scheme = MessageSignatureScheme::parse(&envelope.signature_scheme)?;
    let unsigned = UnsignedDiscoveryEnvelope {
        protocol_version: envelope.protocol_version,
        message: envelope.message.clone(),
    };
    let unsigned_json = serde_json::to_vec(&unsigned).ok()?;
    verify_detached_message_signature(
        &certificate_der,
        signature_scheme,
        &unsigned_json,
        &signature,
    )
    .ok()?;
    if message_fingerprint(&envelope.message)? != certificate_fingerprint(&certificate_der) {
        return None;
    }

    Some(envelope.message)
}

fn message_fingerprint(message: &DiscoveryMessage) -> Option<&str> {
    match message {
        DiscoveryMessage::Probe { fingerprint, .. }
        | DiscoveryMessage::Announce { fingerprint, .. }
        | DiscoveryMessage::TrustRequest { fingerprint, .. }
        | DiscoveryMessage::TrustDecision { fingerprint, .. } => Some(fingerprint.as_str()),
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }

    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = decode_hex_nibble(pair[0])?;
            let low = decode_hex_nibble(pair[1])?;
            Some((high << 4) | low)
        })
        .collect()
}

fn decode_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DiscoveryConfig, DiscoveryMessage, build_discovery_config, decode_message, encode_message,
        merge_announced_peers,
    };
    use crate::core::{
        at_rest::LocalDataCipher, config::AppConfig, paths::AppPaths,
        security::load_or_create_identity,
    };

    fn test_config() -> DiscoveryConfig {
        let temp_root =
            std::env::temp_dir().join(format!("cliplink-discovery-test-{}", uuid::Uuid::new_v4()));
        let paths = AppPaths::from_root(temp_root);
        paths.ensure().unwrap();
        let cipher = LocalDataCipher::from_bytes([7_u8; 32]);
        let identity = load_or_create_identity(&paths, "device-a", &cipher).unwrap();
        let mut app_config = AppConfig::new(Some("Device A"));
        app_config.device_id = "device-a".to_string();
        app_config.device_name = "Device A".to_string();
        build_discovery_config(&app_config, &identity)
    }

    #[test]
    fn discovery_message_roundtrip_works() {
        let config = test_config();
        let encoded = encode_message(
            &config,
            &DiscoveryMessage::Probe {
                device_id: "device-a".to_string(),
                fingerprint: config.fingerprint.clone(),
            },
        )
        .unwrap();
        let decoded = decode_message(&encoded).unwrap();
        assert_eq!(
            decoded,
            DiscoveryMessage::Probe {
                device_id: "device-a".to_string(),
                fingerprint: config.fingerprint,
            }
        );
    }

    #[test]
    fn discovery_decoder_ignores_foreign_payloads() {
        assert!(decode_message(b"not-cliplink").is_none());
        assert!(decode_message(b"CLKD{}").is_none());
    }

    #[test]
    fn discovery_decoder_rejects_fingerprint_mismatch() {
        let config = test_config();
        let encoded = encode_message(
            &config,
            &DiscoveryMessage::Probe {
                device_id: "device-a".to_string(),
                fingerprint: "forged-fingerprint".to_string(),
            },
        )
        .unwrap();
        assert!(decode_message(&encoded).is_none());
    }

    #[test]
    fn merge_announced_peers_replaces_stale_device_id_for_same_fingerprint() {
        let now = time::OffsetDateTime::UNIX_EPOCH;
        let mut discovered = std::collections::HashMap::new();

        merge_announced_peers(
            &mut discovered,
            [crate::core::model::DiscoveredPeer {
                device_id: "old-id".to_string(),
                device_name: "Android".to_string(),
                host: "android.local".to_string(),
                address: "192.168.1.20".to_string(),
                port: 27_841,
                fingerprint: "fp-1".to_string(),
                last_seen: now,
            }],
            now,
        );
        merge_announced_peers(
            &mut discovered,
            [crate::core::model::DiscoveredPeer {
                device_id: "new-id".to_string(),
                device_name: "Android".to_string(),
                host: "android.local".to_string(),
                address: "192.168.1.21".to_string(),
                port: 27_841,
                fingerprint: "fp-1".to_string(),
                last_seen: now,
            }],
            now,
        );

        assert_eq!(discovered.len(), 1);
        assert!(discovered.contains_key("new-id"));
        assert!(!discovered.contains_key("old-id"));
    }
}
