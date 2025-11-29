//! Target address types and utilities

use std::fmt;
use std::fmt::{Display, Formatter};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// IP version specification for hostname resolution
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum IPVersion {
    /// IPv4 only
    V4,
    /// IPv6 only
    V6,
    /// Any IP version (IPv4 or IPv6)
    Any,
}

/// Target address for ping operations
///
/// Can be either a direct IP address or a hostname that needs to be resolved.
#[derive(Debug, Clone)]
pub enum Target {
    /// Direct IP address
    IP(IpAddr),
    /// Hostname with IP version constraint
    Hostname {
        /// Domain name to resolve
        domain: String,
        /// IP version constraint for resolution
        version: IPVersion,
    },
}

impl Target {
    /// Check if the target is IPv6
    ///
    /// Returns `true` if the target is an IPv6 address or a hostname constrained to IPv6.
    pub fn is_ipv6(&self) -> bool {
        match self {
            Target::IP(ip) => ip.is_ipv6(),
            Target::Hostname { version, .. } => *version == IPVersion::V6,
        }
    }

    /// Create a new target from a string, allowing any IP version
    ///
    /// If the string is a valid IP address, it will be used directly.
    /// Otherwise, it will be treated as a hostname that can resolve to either IPv4 or IPv6.
    pub fn new_any(value: impl ToString) -> Self {
        let value = value.to_string();
        if let Ok(ip) = value.parse::<IpAddr>() {
            return Self::IP(ip);
        }
        Self::Hostname {
            domain: value,
            version: IPVersion::Any,
        }
    }

    /// Create a new target from a string, constraining to IPv4
    ///
    /// If the string is a valid IPv4 address, it will be used directly.
    /// Otherwise, it will be treated as a hostname that must resolve to IPv4.
    pub fn new_ipv4(value: impl ToString) -> Self {
        let value = value.to_string();
        if let Ok(ip) = value.parse::<Ipv4Addr>() {
            return Self::IP(IpAddr::V4(ip));
        }
        Self::Hostname {
            domain: value.to_string(),
            version: IPVersion::V4,
        }
    }

    /// Create a new target from a string, constraining to IPv6
    ///
    /// If the string is a valid IPv6 address, it will be used directly.
    /// Otherwise, it will be treated as a hostname that must resolve to IPv6.
    pub fn new_ipv6(value: impl ToString) -> Self {
        let value = value.to_string();
        if let Ok(ip) = value.parse::<Ipv6Addr>() {
            return Self::IP(IpAddr::V6(ip));
        }
        Self::Hostname {
            domain: value.to_string(),
            version: IPVersion::V6,
        }
    }
}

impl Display for Target {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Target::IP(v) => Display::fmt(&v, f),
            Target::Hostname { domain, .. } => Display::fmt(&domain, f),
        }
    }
}
