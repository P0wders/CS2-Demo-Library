//! Parser for Valve's networked type strings, e.g.
//! `int32`, `CHandle< CBaseEntity >`, `CNetworkUtlVectorBase< CHandle< CBasePlayerWeapon > >`,
//! `CCSPlayer_WeaponServices*`, `uint64[4]`, `char[512]`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldType {
    pub base: String,
    pub generic: Option<Box<FieldType>>,
    pub pointer: bool,
    /// Fixed array length (`T[N]`). Symbolic sizes are resolved via a small table;
    /// unknown symbolic sizes fall back to 1024.
    pub count: Option<usize>,
}

impl FieldType {
    pub fn parse(s: &str) -> FieldType {
        let s = s.trim();
        let base_end = s.find(['<', '[', '*']).unwrap_or(s.len());
        let base = s[..base_end].trim().to_string();
        let mut rest = &s[base_end..];

        let mut generic = None;
        if rest.starts_with('<') {
            let mut depth = 0usize;
            let mut close = None;
            for (i, c) in rest.char_indices() {
                match c {
                    '<' => depth += 1,
                    '>' => {
                        depth -= 1;
                        if depth == 0 {
                            close = Some(i);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let close = close.unwrap_or(rest.len());
            let inner = rest[1..close.min(rest.len())].trim();
            if !inner.is_empty() {
                generic = Some(Box::new(FieldType::parse(inner)));
            }
            rest = rest.get(close + 1..).unwrap_or("").trim_start();
        }

        let mut pointer = false;
        if rest.starts_with('*') {
            pointer = true;
            rest = rest[1..].trim_start();
        }

        let mut count = None;
        if rest.starts_with('[') {
            let end = rest.find(']').unwrap_or(rest.len());
            let n = rest[1..end].trim();
            count = Some(match n {
                "MAX_ITEM_STOCKS" => 8,
                "MAX_ABILITY_DRAFT_ABILITIES" => 48,
                _ => n.parse::<usize>().ok().filter(|&v| v > 0).unwrap_or(1024),
            });
        }

        FieldType {
            base,
            generic,
            pointer,
            count,
        }
    }

    /// `CUtlVector<T>`-like dynamic arrays (length-prefixed on the wire).
    pub fn is_dynamic_vector(&self) -> bool {
        matches!(
            self.base.as_str(),
            "CUtlVector" | "CNetworkUtlVectorBase" | "CUtlVectorEmbeddedNetworkVar"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple() {
        let t = FieldType::parse("int32");
        assert_eq!(t.base, "int32");
        assert!(t.generic.is_none() && !t.pointer && t.count.is_none());
    }

    #[test]
    fn nested_generic() {
        let t = FieldType::parse("CNetworkUtlVectorBase< CHandle< CBasePlayerWeapon > >");
        assert_eq!(t.base, "CNetworkUtlVectorBase");
        let g = t.generic.unwrap();
        assert_eq!(g.base, "CHandle");
        assert_eq!(g.generic.unwrap().base, "CBasePlayerWeapon");
    }

    #[test]
    fn pointer_and_array() {
        let p = FieldType::parse("CCSPlayer_WeaponServices*");
        assert_eq!(p.base, "CCSPlayer_WeaponServices");
        assert!(p.pointer);
        let a = FieldType::parse("uint64[4]");
        assert_eq!((a.base.as_str(), a.count), ("uint64", Some(4)));
        let c = FieldType::parse("char[512]");
        assert_eq!(c.count, Some(512));
        let h = FieldType::parse("CHandle< CBaseEntity >[2]");
        assert_eq!((h.base.as_str(), h.count), ("CHandle", Some(2)));
        assert_eq!(h.generic.unwrap().base, "CBaseEntity");
    }
}
