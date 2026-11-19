//! Typed, read-only views over the raw entity state: players, teams and game rules.
//!
//! ```no_run
//! # use cs2_demo::{Context, Visitor};
//! struct Positions;
//! impl Visitor for Positions {
//!     fn on_tick_end(&mut self, ctx: &Context) -> std::ops::ControlFlow<()> {
//!         if ctx.tick() % 640 == 0 {
//!             for p in ctx.players().filter(|p| p.is_alive()) {
//!                 println!("{} {:?} at {:?}", p.name(), p.team(), p.position());
//!             }
//!         }
//!         std::ops::ControlFlow::Continue(())
//!     }
//! }
//! ```

use crate::entities::{Entity, handle_to_index};
use crate::parser::Context;

/// In-game team number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Team {
    Unassigned,
    Spectator,
    Terrorist,
    CounterTerrorist,
}

impl Team {
    pub fn from_num(n: u32) -> Team {
        match n {
            2 => Team::Terrorist,
            3 => Team::CounterTerrorist,
            1 => Team::Spectator,
            _ => Team::Unassigned,
        }
    }

    pub fn num(self) -> u32 {
        match self {
            Team::Unassigned => 0,
            Team::Spectator => 1,
            Team::Terrorist => 2,
            Team::CounterTerrorist => 3,
        }
    }

    pub fn is_playing(self) -> bool {
        matches!(self, Team::Terrorist | Team::CounterTerrorist)
    }

    pub fn opponent(self) -> Team {
        match self {
            Team::Terrorist => Team::CounterTerrorist,
            Team::CounterTerrorist => Team::Terrorist,
            t => t,
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            Team::Terrorist => "T",
            Team::CounterTerrorist => "CT",
            Team::Spectator => "SPEC",
            Team::Unassigned => "-",
        }
    }
}

/// Why a round ended (`m_eRoundEndReason`, `round_end.reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RoundEndReason {
    TargetBombed,
    BombDefused,
    CtWin,
    TWin,
    Draw,
    TargetSaved,
    HostagesRescued,
    HostagesNotRescued,
    TerroristsSurrender,
    CtSurrender,
    Other(i32),
}

impl RoundEndReason {
    pub fn from_num(n: i32) -> RoundEndReason {
        match n {
            1 => RoundEndReason::TargetBombed,
            7 => RoundEndReason::BombDefused,
            8 => RoundEndReason::CtWin,
            9 => RoundEndReason::TWin,
            10 => RoundEndReason::Draw,
            11 => RoundEndReason::HostagesRescued,
            12 => RoundEndReason::TargetSaved,
            13 => RoundEndReason::HostagesNotRescued,
            17 => RoundEndReason::TerroristsSurrender,
            18 => RoundEndReason::CtSurrender,
            n => RoundEndReason::Other(n),
        }
    }
}

/// CS2 coordinates are networked as a 512-unit cell index plus an offset.
const CELL_WIDTH: f32 = 512.0;
const MAX_COORD: f32 = 16384.0;

/// World coordinate from a networked cell index and the offset inside the cell.
#[inline]
pub(crate) fn cell_coord(cell: u32, offset: f32) -> f32 {
    cell as f32 * CELL_WIDTH - MAX_COORD + offset
}

/// World position of an entity with a `CBodyComponent`.
pub fn entity_position(e: &Entity) -> Option<[f32; 3]> {
    let axis = |cell: &str, off: &str| -> Option<f32> {
        let c = e.get::<u32>(cell)?;
        let o = e.get::<f32>(off).unwrap_or(0.0);
        Some(cell_coord(c, o))
    };
    Some([
        axis("CBodyComponent.m_cellX", "CBodyComponent.m_vecX")?,
        axis("CBodyComponent.m_cellY", "CBodyComponent.m_vecY")?,
        axis("CBodyComponent.m_cellZ", "CBodyComponent.m_vecZ")?,
    ])
}

/// A player, backed by its `CCSPlayerController` (and pawn, when alive).
#[derive(Clone, Copy)]
pub struct Player<'a> {
    ctx: &'a Context,
    controller: &'a Entity,
}

