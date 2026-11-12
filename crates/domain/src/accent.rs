use serde::{Deserialize, Serialize};

/// The app's accent colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Accent {
    /// The theme's own neutral primary.
    #[default]
    Monochrome,
    Blue,
    Red,
    Green,
    Purple,
    Orange,
}

impl Accent {
    pub const ALL: [Self; 6] = [
        Self::Monochrome,
        Self::Blue,
        Self::Red,
        Self::Green,
        Self::Purple,
        Self::Orange,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Monochrome => "Monochrome",
            Self::Blue => "Blue",
            Self::Red => "Red",
            Self::Green => "Green",
            Self::Purple => "Purple",
            Self::Orange => "Orange",
        }
    }

    /// The colour as `0xrrggbb`, or `None` for [`Accent::Monochrome`], which follows the theme.
    pub fn hex(self) -> Option<u32> {
        match self {
            Self::Monochrome => None,
            Self::Blue => Some(0x3b82f6),
            Self::Red => Some(0xef4444),
            Self::Green => Some(0x22c55e),
            Self::Purple => Some(0xa855f7),
            Self::Orange => Some(0xf97316),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_monochrome() {
        assert_eq!(Accent::default(), Accent::Monochrome);
        assert_eq!(Accent::ALL[0], Accent::Monochrome);
    }

    #[test]
    fn choices_are_in_order() {
        let names: Vec<_> = Accent::ALL.iter().map(|a| a.name()).collect();
        assert_eq!(
            names,
            ["Monochrome", "Blue", "Red", "Green", "Purple", "Orange"]
        );
    }

    #[test]
    fn hex_values() {
        let hexes: Vec<_> = Accent::ALL.iter().map(|a| a.hex()).collect();
        assert_eq!(
            hexes,
            [
                None,
                Some(0x3b82f6),
                Some(0xef4444),
                Some(0x22c55e),
                Some(0xa855f7),
                Some(0xf97316),
            ]
        );
    }

    #[test]
    fn serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&Accent::Purple).unwrap(),
            r#""purple""#
        );
        let accent: Accent = serde_json::from_str(r#""monochrome""#).unwrap();
        assert_eq!(accent, Accent::Monochrome);
    }
}
