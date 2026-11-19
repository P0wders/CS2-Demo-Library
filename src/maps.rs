//! Radar calibration: where a world position lands on a map's overview image.
//!
//! Each map ships an overview description (`resource/overviews/<map>.txt` in
//! the game files) giving the world coordinates of the radar image's top-left
//! corner and how many world units one pixel covers. Radar images are
//! 1024 × 1024 pixels.
//!
//! ```
//! use cs2_demo::maps::MapOverview;
//!
//! let dust2 = MapOverview::builtin("de_dust2").unwrap();
//! let [x, y] = dust2.to_radar([0.0, 0.0, 0.0]);
//! assert!((0.0..1024.0).contains(&x) && (0.0..1024.0).contains(&y));
//! ```

/// Calibration of a map's radar image.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MapOverview {
    pub name: String,
    /// World x of the radar image's left edge.
    pub pos_x: f32,
    /// World y of the radar image's top edge.
    pub pos_y: f32,
    /// World units per radar pixel.
    pub scale: f32,
    /// Maps with two levels (Nuke, Vertigo): positions below this height
    /// belong on the lower radar image (`<map>_lower_radar`), which uses the
    /// same calibration.
    pub lower_below: Option<f32>,
}

/// Radar images are this many pixels wide and high.
pub const RADAR_SIZE: f32 = 1024.0;

/// (name, pos_x, pos_y, scale, lower level below).
const BUILTIN: &[(&str, f32, f32, f32, Option<f32>)] = &[
    ("de_ancient", -2953.0, 2164.0, 5.0, None),
    ("de_anubis", -2796.0, 3328.0, 5.22, None),
    ("de_cache", -2000.0, 3250.0, 5.5, None),
    ("de_dust2", -2476.0, 3239.0, 4.4, None),
    ("de_inferno", -2087.0, 3870.0, 4.9, None),
    ("de_mirage", -3230.0, 1713.0, 5.0, None),
    ("de_nuke", -3453.0, 2887.0, 7.0, Some(-495.0)),
    ("de_overpass", -4831.0, 1781.0, 5.2, None),
    ("de_train", -2308.0, 2078.0, 4.082, None),
    ("de_vertigo", -3168.0, 1762.0, 4.0, Some(11700.0)),
];

impl MapOverview {
    /// Calibration for a common competitive map (`de_dust2`, `de_mirage`, ...).
    /// For other maps, read the game's overview file with [`MapOverview::from_kv`].
    pub fn builtin(map: &str) -> Option<MapOverview> {
        let map = map.rsplit(['/', '\\']).next().unwrap_or(map);
        BUILTIN.iter().find(|b| b.0.eq_ignore_ascii_case(map)).map(
            |&(name, pos_x, pos_y, scale, lower_below)| MapOverview {
                name: name.to_string(),
                pos_x,
                pos_y,
                scale,
                lower_below,
            },
        )
    }