impl<'a> Player<'a> {
    pub fn controller(&self) -> &'a Entity {
        self.controller
    }

    /// Player slot (as used by game events: `userid`, `attacker`, ...).
    pub fn slot(&self) -> i32 {
        self.controller.index() as i32 - 1
    }

    pub fn name(&self) -> String {
        self.controller
            .get::<String>("m_iszPlayerName")
            .unwrap_or_default()
    }

    /// SteamID64 (0 for bots).
    pub fn steam_id(&self) -> u64 {
        self.controller.get::<u64>("m_steamID").unwrap_or(0)
    }

    pub fn is_bot(&self) -> bool {
        self.steam_id() == 0
    }

    pub fn team(&self) -> Team {
        Team::from_num(self.controller.get::<u32>("m_iTeamNum").unwrap_or(0))
    }

    pub fn clan(&self) -> Option<String> {
        self.controller
            .get::<String>("m_szClan")
            .filter(|s| !s.is_empty())
    }

    /// The player's pawn (in-world body), if it exists.
    pub fn pawn(&self) -> Option<&'a Entity> {
        let h = self.controller.get::<u32>("m_hPlayerPawn")?;
        self.ctx.entities().by_handle(h)
    }

    pub fn is_alive(&self) -> bool {
        self.controller.get::<bool>("m_bPawnIsAlive").unwrap_or(false)
    }

    pub fn health(&self) -> i32 {
        self.pawn()
            .and_then(|p| p.get::<i32>("m_iHealth"))
            .or_else(|| self.controller.get::<i32>("m_iPawnHealth"))
            .unwrap_or(0)
    }

    pub fn armor(&self) -> i32 {
        self.pawn()
            .and_then(|p| p.get::<i32>("m_ArmorValue"))
            .or_else(|| self.controller.get::<i32>("m_iPawnArmor"))
            .unwrap_or(0)
    }

    pub fn has_helmet(&self) -> bool {
        self.controller.get::<bool>("m_bPawnHasHelmet").unwrap_or(false)
    }

    pub fn has_defuser(&self) -> bool {
        self.controller.get::<bool>("m_bPawnHasDefuser").unwrap_or(false)
    }

    pub fn money(&self) -> i32 {
        self.controller
            .get::<i32>("m_pInGameMoneyServices.m_iAccount")
            .unwrap_or(0)
    }

    /// Money at the start of the current round.
    pub fn start_money(&self) -> i32 {
        self.controller
            .get::<i32>("m_pInGameMoneyServices.m_iStartAccount")
            .unwrap_or(0)
    }

    pub fn cash_spent_this_round(&self) -> i32 {
        self.controller
            .get::<i32>("m_pInGameMoneyServices.m_iCashSpentThisRound")
            .unwrap_or(0)
    }

    pub fn total_cash_spent(&self) -> i32 {
        self.controller
            .get::<i32>("m_pInGameMoneyServices.m_iTotalCashSpent")
            .unwrap_or(0)
    }

    /// Value of the equipment currently carried.
    pub fn equipment_value(&self) -> i32 {
        self.pawn()
            .and_then(|p| p.get::<i32>("m_unCurrentEquipmentValue"))
            .unwrap_or(0)
    }

    pub fn round_start_equipment_value(&self) -> i32 {
        self.pawn()
            .and_then(|p| p.get::<i32>("m_unRoundStartEquipmentValue"))
            .unwrap_or(0)
    }

    pub fn freeze_end_equipment_value(&self) -> i32 {
        self.pawn()
            .and_then(|p| p.get::<i32>("m_unFreezetimeEndEquipmentValue"))
            .unwrap_or(0)
    }

    pub fn position(&self) -> Option<[f32; 3]> {
        entity_position(self.pawn()?)
    }

    /// `[pitch, yaw, roll]` in degrees.
    pub fn eye_angles(&self) -> Option<[f32; 3]> {
        self.pawn()?.get::<[f32; 3]>("m_angEyeAngles")
    }

    /// Callout name of the current location ("BombsiteA", "LongDoors", ...).
    pub fn place(&self) -> Option<String> {
        self.pawn()?
            .get::<String>("m_szLastPlaceName")
            .filter(|s| !s.is_empty())
    }

    pub fn flash_duration(&self) -> f32 {
        self.pawn()
            .and_then(|p| p.get::<f32>("m_flFlashDuration"))
            .unwrap_or(0.0)
    }

    pub fn is_scoped(&self) -> bool {
        self.pawn()
            .and_then(|p| p.get::<bool>("m_bIsScoped"))
            .unwrap_or(false)
    }

    pub fn is_defusing(&self) -> bool {
        self.pawn()
            .and_then(|p| p.get::<bool>("m_bIsDefusing"))
            .unwrap_or(false)
    }

    pub fn is_ducking(&self) -> bool {
        self.pawn()
            .and_then(|p| p.get::<bool>("m_pMovementServices.m_bDucked"))
            .unwrap_or(false)
    }

    pub fn active_weapon(&self) -> Option<Weapon<'a>> {
        let h = self.pawn()?.get::<u32>("m_pWeaponServices.m_hActiveWeapon")?;
        self.ctx.entities().by_handle(h).map(|entity| Weapon { entity })
    }

    pub fn weapons(&self) -> Vec<Weapon<'a>> {
        let Some(pawn) = self.pawn() else {
            return Vec::new();
        };
        pawn.vector::<u32>("m_pWeaponServices.m_hMyWeapons")
            .into_iter()
            .filter_map(|h| self.ctx.entities().by_handle(h))
            .map(|entity| Weapon { entity })
            .collect()
    }

    pub fn score(&self) -> i32 {
        self.controller.get::<i32>("m_iScore").unwrap_or(0)
    }

    pub fn mvps(&self) -> i32 {
        self.controller.get::<i32>("m_iMVPs").unwrap_or(0)
    }

    pub fn ping(&self) -> u32 {
        self.controller.get::<u32>("m_iPing").unwrap_or(0)
    }

    /// Server-side scoreboard counters (whole match, as tracked by the game).
    pub fn scoreboard(&self) -> ScoreboardLine {
        let g = |k: &str| {
            self.controller
                .get::<i32>(&format!("m_pActionTrackingServices.{k}"))
                .unwrap_or(0)
        };
        ScoreboardLine {
            kills: g("m_iKills"),
            deaths: g("m_iDeaths"),
            assists: g("m_iAssists"),
            damage: g("m_iDamage"),
        }
    }
}

