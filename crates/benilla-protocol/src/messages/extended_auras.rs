//! Deviation: an opt-in fork protocol beyond the reference's 48 fields (UnitDebuff 0x5198f0).
//! Vanilla descriptors and addon records stay byte-for-byte unchanged when disabled.

use super::{ChatMessage, SecureAddon, UnitAuraSlot, CHAT_MSG_SYSTEM, LANGUAGE_ADDON};

pub const EXTENDED_AURA_SLOTS: u16 = 32 + 255;
pub const EXTENDED_AURA_ADDON: SecureAddon<'static> = SecureAddon {
    name: "Benilla_ExtendedAuras_v1",
    flags: 1,
    modulus_crc: 0x4245_4131,
    url_crc: 0,
};

/// Deviation: this fork advertises support automatically; only the server enables it.
/// BENILLA_EXTENDED_AURAS=0 forces legacy negotiation, outside the stock CVar table.
pub fn enabled() -> bool {
    std::env::var("BENILLA_EXTENDED_AURAS").as_deref() != Ok("0")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtendedAura {
    pub aura: UnitAuraSlot,
    /// -1 is permanent; other values are remaining milliseconds.
    pub remaining_ms: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuraSnapshot {
    pub guid: u64,
    pub auras: Vec<ExtendedAura>,
}

/// Only the reserved server SYSTEM/ADDON envelope can carry snapshots. Player addon chat has
/// another type and a nonzero sender. Malformed reserved messages are consumed, never displayed.
pub fn is_snapshot(m: &ChatMessage) -> bool {
    m.chat_type == CHAT_MSG_SYSTEM
        && m.language == LANGUAGE_ADDON
        && m.sender_guid == 0
        && m.text.starts_with("BENAURA1\t")
}

pub fn parse_snapshot(m: &ChatMessage) -> Option<AuraSnapshot> {
    if !is_snapshot(m) || m.text.len() > 24_576 {
        return None;
    }
    let mut header = m.text.splitn(4, '\t');
    header.next()?;
    let guid: u64 = header.next()?.parse().ok()?;
    let count: usize = header.next()?.parse().ok()?;
    if guid == 0 || count > usize::from(EXTENDED_AURA_SLOTS) {
        return None;
    }
    let body = header.next()?;
    let mut seen = [false; EXTENDED_AURA_SLOTS as usize];
    let mut auras = Vec::with_capacity(crate::wire::capacity_hint(
        count,
        EXTENDED_AURA_SLOTS as usize,
    ));
    for record in body.split_terminator(';') {
        if auras.len() >= count || !body.ends_with(';') {
            return None;
        }
        let mut fields = record.split(',');
        let slot: u16 = fields.next()?.parse().ok()?;
        let spell_id: u32 = fields.next()?.parse().ok()?;
        let flags: u8 = fields.next()?.parse().ok()?;
        let level: u8 = fields.next()?.parse().ok()?;
        let stacks: u8 = fields.next()?.parse().ok()?;
        let remaining_ms: i32 = fields.next()?.parse().ok()?;
        let caster_guid: u64 = fields.next()?.parse().ok()?;
        if slot >= EXTENDED_AURA_SLOTS
            || seen[usize::from(slot)]
            || spell_id == 0
            || flags & 0x0e == 0
            || flags & !0x0f != 0
            || (slot >= 32 && flags & 1 != 0)
            || stacks == 0
            || remaining_ms < -1
            || fields.next().is_some()
        {
            return None;
        }
        seen[usize::from(slot)] = true;
        auras.push(ExtendedAura {
            aura: UnitAuraSlot {
                slot,
                caster_guid,
                spell_id,
                flags,
                level,
                stacks,
            },
            remaining_ms,
        });
    }
    if auras.len() != count {
        return None;
    }
    auras.sort_unstable_by_key(|a| a.aura.slot);
    Some(AuraSnapshot { guid, auras })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(text: String) -> ChatMessage {
        ChatMessage {
            chat_type: CHAT_MSG_SYSTEM,
            language: LANGUAGE_ADDON,
            sender_guid: 0,
            target_guid: 0,
            sender_name: None,
            channel: None,
            text,
            chat_tag: 0,
        }
    }

    #[test]
    fn full_255_debuffs_use_u16_slots_and_keep_buff_class_separate() {
        let records = (0..EXTENDED_AURA_SLOTS)
            .rev()
            .map(|slot| format!("{slot},{},8,60,255,12000,7;", 1000 + slot))
            .collect::<String>();
        let snapshot = parse_snapshot(&message(format!("BENAURA1\t42\t287\t{records}"))).unwrap();
        assert_eq!(
            snapshot
                .auras
                .iter()
                .filter(|a| !a.aura.is_helpful())
                .count(),
            255
        );
        assert_eq!(snapshot.auras.last().unwrap().aura.slot, 286);
        assert_eq!(snapshot.auras.last().unwrap().aura.stacks, 255);
    }

    #[test]
    fn rejects_spoofs_corruption_and_excess_allocation() {
        let valid = "BENAURA1\t42\t1\t286,123,8,60,1,12000,7;";
        let mut m = message(valid.into());
        assert!(parse_snapshot(&m).is_some());
        m.sender_guid = 42;
        assert!(!is_snapshot(&m));
        m.sender_guid = 0;
        m.chat_type = 1;
        assert!(!is_snapshot(&m));
        for text in [
            "BENAURA1\t0\t0\t",
            "BENAURA1\t42\t999999999\t",
            "BENAURA1\t42\t0\tx",
            "BENAURA1\t42\t1\t",
            "BENAURA1\t42\t1\t287,123,8,60,1,12000,7;",
            "BENAURA1\t42\t2\t32,123,8,60,1,0,7;32,456,8,60,1,0,7;",
            "BENAURA1\t42\t1\t32,123,9,60,1,0,7;",
            "BENAURA1\t42\t1\t32,123,0,60,1,0,7;",
            "BENAURA1\t42\t1\t32,0,8,60,1,0,7;",
            "BENAURA1\t42\t1\t32,123,8,60,0,0,7;",
            "BENAURA1\t42\t1\t32,123,8,60,1,-2,7;",
            "BENAURA1\t42\t1\t32,123,8,60,1,0",
        ] {
            assert!(parse_snapshot(&message(text.into())).is_none(), "{text}");
        }
    }

    #[test]
    fn empty_snapshot_is_an_authoritative_clear() {
        assert!(parse_snapshot(&message("BENAURA1\t42\t0\t".into()))
            .unwrap()
            .auras
            .is_empty());
    }

    #[test]
    fn own_buffs_and_debuffs_lead_without_hiding_other_casters_or_overflow_spells() {
        let mut fields = super::super::ObjectFields::default();
        let snapshot = parse_snapshot(&message(
            "BENAURA1\t42\t4\t0,1126,9,60,1,-1,8;1,1459,9,60,1,-1,7;32,348,8,60,1,12000,8;286,348,8,60,1,12000,7;".into()
        )).unwrap();
        fields.set_extended_auras(snapshot.auras.iter().map(|a| a.aura).collect());
        let own_first: Vec<_> = fields.unit_auras_prioritizing(7).map(|a| a.slot).collect();
        assert_eq!(own_first, [1, 286, 0, 32]);
        assert_eq!(
            fields
                .unit_auras_prioritizing(7)
                .filter(|a| a.is_helpful())
                .map(|a| a.spell_id)
                .collect::<Vec<_>>(),
            [1459, 1126]
        );
        assert_eq!(
            fields
                .unit_auras_prioritizing(7)
                .filter(|a| !a.is_helpful())
                .map(|a| a.caster_guid)
                .collect::<Vec<_>>(),
            [7, 8]
        );
        // Gameplay lookups retain both casters' Immolates and every overflow spell.
        assert_eq!(fields.unit_aura_ids().filter(|id| *id == 348).count(), 2);
        assert_eq!(
            fields
                .unit_auras_prioritizing(0)
                .map(|a| a.slot)
                .collect::<Vec<_>>(),
            [0, 1, 32, 286]
        );
        fields.set_extended_auras(vec![]);
        assert_eq!(fields.unit_aura_ids().count(), 0);
    }
}
