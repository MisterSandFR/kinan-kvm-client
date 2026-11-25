//! HID usage-page 0x07 (keyboard) table and modifier handling.
//!
//! The switch expects USB HID boot-protocol keyboard reports: one modifier
//! byte, one reserved byte, and up to six concurrently pressed key usages.

use std::collections::BTreeSet;

/// Individual modifier bits used by the HID report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Modifier {
    /// Left Control.
    ControlLeft,
    /// Left Shift.
    ShiftLeft,
    /// Left Alt.
    AltLeft,
    /// Left GUI / Win / Meta.
    MetaLeft,
    /// Right Control.
    ControlRight,
    /// Right Shift.
    ShiftRight,
    /// Right Alt.
    AltRight,
    /// Right GUI / Win / Meta.
    MetaRight,
}

impl Modifier {
    /// Bit position in the HID modifier byte.
    pub const fn bit(self) -> u8 {
        match self {
            Self::ControlLeft => 0x01,
            Self::ShiftLeft => 0x02,
            Self::AltLeft => 0x04,
            Self::MetaLeft => 0x08,
            Self::ControlRight => 0x10,
            Self::ShiftRight => 0x20,
            Self::AltRight => 0x40,
            Self::MetaRight => 0x80,
        }
    }
}

/// Current set of pressed modifiers, convertible to the HID modifier byte.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Modifiers(pub u8);

impl Modifiers {
    /// Add a modifier.
    pub fn press(&mut self, m: Modifier) {
        self.0 |= m.bit();
    }
    /// Remove a modifier.
    pub fn release(&mut self, m: Modifier) {
        self.0 &= !m.bit();
    }
    /// Clear all modifiers.
    pub fn clear(&mut self) {
        self.0 = 0;
    }
}

/// Tracks the up-to-six concurrently pressed non-modifier keys.
#[derive(Debug, Default)]
pub struct PressedKeys(BTreeSet<u8>);

impl PressedKeys {
    /// Mark `usage` pressed. Oldest key is dropped when the slate is full.
    pub fn press(&mut self, usage: u8) -> bool {
        if usage == 0 {
            return false;
        }
        if self.0.len() < 6 {
            self.0.insert(usage);
            true
        } else {
            // Boot protocol caps at 6 keys; prefer to drop the lowest (oldest-looking).
            if let Some(&drop) = self.0.iter().next() {
                self.0.remove(&drop);
                self.0.insert(usage);
            }
            true
        }
    }
    /// Remove a key usage.
    pub fn release(&mut self, usage: u8) {
        self.0.remove(&usage);
    }
    /// Current set, iterated in sorted order.
    pub fn iter(&self) -> impl Iterator<Item = u8> + '_ {
        self.0.iter().copied()
    }
    /// Reset all state.
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

/// Build the 8-byte HID keyboard report.
pub fn build_report(mods: Modifiers, pressed: &PressedKeys) -> [u8; 8] {
    let mut out = [0u8; 8];
    out[0] = mods.0;
    for (i, u) in pressed.iter().take(6).enumerate() {
        out[2 + i] = u;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_bits_match_hid_spec() {
        assert_eq!(Modifier::ControlLeft.bit(), 0x01);
        assert_eq!(Modifier::MetaRight.bit(), 0x80);
    }

    #[test]
    fn report_has_modifier_and_keys() {
        let mut m = Modifiers::default();
        m.press(Modifier::ControlLeft);
        m.press(Modifier::AltLeft);
        let mut k = PressedKeys::default();
        k.press(0x4C); // Delete
        let r = build_report(m, &k);
        assert_eq!(r[0], 0x05);
        assert_eq!(r[1], 0);
        assert_eq!(r[2], 0x4C);
        assert_eq!(&r[3..], &[0u8; 5]);
    }

    #[test]
    fn pressed_keys_capped_at_six() {
        let mut k = PressedKeys::default();
        for u in 0x04..=0x0B {
            k.press(u);
        }
        assert_eq!(k.iter().count(), 6);
    }
}
