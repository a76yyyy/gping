/// Utility functions for pinger
use crate::target::{IPVersion, Target};
use crate::PingCreationError;
use std::net::{IpAddr, ToSocketAddrs};
/// Resolve IP address with IPv4/IPv6 filtering support
fn resolve_ip(target: &str, version: IPVersion) -> Result<IpAddr, PingCreationError> {
    // Try to parse directly as IP address
    if let Ok(ip) = target.parse::<IpAddr>() {
        // Verify IP version matches
        match version {
            IPVersion::V4 if ip.is_ipv4() => return Ok(ip),
            IPVersion::V6 if ip.is_ipv6() => return Ok(ip),
            IPVersion::Any => return Ok(ip),
            _ => return Err(PingCreationError::HostnameError(target.to_string())),
        }
    }

    // Resolve as hostname
    let socket_addrs = (target, 0)
        .to_socket_addrs()
        .map_err(|_| PingCreationError::HostnameError(target.to_string()))?;

    // Filter addresses by IP version
    let selected_ips: Vec<_> = socket_addrs
        .filter(|addr| match version {
            IPVersion::V4 => matches!(addr.ip(), IpAddr::V4(_)),
            IPVersion::V6 => matches!(addr.ip(), IpAddr::V6(_)),
            IPVersion::Any => true,
        })
        .collect();

    if selected_ips.is_empty() {
        return Err(PingCreationError::HostnameError(target.to_string()));
    }

    Ok(selected_ips[0].ip())
}

/// Resolve target and return valid IP address
///
/// # Arguments
/// * `target` - Target address (IP or hostname)
///
/// # Returns
/// * `Ok(IpAddr)` - Successfully resolved IP address
/// * `Err(PingCreationError)` - Resolution failed
///
/// # Examples
/// ```
/// use pinger::target::Target;
/// use pinger::utils::resolve_target;
///
/// let target = Target::new_any("google.com");
/// let ip = resolve_target(&target).unwrap();
/// ```
pub fn resolve_target(target: &Target) -> Result<IpAddr, PingCreationError> {
    match target {
        Target::IP(ip) => Ok(*ip),
        Target::Hostname { domain, version } => resolve_ip(domain, *version),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_ipv4_address() {
        let target = Target::new_any("8.8.8.8");
        let result = resolve_target(&target);
        assert!(result.is_ok());
        assert!(result.unwrap().is_ipv4());
    }

    #[test]
    fn test_resolve_ipv6_address() {
        let target = Target::new_any("::1");
        let result = resolve_target(&target);
        assert!(result.is_ok());
        assert!(result.unwrap().is_ipv6());
    }

    #[test]
    fn test_resolve_hostname() {
        let target = Target::new_any("localhost");
        let result = resolve_target(&target);
        assert!(result.is_ok());
    }

    #[test]
    fn test_resolve_invalid_hostname() {
        let target = Target::new_any("this-hostname-does-not-exist-12345.invalid");
        let result = resolve_target(&target);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_ipv4_only() {
        let target = Target::new_ipv4("8.8.8.8");
        let result = resolve_target(&target);
        assert!(result.is_ok());
        assert!(result.unwrap().is_ipv4());
    }

    #[test]
    fn test_resolve_ipv6_only() {
        let target = Target::new_ipv6("::1");
        let result = resolve_target(&target);
        assert!(result.is_ok());
        assert!(result.unwrap().is_ipv6());
    }

    #[test]
    fn test_resolve_ipv4_with_ipv6_constraint() {
        let target = Target::new_ipv6("8.8.8.8");
        let result = resolve_target(&target);
        // Should fail because 8.8.8.8 is IPv4 but IPv6 is required
        assert!(result.is_err());
    }
}
