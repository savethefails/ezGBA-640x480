//! The quick menu's Wi-Fi page: what its rows are and what each says, worked out from what the
//! Wi-Fi worker last saw. The page itself is drawn with `slot_ui::ListMenu` and its buttons are
//! `App`'s; this is only what to show.
//!
//! The first page has Wi-Fi itself, on or off, the networks the card has saved, and Scan for
//! Networks, which opens the second: every network in range, to join one.

use crate::wifi::{Failure, Phase, Snapshot, State};

/// Which of the two pages is up.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum NetPage {
    Main,
    Scan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetRow {
    /// Wi-Fi on or off. Left, Right and A turn it over.
    Power,
    /// A network the card has saved. A joins it and X forgets it.
    Saved { id: u32, ssid: String },
    /// Opens the scan.
    Scan,
    /// A network in range. A joins it, asking for its password first if it has one.
    Found { ssid: String, secured: bool },
    /// The scan has found nothing yet.
    Searching,
}

/// The rows of `page`, top to bottom. With Wi-Fi off the first page is only the switch: there is
/// nothing to join until it is on.
pub fn rows(page: NetPage, snap: &Snapshot, on: bool) -> Vec<NetRow> {
    match page {
        NetPage::Main => {
            let mut rows = vec![NetRow::Power];
            if on {
                rows.extend(snap.saved.iter().map(|s| NetRow::Saved {
                    id: s.id,
                    ssid: s.ssid.clone(),
                }));
                rows.push(NetRow::Scan);
            }
            rows
        }
        NetPage::Scan => {
            let saved = |ssid: &str| snap.saved.iter().any(|s| s.ssid == ssid);
            let mut found: Vec<_> = snap
                .scan
                .iter()
                .flatten()
                .filter(|n| !n.ssid.is_empty() && !saved(&n.ssid))
                .collect();
            // Strongest first, and each name once, at its strongest: one network is often heard
            // from several access points.
            found.sort_by_key(|n| std::cmp::Reverse(n.signal));
            let mut seen = std::collections::HashSet::new();
            found.retain(|n| seen.insert(n.ssid.as_str()));
            if found.is_empty() {
                return vec![NetRow::Searching];
            }
            found
                .into_iter()
                .map(|n| NetRow::Found {
                    ssid: n.ssid.clone(),
                    secured: n.secured,
                })
                .collect()
        }
    }
}

/// What a row says: its label, and its value if it has one.
pub fn words(row: &NetRow, snap: &Snapshot, on: bool) -> (String, Option<String>) {
    match row {
        NetRow::Power => ("Wi-Fi".into(), Some(power_word(snap, on).into())),
        NetRow::Saved { ssid, .. } => (ssid.clone(), saved_word(ssid, snap).map(str::to_string)),
        NetRow::Scan => ("Scan for Networks".into(), None),
        NetRow::Found { ssid, secured } => (
            ssid.clone(),
            Some(if *secured { "Password" } else { "Open" }.into()),
        ),
        NetRow::Searching => ("Searching...".into(), None),
    }
}

/// The switch's value says how far Wi-Fi has got, not only whether it is on.
fn power_word(snap: &Snapshot, on: bool) -> &'static str {
    if !on {
        return "Off";
    }
    if let Some((_, phase)) = &snap.attempt {
        return match phase {
            Phase::Connected => "Connected",
            Phase::Failed(_) => "Not Connected",
            _ => "Connecting...",
        };
    }
    match (snap.status.state, &snap.status.ip) {
        (State::Completed, Some(_)) => "Connected",
        (State::Associating | State::Handshake, _) | (State::Completed, None) => "Connecting...",
        _ if !snap.held => "Starting...",
        _ => "Not Connected",
    }
}

/// A saved network says so when it is the one joined or being joined, and why it would not join
/// when it would not.
fn saved_word(ssid: &str, snap: &Snapshot) -> Option<&'static str> {
    if let Some((name, failure)) = &snap.failure {
        if name == ssid {
            return Some(failure.label());
        }
    }
    if let Some((name, phase)) = &snap.attempt {
        if name == ssid {
            return Some(match phase {
                Phase::Connected => "Connected",
                Phase::Failed(f) => f.label(),
                _ => "Connecting...",
            });
        }
    }
    let here = snap.status.ssid.as_deref() == Some(ssid);
    match (here, snap.status.state, &snap.status.ip) {
        (true, State::Completed, Some(_)) => Some("Connected"),
        (true, State::Associating | State::Handshake | State::Completed, _) => {
            Some("Connecting...")
        }
        _ => None,
    }
}

