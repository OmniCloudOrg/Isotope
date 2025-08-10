use std::net::TcpListener;

/// Find a random unoccupied port on localhost
pub fn find_free_port() -> Option<u16> {
    (20000..60000).find_map(|port| {
        TcpListener::bind(("127.0.0.1", port)).ok().map(|listener| {
            drop(listener);
            port
        })
    })
}
