use std::net::{Ipv4Addr, Ipv6Addr};

use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetAdaptersAddresses, GetIfTable2, GAA_FLAG_INCLUDE_GATEWAYS, IP_ADAPTER_ADDRESSES_LH,
    MIB_IF_ROW2, MIB_IF_TABLE2,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6};

use crate::win::{from_pcwstr, from_wide};

const IF_TYPE_SOFTWARE_LOOPBACK: u32 = 24;

/// One selectable network connection (`NetWorkConection`).
#[derive(Clone, Debug, Default)]
pub struct Connection {
    pub description: String,
    pub luid: u64,
    /// Counters when the program started, for the "since start" rows of the details window.
    pub in_bytes_at_start: u64,
    pub out_bytes_at_start: u64,
}

#[derive(Clone, Debug, Default)]
pub struct IfRow {
    pub luid: u64,
    pub alias: String,
    pub description: String,
    pub if_type: u32,
    pub oper_up: bool,
    pub oper_status: i32,
    pub speed: u64,
    pub mac: Vec<u8>,
    pub in_octets: u64,
    pub out_octets: u64,
}

pub fn if_table() -> Vec<IfRow> {
    let mut out = Vec::new();
    unsafe {
        let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut table).is_err() || table.is_null() {
            return out;
        }
        let n = (*table).NumEntries as usize;
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), n);
        for r in rows {
            out.push(row(r));
        }
        FreeMibTable(table as *const _);
    }
    out
}

fn row(r: &MIB_IF_ROW2) -> IfRow {
    let mac_len = (r.PhysicalAddressLength as usize).min(r.PhysicalAddress.len());
    IfRow {
        luid: unsafe { r.InterfaceLuid.Value },
        alias: from_wide(&r.Alias),
        description: from_wide(&r.Description),
        if_type: r.Type,
        oper_up: r.OperStatus.0 == 1,
        oper_status: r.OperStatus.0,
        speed: r.TransmitLinkSpeed,
        mac: r.PhysicalAddress[..mac_len].to_vec(),
        in_octets: r.InOctets,
        out_octets: r.OutOctets,
    }
}

#[derive(Clone, Debug, Default)]
pub struct AdapterAddresses {
    pub luid: u64,
    pub description: String,
    pub friendly_name: String,
    pub if_type: u32,
    pub ipv4: Vec<(String, String)>,
    pub ipv6: Vec<String>,
    pub gateways: Vec<String>,
    pub dns: Vec<String>,
}

pub fn adapters() -> Vec<AdapterAddresses> {
    let mut out = Vec::new();
    unsafe {
        let mut size: u32 = 16 * 1024;
        let mut buf: Vec<u8>;
        loop {
            buf = vec![0u8; size as usize];
            let r = GetAdaptersAddresses(
                AF_UNSPEC.0 as u32,
                GAA_FLAG_INCLUDE_GATEWAYS,
                None,
                Some(buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
                &mut size,
            );
            if r == ERROR_BUFFER_OVERFLOW.0 {
                continue;
            }
            if r != NO_ERROR.0 {
                return out;
            }
            break;
        }
        let mut p = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        while !p.is_null() {
            let a = &*p;
            let mut info = AdapterAddresses {
                luid: a.Luid.Value,
                description: from_pcwstr(a.Description.0),
                friendly_name: from_pcwstr(a.FriendlyName.0),
                if_type: a.IfType,
                ..Default::default()
            };
            let mut u = a.FirstUnicastAddress;
            while !u.is_null() {
                let ua = &*u;
                if let Some(addr) = sockaddr_to_string(ua.Address.lpSockaddr) {
                    if (*ua.Address.lpSockaddr).sa_family == AF_INET {
                        info.ipv4.push((addr, prefix_to_mask(ua.OnLinkPrefixLength)));
                    } else {
                        info.ipv6.push(addr);
                    }
                }
                u = ua.Next;
            }
            let mut g = a.FirstGatewayAddress;
            while !g.is_null() {
                if let Some(addr) = sockaddr_to_string((*g).Address.lpSockaddr) {
                    info.gateways.push(addr);
                }
                g = (*g).Next;
            }
            let mut d = a.FirstDnsServerAddress;
            while !d.is_null() {
                if let Some(addr) = sockaddr_to_string((*d).Address.lpSockaddr) {
                    info.dns.push(addr);
                }
                d = (*d).Next;
            }
            out.push(info);
            p = a.Next;
        }
    }
    out
}

unsafe fn sockaddr_to_string(sa: *const SOCKADDR) -> Option<String> {
    if sa.is_null() {
        return None;
    }
    unsafe {
        match (*sa).sa_family {
            AF_INET => {
                let sin = &*(sa as *const SOCKADDR_IN);
                let b = sin.sin_addr.S_un.S_un_b;
                Some(Ipv4Addr::new(b.s_b1, b.s_b2, b.s_b3, b.s_b4).to_string())
            }
            AF_INET6 => {
                let sin6 = &*(sa as *const SOCKADDR_IN6);
                Some(Ipv6Addr::from(sin6.sin6_addr.u.Byte).to_string())
            }
            _ => None,
        }
    }
}

fn prefix_to_mask(prefix: u8) -> String {
    let p = prefix.min(32) as u32;
    let mask: u32 = if p == 0 { 0 } else { u32::MAX << (32 - p) };
    Ipv4Addr::from(mask).to_string()
}

/// Builds the selectable connection list (`CTrafficMonitorDlg::IniConnection`).
pub fn connections(show_all: bool, hidden: &[String]) -> Vec<Connection> {
    let rows = if_table();
    let mut out = Vec::new();
    if show_all {
        for r in &rows {
            out.push(Connection {
                description: r.description.clone(),
                luid: r.luid,
                in_bytes_at_start: r.in_octets,
                out_bytes_at_start: r.out_octets,
            });
        }
    } else {
        for a in adapters() {
            if a.if_type == IF_TYPE_SOFTWARE_LOOPBACK {
                continue;
            }
            if hidden.iter().any(|h| *h == a.description) {
                continue;
            }
            let Some(r) = rows.iter().find(|r| r.luid == a.luid) else {
                continue;
            };
            out.push(Connection {
                description: a.description.clone(),
                luid: a.luid,
                in_bytes_at_start: r.in_octets,
                out_bytes_at_start: r.out_octets,
            });
        }
    }
    out
}

pub fn all_adapter_descriptions() -> Vec<String> {
    adapters()
        .into_iter()
        .filter(|a| a.if_type != IF_TYPE_SOFTWARE_LOOPBACK)
        .map(|a| a.description)
        .collect()
}

pub fn interface_count() -> u32 {
    let mut n = 0u32;
    unsafe {
        let _ = windows::Win32::NetworkManagement::IpHelper::GetNumberOfInterfaces(&mut n);
    }
    n
}
