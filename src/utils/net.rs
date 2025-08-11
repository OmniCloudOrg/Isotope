use std::net::TcpListener;
use std::collections::HashSet;

/// Find a random unoccupied port on localhost
pub fn find_free_port() -> Option<u16> {
    find_free_port_with_exclusions(&HashSet::new())
}

/// Find a random unoccupied port on localhost, excluding specified ports
pub fn find_free_port_with_exclusions(excluded_ports: &HashSet<u16>) -> Option<u16> {
    use std::time::{SystemTime, UNIX_EPOCH};
    
    // Use current time as seed for pseudo-randomness to avoid always starting at 20000
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    
    // Generate a pseudo-random starting port within the ephemeral range
    let start_port = 20000 + ((seed % 40000) as u16);
    
    tracing::debug!("Searching for free port starting from {} (excluding {:?})", start_port, excluded_ports);
    
    // Try ports in two passes: from random start to end, then from beginning to random start
    let ranges = [
        (start_port..60000).collect::<Vec<_>>(),
        (20000..start_port).collect::<Vec<_>>(),
    ];
    
    for range in ranges.iter() {
        for &port in range {
            if excluded_ports.contains(&port) {
                continue;
            }
            
            if is_port_available(port) {
                tracing::debug!("Found available port: {}", port);
                return Some(port);
            }
        }
    }
    
    tracing::warn!("No free ports found in range 20000-59999");
    None
}

/// Check if a port is available by attempting to bind to it
fn is_port_available(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}