impl std::fmt::Debug for Player<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Player")
            .field("slot", &self.slot())
            .field("name", &self.name())
            .field("steam_id", &self.steam_id())
            .field("team", &self.team())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScoreboardLine {
    pub kills: i32,
    pub deaths: i32,
    pub assists: i32,
    pub damage: i32,
}

/// A weapon entity (`CAK47`, `CWeaponAWP`, `CC4`, ...).
#[derive(Clone, Copy)]
pub struct Weapon<'a> {
    entity: &'a Entity,
}

impl<'a> Weapon<'a> {
    pub fn entity(&self) -> &'a Entity {
        self.entity
    }

    pub fn class_name(&self) -> &'a str {
        self.entity.class_name()
    }

    /// Item definition index (7 = AK-47, 9 = AWP, ...).
    pub fn item_def_index(&self) -> Option<u32> {
        self.entity.get::<u32>("m_iItemDefinitionIndex")
    }

    /// Weapon name, e.g. "ak47", "awp", "knife".
    pub fn name(&self) -> String {
        match self.item_def_index().and_then(weapon_name) {
            Some(n) => n.to_string(),
            None => self
                .class_name()
                .trim_start_matches('C')
                .trim_start_matches("Weapon")
                .to_ascii_lowercase(),
        }
    }

    pub fn clip(&self) -> Option<i32> {
        self.entity.get::<i32>("m_iClip1")
    }

    pub fn reserve_ammo(&self) -> Option<i32> {
        self.entity.get::<i32>("m_pReserveAmmo.0")
    }
}

