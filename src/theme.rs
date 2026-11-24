//! Terminal colour palette and VT colour resolution.

use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};
use egui::Color32;

#[derive(Clone)]
pub struct Theme {
    pub foreground: Color32,
    pub background: Color32,
    pub cursor: Color32,
    pub selection: Color32,
    pub ansi: [Color32; 16],
}

impl Default for Theme {
    fn default() -> Self {
        let c = Color32::from_rgb;
        Self {
            foreground: c(0xd4, 0xd7, 0xde),
            background: c(0x16, 0x18, 0x1d),
            cursor: c(0x7a, 0xa2, 0xf7),
            selection: Color32::from_rgba_unmultiplied(0x7a, 0xa2, 0xf7, 0x55),
            ansi: [
                c(0x1d, 0x20, 0x26), c(0xf7, 0x76, 0x8e), c(0x9e, 0xce, 0x6a), c(0xe0, 0xaf, 0x68),
                c(0x7a, 0xa2, 0xf7), c(0xbb, 0x9a, 0xf7), c(0x7d, 0xcf, 0xff), c(0xc0, 0xca, 0xf5),
                c(0x41, 0x48, 0x68), c(0xff, 0x89, 0x9d), c(0xb9, 0xf2, 0x7c), c(0xff, 0x9e, 0x64),
                c(0x8d, 0xb0, 0xff), c(0xc7, 0xa9, 0xff), c(0xa4, 0xda, 0xff), c(0xe6, 0xe9, 0xf5),
            ],
        }
    }
}

impl Theme {
    /// Resolve a cell colour. `overrides` holds colours the remote program set
    /// with OSC 4/10/11, which take priority over the theme.
    pub fn resolve(&self, color: Color, overrides: &Colors) -> Color32 {
        match color {
            Color::Spec(rgb) => to_color32(rgb),
            Color::Indexed(i) => self.indexed(i as usize, overrides),
            Color::Named(named) => self.named(named, overrides),
        }
    }

    pub fn named(&self, named: NamedColor, overrides: &Colors) -> Color32 {
        let idx = named as usize;
        if let Some(rgb) = overrides[idx] {
            return to_color32(rgb);
        }
        match named {
            NamedColor::Foreground | NamedColor::BrightForeground => self.foreground,
            NamedColor::DimForeground => dim(self.foreground),
            NamedColor::Background => self.background,
            NamedColor::Cursor => self.cursor,
            n if idx < 16 => self.ansi[n as usize],
            // DimBlack..=DimWhite
            _ => dim(self.ansi[(idx - NamedColor::DimBlack as usize) % 8]),
        }
    }

    fn indexed(&self, i: usize, overrides: &Colors) -> Color32 {
        if let Some(rgb) = overrides[i] {
            return to_color32(rgb);
        }
        match i {
            0..=15 => self.ansi[i],
            16..=231 => {
                let i = i - 16;
                let step = |v: usize| if v == 0 { 0 } else { (v * 40 + 55) as u8 };
                Color32::from_rgb(step(i / 36), step((i / 6) % 6), step(i % 6))
            }
            _ => {
                let v = (8 + (i - 232) * 10) as u8;
                Color32::from_rgb(v, v, v)
            }
        }
    }
}

fn to_color32(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb.r, rgb.g, rgb.b)
}

fn dim(c: Color32) -> Color32 {
    Color32::from_rgb(
        (c.r() as f32 * 0.66) as u8,
        (c.g() as f32 * 0.66) as u8,
        (c.b() as f32 * 0.66) as u8,
    )
}

pub fn to_rgb(c: Color32) -> Rgb {
    Rgb { r: c.r(), g: c.g(), b: c.b() }
}
