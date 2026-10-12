//! The few nl80211 queries we need: which network an interface is on, and how strong it is.
//! nl80211 is how every Wi-Fi daemon (iwd, wpa_supplicant) talks to the kernel, so this works
//! whichever one is running.

use std::io;

use netlink_packet_core::{
    DecodeError, DefaultNla, Emitable, NlaBuffer, NlasIterator, ParseableParametrized,
};
use netlink_packet_generic::ctrl::nlas::GenlCtrlAttrs;
use netlink_packet_generic::ctrl::{GenlCtrl, GenlCtrlCmd};
use netlink_packet_generic::{GenlFamily, GenlHeader, GenlMessage};
use rustix::net::netlink;

use crate::netlink::Requests;

const GENL_ID_CTRL: u16 = 0x10;

const CMD_GET_INTERFACE: u8 = 5;
const CMD_GET_STATION: u8 = 17;

const ATTR_IFINDEX: u16 = 3;
const ATTR_STA_INFO: u16 = 21;
const ATTR_WIPHY_FREQ: u16 = 38;
const ATTR_SSID: u16 = 52;
const STA_INFO_SIGNAL: u16 = 7;

/// The network a Wi-Fi interface is on.
#[derive(Debug, Default)]
pub struct Association {
    pub ssid: Option<String>,
    /// MHz.
    pub frequency: u32,
    /// dBm.
    pub signal: Option<i8>,
}

pub struct Nl80211 {
    requests: Requests,
    family: u16,
}

impl Nl80211 {
    /// `None` when the kernel has no nl80211 (no Wi-Fi drivers at all).
    pub fn new() -> io::Result<Option<Self>> {
        let mut requests = Requests::new(Some(netlink::GENERIC))?;
        let query = GenlMessage::from_payload(GenlCtrl {
            cmd: GenlCtrlCmd::GetFamily,
            nlas: vec![GenlCtrlAttrs::FamilyName("nl80211".into())],
        });
        let family = match requests.get(GENL_ID_CTRL, query) {
            Ok(reply) => reply.and_then(|reply| {
                reply.payload.nlas.iter().find_map(|attr| match attr {
                    GenlCtrlAttrs::FamilyId(id) => Some(*id),
                    _ => None,
                })
            }),
            Err(error) if error.raw_os_error() == Some(rustix::io::Errno::NOENT.raw_os_error()) => {
                None
            }
            Err(error) => return Err(error),
        };
        Ok(family.map(|family| Self { requests, family }))
    }

    /// What the interface is associated with, or `None` if it isn't a Wi-Fi interface.
    pub fn association(&mut self, index: u32) -> io::Result<Option<Association>> {
        let ifindex = || vec![(ATTR_IFINDEX, index.to_ne_bytes().to_vec())];
        let interface = match self.request(CMD_GET_INTERFACE, ifindex(), false) {
            Ok(mut replies) if !replies.is_empty() => replies.swap_remove(0),
            Ok(_) => return Ok(None),
            // Not a wireless interface.
            Err(error) if is_not_wireless(&error) => return Ok(None),
            Err(error) => return Err(error),
        };
        let mut association = Association::default();
        for (kind, value) in &interface.attributes {
            match *kind {
                ATTR_SSID => association.ssid = Some(String::from_utf8_lossy(value).into_owned()),
                ATTR_WIPHY_FREQ => association.frequency = u32_from(value),
                _ => {}
            }
        }
        if association.ssid.is_some() {
            // In station mode the only station is the access point.
            let stations = self.request(CMD_GET_STATION, ifindex(), true)?;
            association.signal = stations.iter().find_map(signal);
        }
        Ok(Some(association))
    }

    fn request(
        &mut self,
        command: u8,
        attributes: Vec<(u16, Vec<u8>)>,
        dump: bool,
    ) -> io::Result<Vec<Message>> {
        let mut message = GenlMessage::from_payload(Message {
            command,
            attributes,
        });
        message.set_resolved_family_id(self.family);
        let replies = if dump {
            self.requests.dump(self.family, message)?
        } else {
            self.requests
                .get(self.family, message)?
                .into_iter()
                .collect()
        };
        Ok(replies.into_iter().map(|reply| reply.payload).collect())
    }
}

fn is_not_wireless(error: &io::Error) -> bool {
    use rustix::io::Errno;
    [Errno::NODEV, Errno::OPNOTSUPP, Errno::INVAL, Errno::NOENT]
        .iter()
        .any(|errno| error.raw_os_error() == Some(errno.raw_os_error()))
}

/// `NL80211_STA_INFO_SIGNAL` from a station's info.
fn signal(station: &Message) -> Option<i8> {
    let (_, info) = station
        .attributes
        .iter()
        .find(|(kind, _)| *kind == ATTR_STA_INFO)?;
    attributes(info)
        .into_iter()
        .find(|(kind, _)| *kind == STA_INFO_SIGNAL)
        .and_then(|(_, value)| value.first().map(|&byte| byte as i8))
}

fn u32_from(value: &[u8]) -> u32 {
    value
        .get(..4)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u32::from_ne_bytes)
        .unwrap_or(0)
}

/// `(type, value)` pairs, with the nested and byte-order flags cleared from the type.
fn attributes(buffer: &[u8]) -> Vec<(u16, Vec<u8>)> {
    NlasIterator::new(buffer)
        .filter_map(Result::ok)
        .map(|nla: NlaBuffer<&[u8]>| (nla.kind(), nla.value().to_vec()))
        .collect()
}

/// An nl80211 message, with its attributes left raw.
#[derive(Debug, Clone)]
pub struct Message {
    command: u8,
    attributes: Vec<(u16, Vec<u8>)>,
}

impl Message {
    fn nlas(&self) -> Vec<DefaultNla> {
        self.attributes
            .iter()
            .map(|(kind, value)| DefaultNla::new(*kind, value.clone()))
            .collect()
    }
}

impl GenlFamily for Message {
    fn family_name() -> &'static str {
        "nl80211"
    }

    fn command(&self) -> u8 {
        self.command
    }

    fn version(&self) -> u8 {
        1
    }
}

impl Emitable for Message {
    fn buffer_len(&self) -> usize {
        self.nlas().as_slice().buffer_len()
    }

    fn emit(&self, buffer: &mut [u8]) {
        self.nlas().as_slice().emit(buffer);
    }
}

impl ParseableParametrized<[u8], GenlHeader> for Message {
    fn parse_with_param(buffer: &[u8], header: GenlHeader) -> Result<Self, DecodeError> {
        Ok(Self {
            command: header.cmd,
            attributes: attributes(buffer),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nla(kind: u16, value: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&((4 + value.len()) as u16).to_ne_bytes());
        bytes.extend_from_slice(&kind.to_ne_bytes());
        bytes.extend_from_slice(value);
        while bytes.len() % 4 != 0 {
            bytes.push(0);
        }
        bytes
    }

    #[test]
    fn reads_signal_from_station_info() {
        let info = [nla(1, &[0; 4]), nla(STA_INFO_SIGNAL, &[(-53i8) as u8])].concat();
        let station = Message {
            command: CMD_GET_STATION,
            attributes: attributes(
                &[
                    nla(ATTR_IFINDEX, &3u32.to_ne_bytes()),
                    nla(ATTR_STA_INFO, &info),
                ]
                .concat(),
            ),
        };
        assert_eq!(signal(&station), Some(-53));
    }
}
