//! Key-name table: profile key names -> Linux evdev key codes
//! (values from linux/input-event-codes.h). Only names in this table are
//! accepted in profiles; anything else is rejected at validation time.

pub const KEY_TABLE: &[(&str, u16)] = &[
    ("esc", 1),
    ("1", 2),
    ("2", 3),
    ("3", 4),
    ("4", 5),
    ("5", 6),
    ("6", 7),
    ("7", 8),
    ("8", 9),
    ("9", 10),
    ("0", 11),
    ("minus", 12),
    ("equal", 13),
    ("backspace", 14),
    ("tab", 15),
    ("q", 16),
    ("w", 17),
    ("e", 18),
    ("r", 19),
    ("t", 20),
    ("y", 21),
    ("u", 22),
    ("i", 23),
    ("o", 24),
    ("p", 25),
    ("leftbrace", 26),
    ("rightbrace", 27),
    ("enter", 28),
    ("ctrl", 29),
    ("a", 30),
    ("s", 31),
    ("d", 32),
    ("f", 33),
    ("g", 34),
    ("h", 35),
    ("j", 36),
    ("k", 37),
    ("l", 38),
    ("semicolon", 39),
    ("apostrophe", 40),
    ("grave", 41),
    ("shift", 42),
    ("backslash", 43),
    ("z", 44),
    ("x", 45),
    ("c", 46),
    ("v", 47),
    ("b", 48),
    ("n", 49),
    ("m", 50),
    ("comma", 51),
    ("dot", 52),
    ("slash", 53),
    ("rightshift", 54),
    ("kpasterisk", 55),
    ("alt", 56),
    ("space", 57),
    ("capslock", 58),
    ("f1", 59),
    ("f2", 60),
    ("f3", 61),
    ("f4", 62),
    ("f5", 63),
    ("f6", 64),
    ("f7", 65),
    ("f8", 66),
    ("f9", 67),
    ("f10", 68),
    ("numlock", 69),
    ("scrolllock", 70),
    ("kp7", 71),
    ("kp8", 72),
    ("kp9", 73),
    ("kpminus", 74),
    ("kp4", 75),
    ("kp5", 76),
    ("kp6", 77),
    ("kpplus", 78),
    ("kp1", 79),
    ("kp2", 80),
    ("kp3", 81),
    ("kp0", 82),
    ("kpdot", 83),
    ("f11", 87),
    ("f12", 88),
    ("kpenter", 96),
    ("rightctrl", 97),
    ("kpslash", 98),
    ("sysrq", 99),
    ("rightalt", 100),
    ("home", 102),
    ("up", 103),
    ("pageup", 104),
    ("left", 105),
    ("right", 106),
    ("end", 107),
    ("down", 108),
    ("pagedown", 109),
    ("insert", 110),
    ("delete", 111),
    ("mute", 113),
    ("volumedown", 114),
    ("volumeup", 115),
    ("kpequal", 117),
    ("pause", 119),
    ("super", 125),
    ("rightsuper", 126),
    ("compose", 127),
    ("f13", 183),
    ("f14", 184),
    ("f15", 185),
    ("f16", 186),
    ("f17", 187),
    ("f18", 188),
    ("f19", 189),
    ("f20", 190),
    ("f21", 191),
    ("f22", 192),
    ("f23", 193),
    ("f24", 194),
    ("nextsong", 163),
    ("playpause", 164),
    ("previoussong", 165),
    ("stopcd", 166),
    ("print", 210),
    ("brightnessdown", 224),
    ("brightnessup", 225),
];

/// Aliases accepted for convenience.
const ALIASES: &[(&str, &str)] = &[
    ("control", "ctrl"),
    ("leftctrl", "ctrl"),
    ("lctrl", "ctrl"),
    ("leftshift", "shift"),
    ("lshift", "shift"),
    ("leftalt", "alt"),
    ("lalt", "alt"),
    ("meta", "super"),
    ("leftmeta", "super"),
    ("win", "super"),
    ("logo", "super"),
    ("escape", "esc"),
    ("return", "enter"),
    ("period", "dot"),
    ("del", "delete"),
    ("pgup", "pageup"),
    ("pgdn", "pagedown"),
    ("next", "nextsong"),
    ("prev", "previoussong"),
    ("play", "playpause"),
    ("plus", "equal"),
];

pub const MODIFIERS: &[u16] = &[29, 42, 56, 125, 97, 54, 100, 126];

pub fn code(name: &str) -> Option<u16> {
    let n = name.trim().to_ascii_lowercase();
    let n = ALIASES.iter().find(|(a, _)| *a == n).map(|(_, b)| b.to_string()).unwrap_or(n);
    KEY_TABLE.iter().find(|(k, _)| *k == n).map(|(_, c)| *c)
}

pub fn name(code: u16) -> Option<&'static str> {
    KEY_TABLE.iter().find(|(_, c)| *c == code).map(|(k, _)| *k)
}

pub fn is_modifier(code: u16) -> bool {
    MODIFIERS.contains(&code)
}

/// Every code that may ever be injected through the virtual keyboard.
pub fn all_codes() -> impl Iterator<Item = u16> {
    KEY_TABLE.iter().map(|(_, c)| *c)
}

/// Parse "ctrl+shift+z" into ordered codes (modifiers first).
pub fn parse_chord(s: &str) -> Result<Vec<u16>, String> {
    let mut mods = Vec::new();
    let mut keys = Vec::new();
    for part in s.split('+') {
        let c = code(part).ok_or_else(|| format!("unknown key `{}`", part.trim()))?;
        if is_modifier(c) {
            mods.push(c)
        } else {
            keys.push(c)
        }
    }
    if mods.len() + keys.len() > 6 {
        return Err("chord has more than 6 keys".into());
    }
    mods.extend(keys);
    mods.dedup();
    if mods.is_empty() {
        return Err("empty chord".into());
    }
    if is_sysrq_combo(&mods) {
        return Err("alt+sysrq/print combinations are refused (kernel magic SysRq)".into());
    }
    Ok(mods)
}

/// KEY_SYSRQ (99, also Print Screen) together with Alt is the kernel's
/// magic SysRq trigger (reboot, kill all, …). The virtual device must never
/// be able to produce it, whatever a profile says.
pub const KEY_SYSRQ: u16 = 99;
pub const ALT_KEYS: [u16; 2] = [56, 100];

pub fn is_sysrq_combo(codes: &[u16]) -> bool {
    codes.contains(&KEY_SYSRQ) && codes.iter().any(|c| ALT_KEYS.contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chords() {
        assert_eq!(parse_chord("ctrl+shift+z").unwrap(), vec![29, 42, 44]);
        assert_eq!(parse_chord("Z+Control").unwrap(), vec![29, 44]);
        assert!(parse_chord("ctrl+bogus").is_err());
        assert_eq!(parse_chord("kp7").unwrap(), vec![71]);
        assert_eq!(parse_chord("sysrq").unwrap(), vec![99], "plain Print Screen is fine");
        assert!(parse_chord("alt+sysrq+b").is_err());
        assert!(parse_chord("rightalt+sysrq").is_err());
    }
    #[test]
    fn table_unique() {
        let mut n: Vec<_> = KEY_TABLE.iter().map(|k| k.0).collect();
        n.sort();
        let l = n.len();
        n.dedup();
        assert_eq!(l, n.len());
        let mut c: Vec<_> = KEY_TABLE.iter().map(|k| k.1).collect();
        c.sort();
        let l = c.len();
        c.dedup();
        assert_eq!(l, c.len());
    }
}