/// Every word the two pages can say whatever the radio is doing, so the faces for them can be
/// made before they are needed. Network names are not among them: those are made as they appear.
pub const FIXED_WORDS: &[&str] = &[
    "Wi-Fi",
    "Off",
    "On",
    "Connected",
    "Connecting...",
    "Starting...",
    "Not Connected",
    "Scan for Networks",
    "Searching...",
    "Password",
    "Open",
];

/// The failure labels too, which a saved network's row can carry.
pub fn failure_words() -> [&'static str; 4] {
    [
        Failure::WrongPassword.label(),
        Failure::NotFound.label(),
        Failure::NoIp.label(),
        Failure::Other.label(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wifi::{Saved, Scanned, Status};

    fn snap() -> Snapshot {
        Snapshot {
            held: true,
            saved: vec![Saved {
                id: 0,
                ssid: "Home".into(),
                auto_join: true,
            }],
            scan: Some(vec![
                Scanned {
                    ssid: "Cafe".into(),
                    signal: -70,
                    secured: false,
                },
                Scanned {
                    ssid: "Home".into(),
                    signal: -40,
                    secured: true,
                },
                Scanned {
                    ssid: "Neighbour".into(),
                    signal: -50,
                    secured: true,
                },
                Scanned {
                    ssid: "Neighbour".into(),
                    signal: -80,
                    secured: true,
                },
            ]),
            status: Status {
                state: State::Completed,
                ssid: Some("Home".into()),
                ip: Some("192.168.1.20".into()),
            },
            ..Snapshot::default()
        }
    }

    #[test]
    fn off_is_only_the_switch_and_on_lists_the_saved_networks_and_the_scan() {
        assert_eq!(rows(NetPage::Main, &snap(), false), vec![NetRow::Power]);
        assert_eq!(
            rows(NetPage::Main, &snap(), true),
            vec![
                NetRow::Power,
                NetRow::Saved {
                    id: 0,
                    ssid: "Home".into()
                },
                NetRow::Scan
            ]
        );
    }

    #[test]
    fn the_scan_lists_each_unsaved_network_once_strongest_first() {
        assert_eq!(
            rows(NetPage::Scan, &snap(), true),
            vec![
                NetRow::Found {
                    ssid: "Neighbour".into(),
                    secured: true
                },
                NetRow::Found {
                    ssid: "Cafe".into(),
                    secured: false
                },
            ]
        );
        let empty = Snapshot {
            scan: None,
            ..snap()
        };
        assert_eq!(rows(NetPage::Scan, &empty, true), vec![NetRow::Searching]);
    }

    #[test]
    fn the_switch_and_the_joined_network_say_connected() {
        let s = snap();
        assert_eq!(
            words(&NetRow::Power, &s, true).1.as_deref(),
            Some("Connected")
        );
        assert_eq!(words(&NetRow::Power, &s, false).1.as_deref(), Some("Off"));
        let home = NetRow::Saved {
            id: 0,
            ssid: "Home".into(),
        };
        assert_eq!(
            words(&home, &s, true),
            ("Home".into(), Some("Connected".into()))
        );
    }

    #[test]
    fn a_failed_join_says_why_on_its_network() {
        let s = Snapshot {
            failure: Some(("Home".into(), Failure::WrongPassword)),
            status: Status::default(),
            ..snap()
        };
        let home = NetRow::Saved {
            id: 0,
            ssid: "Home".into(),
        };
        assert_eq!(words(&home, &s, true).1.as_deref(), Some("Wrong Password"));
    }

    #[test]
    fn every_word_a_page_can_say_but_a_name_is_made_ahead() {
        let s = snap();
        for page in [NetPage::Main, NetPage::Scan] {
            for row in rows(page, &s, true) {
                let (label, value) = words(&row, &s, true);
                for w in [Some(label), value].into_iter().flatten() {
                    let named = s.saved.iter().any(|n| n.ssid == w)
                        || s.scan.iter().flatten().any(|n| n.ssid == w);
                    assert!(
                        named
                            || FIXED_WORDS.contains(&w.as_str())
                            || failure_words().contains(&w.as_str()),
                        "{w:?} has no face made ahead"
                    );
                }
            }
        }
    }
}