    /// Names of the maps [`MapOverview::builtin`] knows.
    pub fn builtin_maps() -> impl Iterator<Item = &'static str> {
        BUILTIN.iter().map(|b| b.0)
    }

    /// Reads an overview description in the game's KeyValues format:
    ///
    /// ```text
    /// "de_nuke"
    /// {
    ///     "pos_x"  "-3453"
    ///     "pos_y"  "2887"
    ///     "scale"  "7"
    ///     "verticalsections" { "lower" { "AltitudeMax" "-495" } }
    /// }
    /// ```
    pub fn from_kv(text: &str) -> Option<MapOverview> {
        let tokens = kv_tokens(text);
        let name = tokens.iter().find_map(|t| match t {
            Token::Str(s) => Some(s.to_string()),
            _ => None,
        })?;
        let mut pos_x = None;
        let mut pos_y = None;
        let mut scale = None;
        let mut lower_below = None;
        // Path of enclosing keys, to find verticalsections/lower/AltitudeMax.
        let mut stack: Vec<String> = Vec::new();
        let mut i = 0;
        let mut pending: Option<String> = None;
        while i < tokens.len() {
            match &tokens[i] {
                Token::Open => {
                    stack.push(pending.take().unwrap_or_default().to_ascii_lowercase());
                }
                Token::Close => {
                    stack.pop();
                    pending = None;
                }
                Token::Str(key) => {
                    if let Some(Token::Str(value)) = tokens.get(i + 1) {
                        let v = value.trim().parse::<f32>().ok();
                        let in_lower = stack.iter().any(|k| k == "lower");
                        match key.to_ascii_lowercase().as_str() {
                            "pos_x" if !in_lower => pos_x = v,
                            "pos_y" if !in_lower => pos_y = v,
                            "scale" if !in_lower => scale = v,
                            "altitudemax" if in_lower => lower_below = v,
                            _ => {}
                        }
                        i += 2;
                        continue;
                    }
                    pending = Some(key.to_string());
                }
            }
            i += 1;
        }
        Some(MapOverview {
            name,
            pos_x: pos_x?,
            pos_y: pos_y?,
            scale: scale.filter(|s| *s > 0.0)?,
            lower_below,
        })
    }

    /// Radar pixel of a world position: x to the right, y down, (0, 0) at
    /// the image's top-left corner, (1024, 1024) at its bottom-right.
    pub fn to_radar(&self, pos: [f32; 3]) -> [f32; 2] {
        [
            (pos[0] - self.pos_x) / self.scale,
            (self.pos_y - pos[1]) / self.scale,
        ]
    }

    /// World x and y of a radar pixel.
    pub fn to_world(&self, px: [f32; 2]) -> [f32; 2] {
        [px[0] * self.scale + self.pos_x, self.pos_y - px[1] * self.scale]
    }

    /// Whether a position belongs on the lower radar image.
    pub fn is_lower(&self, z: f32) -> bool {
        self.lower_below.is_some_and(|l| z < l)
    }

    /// Whether a position falls on the radar image (with `margin` pixels to spare).
    pub fn contains(&self, pos: [f32; 3], margin: f32) -> bool {
        let [x, y] = self.to_radar(pos);
        (-margin..=RADAR_SIZE + margin).contains(&x) && (-margin..=RADAR_SIZE + margin).contains(&y)
    }
}

#[derive(Debug, PartialEq)]
enum Token<'a> {
    Str(&'a str),
    Open,
    Close,
}

/// Quoted strings, bare words and braces; `//` comments are skipped.
fn kv_tokens(text: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'{' => {
                out.push(Token::Open);
                i += 1;
            }
            b'}' => {
                out.push(Token::Close);
                i += 1;
            }
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => {
                let start = i + 1;
                let end = text[start..].find('"').map_or(b.len(), |e| start + e);
                out.push(Token::Str(&text[start..end]));
                i = end + 1;
            }
            c if c.is_ascii_whitespace() => i += 1,
            _ => {
                let start = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'{' | b'}' | b'"') {
                    i += 1;
                }
                out.push(Token::Str(&text[start..i]));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const NUKE: &str = r#"
// Overview: Nuke
"de_nuke"
{
	"material"	"overviews/de_nuke"
	"pos_x"		"-3453"	// upper left world coordinate
	"pos_y"		"2887"
	"scale"		"7.00"
	"verticalsections"
	{
		"default" // use the primary radar image
		{
			"AltitudeMax" "10000"
			"AltitudeMin" "-495"
		}
		"lower" // i.e. de_nuke_lower_radar.dds
		{
			"AltitudeMax" "-495"
			"AltitudeMin" "-10000"
		}
	}
}
"#;

    #[test]
    fn reads_overview_files() {
        let o = MapOverview::from_kv(NUKE).unwrap();
        assert_eq!(o, MapOverview::builtin("de_nuke").unwrap());
        assert!(o.is_lower(-600.0) && !o.is_lower(-400.0));
        let flat = MapOverview::from_kv(r#""de_x" { "pos_x" "-100" "pos_y" "200" "scale" "2" }"#).unwrap();
        assert_eq!(
            (flat.pos_x, flat.pos_y, flat.scale, flat.lower_below),
            (-100.0, 200.0, 2.0, None)
        );
        assert!(MapOverview::from_kv(r#""de_x" { "pos_x" "1" }"#).is_none());
    }

    #[test]
    fn radar_round_trip() {
        let o = MapOverview::builtin("maps/DE_DUST2").unwrap();
        assert_eq!(o.to_radar([o.pos_x, o.pos_y, 0.0]), [0.0, 0.0]);
        let corner = o.to_world([1024.0, 1024.0]);
        assert_eq!(o.to_radar([corner[0], corner[1], 0.0]), [1024.0, 1024.0]);
        assert!(o.contains([0.0, 0.0, 0.0], 0.0));
        assert!(!o.contains([o.pos_x - 100.0, 0.0, 0.0], 0.0));
        assert_eq!(MapOverview::builtin_maps().count(), BUILTIN.len());
        assert!(MapOverview::builtin("de_unknown").is_none());
    }
}
