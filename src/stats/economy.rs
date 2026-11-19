//! Team economy per round: money, equipment value and the kind of buy.

use crate::model::Team;

/// How much a team invested in a round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BuyType {
    /// First round of a half: everyone started with the starting money.
    Pistol,
    Eco,
    SemiEco,
    Force,
    FullBuy,
}

impl BuyType {
    pub const ALL: [BuyType; 5] = [
        BuyType::Pistol,
        BuyType::Eco,
        BuyType::SemiEco,
        BuyType::Force,
        BuyType::FullBuy,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BuyType::Pistol => "Pistol",
            BuyType::Eco => "Eco",
            BuyType::SemiEco => "Semi-eco",
            BuyType::Force => "Force buy",
            BuyType::FullBuy => "Full buy",
        }
    }
}

/// Where the buy types start. Equipment values are per player on average,
/// measured when freeze time ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuyThresholds {
    /// Below this average equipment value: [`BuyType::Eco`].
    pub eco: i32,
    /// Below this: [`BuyType::SemiEco`].
    pub semi_eco: i32,
    /// Below this: [`BuyType::Force`]; from here on: [`BuyType::FullBuy`].
    pub force: i32,
    /// A round where every player of the team started with at most this much
    /// money is a [`BuyType::Pistol`] round (the first round of each half;
    /// overtime halves start with far more).
    pub pistol_start_money: i32,
}

impl BuyThresholds {
    /// $1,000 / $2,000 / $4,000 per player (so $5k / $10k / $20k for a team of
    /// five), and pistol rounds at the $800 starting money.
    pub const DEFAULT: BuyThresholds = BuyThresholds {
        eco: 1_000,
        semi_eco: 2_000,
        force: 4_000,
        pistol_start_money: 1_000,
    };

    /// Classifies a team's round from its players' start money and equipment value.
    pub fn classify(&self, players: u32, max_start_money: i32, equipment_value: i32) -> BuyType {
        if players > 0 && max_start_money <= self.pistol_start_money {
            return BuyType::Pistol;
        }
        let avg = equipment_value / players.max(1) as i32;
        if avg < self.eco {
            BuyType::Eco
        } else if avg < self.semi_eco {
            BuyType::SemiEco
        } else if avg < self.force {
            BuyType::Force
        } else {
            BuyType::FullBuy
        }
    }
}

impl Default for BuyThresholds {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// One team's economy in one round.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TeamEconomy {
    pub side: Team,
    /// Players on the side when freeze time ended.
    pub players: u32,
    /// Money the players had at the start of the round, summed.
    pub money_start: i32,
    /// Value of the equipment the players carried when freeze time ended, summed.
    pub equipment_value: i32,
    /// Money spent during the round, summed.
    pub cash_spent: i32,
    /// The game's count of consecutive rounds lost by this side, which sets the
    /// loss bonus (`m_iNumConsecutiveCTLoses` / `m_iNumConsecutiveTerroristLoses`;
    /// CS2 starts it at 1).
    pub loss_streak: i32,
    pub buy: BuyType,
}

impl TeamEconomy {
    /// Average equipment value per player.
    pub fn avg_equipment_value(&self) -> i32 {
        self.equipment_value / self.players.max(1) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify() {
        let t = BuyThresholds::DEFAULT;
        assert_eq!(t.classify(5, 800, 4_250), BuyType::Pistol);
        assert_eq!(t.classify(5, 2_400, 4_000), BuyType::Eco);
        assert_eq!(t.classify(5, 3_000, 9_999), BuyType::SemiEco);
        assert_eq!(t.classify(5, 3_000, 10_000), BuyType::Force);
        assert_eq!(t.classify(5, 5_000, 24_500), BuyType::FullBuy);
        // Four players: the same average per player.
        assert_eq!(t.classify(4, 5_000, 16_000), BuyType::FullBuy);
        // Overtime starts with $12,500: not a pistol round.
        assert_eq!(t.classify(5, 12_500, 26_000), BuyType::FullBuy);
        assert_eq!(t.classify(0, 0, 0), BuyType::Eco);
    }
}