/// Weapon name for common item definition indices.
pub fn weapon_name(def_index: u32) -> Option<&'static str> {
    Some(match def_index {
        1 => "deagle",
        2 => "elite",
        3 => "fiveseven",
        4 => "glock",
        7 => "ak47",
        8 => "aug",
        9 => "awp",
        10 => "famas",
        11 => "g3sg1",
        13 => "galilar",
        14 => "m249",
        16 => "m4a1",
        17 => "mac10",
        19 => "p90",
        23 => "mp5sd",
        24 => "ump45",
        25 => "xm1014",
        26 => "bizon",
        27 => "mag7",
        28 => "negev",
        29 => "sawedoff",
        30 => "tec9",
        31 => "taser",
        32 => "hkp2000",
        33 => "mp7",
        34 => "mp9",
        35 => "nova",
        36 => "p250",
        38 => "scar20",
        39 => "sg556",
        40 => "ssg08",
        42 | 59 | 500..=599 => "knife",
        43 => "flashbang",
        44 => "hegrenade",
        45 => "smokegrenade",
        46 => "molotov",
        47 => "decoy",
        48 => "incgrenade",
        49 => "c4",
        60 => "m4a1_silencer",
        61 => "usp_silencer",
        63 => "cz75a",
        64 => "revolver",
        _ => return None,
    })
}

/// One of the `CCSTeam` entities.
#[derive(Clone, Copy)]
pub struct TeamInfo<'a> {
    entity: &'a Entity,
}

impl<'a> TeamInfo<'a> {
    pub fn team(&self) -> Team {
        Team::from_num(self.entity.get::<u32>("m_iTeamNum").unwrap_or(0))
    }

    pub fn score(&self) -> i32 {
        self.entity.get::<i32>("m_iScore").unwrap_or(0)
    }

    pub fn score_first_half(&self) -> i32 {
        self.entity.get::<i32>("m_scoreFirstHalf").unwrap_or(0)
    }

    pub fn score_second_half(&self) -> i32 {
        self.entity.get::<i32>("m_scoreSecondHalf").unwrap_or(0)
    }

    pub fn score_overtime(&self) -> i32 {
        self.entity.get::<i32>("m_scoreOvertime").unwrap_or(0)
    }

    /// The team's name, if the server set one (tournaments and FACEIT do,
    /// Valve matchmaking doesn't).
    pub fn clan_name(&self) -> Option<String> {
        self.entity
            .get::<String>("m_szClanTeamname")
            .filter(|s| !s.is_empty())
    }

    /// Clan name ("Vitality") if set, otherwise the side name ("TERRORIST").
    pub fn name(&self) -> String {
        self.entity
            .get::<String>("m_szClanTeamname")
            .filter(|s| !s.is_empty())
            .or_else(|| self.entity.get::<String>("m_szTeamname"))
            .unwrap_or_default()
    }

    pub fn entity(&self) -> &'a Entity {
        self.entity
    }
}

/// The `CCSGameRulesProxy` entity.
#[derive(Clone, Copy)]
pub struct GameRules<'a> {
    entity: &'a Entity,
}

impl<'a> GameRules<'a> {
    fn get<T: crate::entities::FromValue>(&self, k: &str) -> Option<T> {
        self.entity.get::<T>(&format!("m_pGameRules.{k}"))
    }

    pub fn entity(&self) -> &'a Entity {
        self.entity
    }

    pub fn is_warmup(&self) -> bool {
        self.get::<bool>("m_bWarmupPeriod").unwrap_or(false)
    }

    pub fn is_freeze_period(&self) -> bool {
        self.get::<bool>("m_bFreezePeriod").unwrap_or(false)
    }

    pub fn has_match_started(&self) -> bool {
        self.get::<bool>("m_bHasMatchStarted").unwrap_or(false)
    }

    pub fn total_rounds_played(&self) -> i32 {
        self.get::<i32>("m_totalRoundsPlayed").unwrap_or(0)
    }

    pub fn game_phase(&self) -> i32 {
        self.get::<i32>("m_gamePhase").unwrap_or(0)
    }

    pub fn is_bomb_planted(&self) -> bool {
        self.get::<bool>("m_bBombPlanted").unwrap_or(false)
    }

    pub fn round_in_progress(&self) -> bool {
        self.get::<bool>("m_bRoundInProgress").unwrap_or(false)
    }

    pub fn round_start_count(&self) -> u32 {
        self.get::<u32>("m_nRoundStartCount").unwrap_or(0)
    }

    pub fn round_end_count(&self) -> u32 {
        self.get::<u32>("m_nRoundEndCount").unwrap_or(0)
    }

    pub fn round_end_winner(&self) -> Team {
        Team::from_num(self.get::<i32>("m_iRoundEndWinnerTeam").unwrap_or(0) as u32)
    }

    pub fn round_end_reason(&self) -> RoundEndReason {
        RoundEndReason::from_num(self.get::<i32>("m_eRoundEndReason").unwrap_or(0))
    }

    pub fn is_technical_timeout(&self) -> bool {
        self.get::<bool>("m_bTechnicalTimeOut").unwrap_or(false)
    }
}

