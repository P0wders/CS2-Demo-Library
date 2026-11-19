//! Team names. Tournament and FACEIT servers name the teams; Valve
//! matchmaking doesn't, and its side names ("TERRORIST", "CT") change hands at
//! half-time. Such a team is named after one of its players ("Team ZyWho")
//! and followed across side switches by who plays in it.

use super::player_key;
use crate::model::Team;
use crate::parser::Context;
use rustc_hash::FxHashMap;

#[derive(Debug, Default, Clone)]
pub(crate) struct Teams {
    /// Player key → index into `names` (unnamed teams only).
    team_of: FxHashMap<u64, usize>,
    names: Vec<String>,
}

impl Teams {
    /// Name of the team playing `side` now.
    pub fn name(&mut self, ctx: &Context, side: Team) -> String {
        let Some(info) = ctx.team(side) else {
            return String::new();
        };
        if let Some(clan) = info.clan_name() {
            return clan;
        }
        let players: Vec<(u64, String)> = ctx
            .active_players()
            .filter(|p| p.team() == side)
            .map(|p| (player_key(p.steam_id(), p.slot()), p.name()))
            .collect();
        if players.is_empty() {
            return info.name();
        }
        let team = match self.majority(&players) {
            Some(t) => t,
            None => {
                let first = players
                    .iter()
                    .map(|(_, n)| n)
                    .min_by_key(|n| n.to_lowercase())
                    .cloned()
                    .unwrap_or_default();
                self.names.push(format!("Team {first}"));
                self.names.len() - 1
            }
        };
        for (k, _) in &players {
            self.team_of.entry(*k).or_insert(team);
        }
        self.names[team].clone()
    }

    /// The team most of these players were seen in.
    fn majority(&self, players: &[(u64, String)]) -> Option<usize> {
        let mut votes: Vec<(usize, u32)> = Vec::new();
        for (k, _) in players {
            if let Some(&t) = self.team_of.get(k) {
                match votes.iter_mut().find(|v| v.0 == t) {
                    Some(v) => v.1 += 1,
                    None => votes.push((t, 1)),
                }
            }
        }
        votes.into_iter().max_by_key(|v| v.1).map(|v| v.0)
    }
}
