//! Which of a box's addresses to dial. Every candidate is probed at once and the most
//! preferred one that answers wins, so an unreachable home address costs one short timeout
//! rather than one per candidate.

use borrow_core::config::Agent;
use borrow_core::network::Network;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Mutex;
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_millis(800);
const SSH_PORT: u16 = 22;

/// Set by the Client for the rsync helper it starts, so that process skips probing.
pub const ROUTE_ENV: &str = "BORROW_RSH_HOST";

#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub host: String,
    pub network: Network,
}

impl Route {
    fn to(host: &str) -> Route {
        Route {
            host: host.to_string(),
            network: Network::of(host),
        }
    }
}

/// One process talks to one box, so the first answer is kept for every later ssh call.
static CHOSEN: Mutex<Option<(String, Route)>> = Mutex::new(None);

/// The address to dial for `agent`. When nothing answers, the pairing address is returned
/// so ssh reports the failure in its own words.
pub fn resolve(agent: &Agent) -> Route {
    let mut chosen = CHOSEN.lock().expect("route lock was poisoned");
    if let Some((name, route)) = chosen.as_ref()
        && *name == agent.name
    {
        return route.clone();
    }
    let route = probe(agent);
    *chosen = Some((agent.name.clone(), route.clone()));
    route
}

/// Use `host` without probing, when a parent process already chose it. Ignored unless it
/// is one of the box's own addresses.
pub fn assume(agent: &Agent, host: &str) {
    if agent.candidates().contains(&host) {
        *CHOSEN.lock().expect("route lock was poisoned") =
            Some((agent.name.clone(), Route::to(host)));
    }
}

fn probe(agent: &Agent) -> Route {
    let candidates = agent.candidates();
    if let [only] = candidates.as_slice() {
        return Route::to(only);
    }
    let port = agent.port.unwrap_or(SSH_PORT);
    let attempts: Vec<_> = candidates
        .iter()
        .map(|host| {
            let host = host.to_string();
            std::thread::spawn(move || answers(&host, port))
        })
        .collect();
    for (host, attempt) in candidates.iter().zip(attempts) {
        if attempt.join().unwrap_or(false) {
            return Route::to(host);
        }
    }
    Route::to(&agent.host)
}

fn answers(host: &str, port: u16) -> bool {
    let Ok(addresses) = (host, port).to_socket_addrs() else {
        return false;
    };
    addresses
        .into_iter()
        .any(|address| TcpStream::connect_timeout(&address, PROBE_TIMEOUT).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn agent(host: &str, addresses: &[&str], port: Option<u16>) -> Agent {
        Agent {
            name: "archbox".to_string(),
            host: host.to_string(),
            user: "me".to_string(),
            port,
            daemon_port: None,
            identity_file: None,
            known_hosts: None,
            program: None,
            addresses: addresses.iter().map(|a| a.to_string()).collect(),
            specs: None,
        }
    }

    #[test]
    fn the_first_preferred_address_that_answers_wins() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let box_ = agent("192.0.2.1", &["127.0.0.1"], Some(port));

        assert_eq!(probe(&box_), Route::to("127.0.0.1"));
    }

    #[test]
    fn with_nothing_answering_the_pairing_address_is_used() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let box_ = agent("192.0.2.1", &["127.0.0.1"], Some(port));

        assert_eq!(probe(&box_).host, "192.0.2.1");
    }

    #[test]
    fn a_single_address_is_used_without_probing() {
        let box_ = agent("192.0.2.1", &[], None);

        assert_eq!(probe(&box_), Route::to("192.0.2.1"));
    }
}