impl Context {
    /// All player controllers (players, bots, and connected spectators/casters).
    pub fn players(&self) -> impl Iterator<Item = Player<'_>> {
        // Controllers always live at entity index slot + 1 (slots 0..64).
        (1..=MAX_PLAYERS)
            .filter_map(|i| self.entity(i))
            .filter(|e| e.class_name() == "CCSPlayerController")
            .map(move |controller| Player {
                ctx: self,
                controller,
            })
    }

    /// Players currently on T or CT.
    pub fn active_players(&self) -> impl Iterator<Item = Player<'_>> {
        self.players().filter(|p| p.team().is_playing())
    }

    /// Player for a game-event slot (`userid`, `attacker`, `assister`, ...).
    pub fn player_by_slot(&self, slot: i32) -> Option<Player<'_>> {
        let controller = self.entity(usize::try_from(slot + 1).ok()?)?;
        (controller.class_name() == "CCSPlayerController").then_some(Player {
            ctx: self,
            controller,
        })
    }

    pub fn player_by_steam_id(&self, steam_id: u64) -> Option<Player<'_>> {
        self.players().find(|p| p.steam_id() == steam_id)
    }

    /// Player owning a pawn handle (e.g. `userid_pawn` in events).
    pub fn player_by_pawn_handle(&self, handle: u32) -> Option<Player<'_>> {
        let idx = handle_to_index(handle)? as u32;
        self.players()
            .find(|p| p.pawn().is_some_and(|pawn| pawn.index() == idx))
    }

    pub fn teams(&self) -> impl Iterator<Item = TeamInfo<'_>> {
        self.team_indices()
            .iter()
            .filter_map(|&i| self.entity(i))
            .map(|entity| TeamInfo { entity })
    }

    pub fn team(&self, team: Team) -> Option<TeamInfo<'_>> {
        self.teams().find(|t| t.team() == team)
    }

    pub fn game_rules(&self) -> Option<GameRules<'_>> {
        self.game_rules_index()
            .and_then(|i| self.entity(i))
            .map(|entity| GameRules { entity })
    }
}

const MAX_PLAYERS: usize = 64;

/// Player identity from the `userinfo` string table. Unlike [`Player`] this is
/// available in events-only mode (no entity decoding).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PlayerInfo {
    pub slot: i32,
    pub name: String,
    pub steam_id: u64,
    pub user_id: i32,
    pub is_bot: bool,
    pub is_hltv: bool,
}

impl Context {
    /// Identity of the player in `slot` (as used by game events).
    pub fn player_info(&self, slot: i32) -> Option<PlayerInfo> {
        let table = self.string_tables().get("userinfo")?;
        let entry = table.get(usize::try_from(slot).ok()?)?;
        let info = crate::stringtables::decode_player_info(entry.value.as_deref()?).ok()?;
        Some(PlayerInfo {
            slot,
            name: info.name.unwrap_or_default(),
            steam_id: info.steamid.or(info.xuid).unwrap_or(0),
            user_id: info.userid.unwrap_or(-1),
            is_bot: info.fakeplayer.unwrap_or(false),
            is_hltv: info.ishltv.unwrap_or(false),
        })
    }

    /// All connected players (including casters and SourceTV) from `userinfo`.
    pub fn player_infos(&self) -> Vec<PlayerInfo> {
        let Some(table) = self.string_tables().get("userinfo") else {
            return Vec::new();
        };
        table
            .iter()
            .filter_map(|(i, _)| self.player_info(i as i32))
            .collect()
    }
}
