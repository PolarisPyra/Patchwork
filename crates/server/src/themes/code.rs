//! The code themes: the colors code is highlighted in, picked apart from the color scheme.

// Rust guideline compliant 2026-10-09

use serde::{Deserialize, Serialize};

use super::types::{Color, Scheme};

/// The colors of highlighted code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Syntax {
    pub keyword: Color,
    pub string: Color,
    pub property: Color,
    /// The names of types.
    pub ty: Color,
    pub number: Color,
    pub comment: Color,
    pub function: Color,
    pub tag: Color,
}

/// A code theme that can be picked for light windows or for dark ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeTheme {
    pub id: &'static str,
    pub name: &'static str,
    /// The kind of window its colors read on.
    pub scheme: Scheme,
    pub colors: Syntax,
}

/// Builds a code theme from its eight colors, in the order of [`Syntax`]'s fields.
const fn code(id: &'static str, name: &'static str, scheme: Scheme, c: [u32; 8]) -> CodeTheme {
    CodeTheme {
        id,
        name,
        scheme,
        colors: Syntax {
            keyword: Color(c[0]),
            string: Color(c[1]),
            property: Color(c[2]),
            ty: Color(c[3]),
            number: Color(c[4]),
            comment: Color(c[5]),
            function: Color(c[6]),
            tag: Color(c[7]),
        },
    }
}

/// The code themes Patchwork comes with. The colorblind ones keep red and green apart by using
/// neither: they tell tokens apart in orange and blue.
pub const CODE_THEMES: &[CodeTheme] = &[
    code(
        "github-light",
        "GitHub Light",
        Scheme::Light,
        [
            0x00cf_222e,
            0x000a_3069,
            0x0005_50ae,
            0x0095_3800,
            0x0005_50ae,
            0x006e_7781,
            0x0082_50df,
            0x0011_6329,
        ],
    ),
    code(
        "catppuccin-latte",
        "Catppuccin Latte",
        Scheme::Light,
        [
            0x0088_39ef,
            0x0040_a02b,
            0x0072_87fd,
            0x00df_8e1d,
            0x00fe_640b,
            0x007c_7f93,
            0x001e_66f5,
            0x0020_9fb5,
        ],
    ),
    code(
        "rose-pine-dawn",
        "Rosé Pine Dawn",
        Scheme::Light,
        [
            0x0028_6983,
            0x00ea_9d34,
            0x0090_7aa9,
            0x0056_949f,
            0x00d7_827e,
            0x0098_93a5,
            0x00b4_637a,
            0x0056_949f,
        ],
    ),
    code(
        "light-high-contrast",
        "High contrast",
        Scheme::Light,
        [
            0x00a0_111f,
            0x0003_2563,
            0x0002_3b95,
            0x0070_2c00,
            0x0002_3b95,
            0x0066_707b,
            0x0062_2cbc,
            0x0002_4c1a,
        ],
    ),
    code(
        "light-colorblind",
        "Colorblind",
        Scheme::Light,
        [
            0x00b3_5900,
            0x000a_3069,
            0x0005_50ae,
            0x008a_4600,
            0x0005_50ae,
            0x006e_7781,
            0x0082_50df,
            0x0009_69da,
        ],
    ),
    code(
        "github-dark",
        "GitHub Dark",
        Scheme::Dark,
        [
            0x00ff_7b72,
            0x00a5_d6ff,
            0x0079_c0ff,
            0x00ff_a657,
            0x0079_c0ff,
            0x008b_949e,
            0x00d2_a8ff,
            0x007e_e787,
        ],
    ),
    code(
        "catppuccin-mocha",
        "Catppuccin Mocha",
        Scheme::Dark,
        [
            0x00cb_a6f7,
            0x00a6_e3a1,
            0x00b4_befe,
            0x00f9_e2af,
            0x00fa_b387,
            0x0093_99b2,
            0x0089_b4fa,
            0x0074_c7ec,
        ],
    ),
    code(
        "rose-pine",
        "Rosé Pine",
        Scheme::Dark,
        [
            0x003e_8fb0,
            0x00f6_c177,
            0x00c4_a7e7,
            0x009c_cfd8,
            0x00f6_c177,
            0x006e_6a86,
            0x00eb_bcba,
            0x009c_cfd8,
        ],
    ),
    code(
        "dark-high-contrast",
        "High contrast",
        Scheme::Dark,
        [
            0x00ff_9492,
            0x00ad_dcff,
            0x0091_cbff,
            0x00ff_b757,
            0x0091_cbff,
            0x00bd_c4cc,
            0x00db_b7ff,
            0x0072_f088,
        ],
    ),
    code(
        "dark-colorblind",
        "Colorblind",
        Scheme::Dark,
        [
            0x00ec_8e2c,
            0x00a5_d6ff,
            0x0079_c0ff,
            0x00fd_ac54,
            0x0079_c0ff,
            0x008b_949e,
            0x00d2_a8ff,
            0x0058_a6ff,
        ],
    ),
];

/// The colors of the code theme with this id, when it is one for `scheme`.
pub(super) fn syntax(id: Option<&str>, scheme: Scheme) -> Option<Syntax> {
    let id = id?;
    CODE_THEMES
        .iter()
        .find(|theme| theme.id == id && theme.scheme == scheme)
        .map(|theme| theme.colors)
}
