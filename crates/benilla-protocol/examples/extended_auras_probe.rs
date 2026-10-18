//! Local-only non-combat negotiation/lifecycle probe. Uses a dedicated PROBE account and
//! explicitly supplied harmless test aura IDs; never runs on a remote or player account.
//! `... -- fixture <file>` also checks the C++ allocation test's maximum-size wire output.

use std::time::{Duration, Instant};

use anyhow::{ensure, Context, Result};
use benilla_protocol::{
    decode, logon,
    messages::{extended_auras, ChatMessage, CHAT_MSG_SYSTEM, LANGUAGE_ADDON},
    CharCreateReq, ObjectFields, SessionEvent, WorldSession,
};

fn drain(
    session: &mut WorldSession,
    guid: u64,
    fields: &mut ObjectFields,
    latest: &mut Option<extended_auras::AuraSnapshot>,
    snapshots: &mut usize,
    seconds: u64,
) {
    let until = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < until {
        let packet = match session.recv() {
            Ok(packet) => packet,
            Err(error) => {
                if std::env::var_os("WOW_AURA_PROBE_TRACE").is_some() {
                    eprintln!("probe read: {error}");
                }
                continue;
            }
        };
        if std::env::var_os("WOW_AURA_PROBE_TRACE").is_some() {
            eprintln!("probe in: {}", packet.name());
        }
        for event in decode(packet) {
            match event {
                SessionEvent::ObjectCreate {
                    guid: g, fields: f, ..
                } if g == guid => *fields = f,
                SessionEvent::ObjectValues { guid: g, fields: f } if g == guid => fields.merge(f),
                SessionEvent::ExtendedAuras(s) if s.guid == guid => {
                    *snapshots += 1;
                    *latest = Some(s);
                }
                _ => {}
            }
        }
    }
}

// Applies the proc's harmless cast-time modifier directly; does not cast a damaging spell.
fn verify_shadow_trance(session: &mut WorldSession, guid: u64) -> Result<()> {
    session.send_chat(".aura 17941 30")?;
    let until = Instant::now() + Duration::from_secs(5);
    let (mut modifier, mut aura) = (false, false);
    while Instant::now() < until {
        if let Ok(packet) = session.recv() {
            for event in decode(packet) {
                match event {
                    SessionEvent::SpellModifier {
                        flat: false,
                        mask_bit: 0,
                        op: 10,
                        value: -100,
                    } => modifier = true,
                    SessionEvent::ExtendedAuras(snapshot) if snapshot.guid == guid => {
                        aura |= snapshot
                            .auras
                            .iter()
                            .any(|a| a.aura.spell_id == 17941 && a.aura.caster_guid == guid);
                    }
                    _ => {}
                }
            }
        }
    }
    session.send_chat(".unaura 17941")?;
    let until = Instant::now() + Duration::from_secs(3);
    let mut reset = false;
    while Instant::now() < until {
        if let Ok(packet) = session.recv() {
            for event in decode(packet) {
                if let SessionEvent::SpellModifier {
                    flat: false,
                    mask_bit: 0,
                    op: 10,
                    value: 0,
                } = event
                {
                    reset = true;
                }
            }
        }
    }
    ensure!(
        modifier && aura && reset,
        "Shadow Trance modifier/aura/reset missing: {modifier}/{aura}/{reset}"
    );
    println!(
        "PASS: Shadow Trance -100% Shadow Bolt cast-time modifier, caster ownership, and modifier removal"
    );
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mode = args
        .get(1)
        .context("mode: fixture <path>, legacy, or extended")?;
    if mode == "fixture" {
        let text = std::fs::read_to_string(args.get(2).context("fixture path")?)?;
        let m = ChatMessage {
            chat_type: CHAT_MSG_SYSTEM,
            language: LANGUAGE_ADDON,
            sender_guid: 0,
            target_guid: 0,
            sender_name: None,
            channel: None,
            text: text.trim_end_matches('\n').into(),
            chat_tag: 0,
        };
        let snapshot = extended_auras::parse_snapshot(&m).context("C++ fixture rejected")?;
        ensure!(snapshot.auras.len() == 255 && snapshot.auras.last().unwrap().aura.slot == 286);
        println!("PASS: C++ 255-debuff fixture decoded by the production Rust parser");
        return Ok(());
    }
    ensure!(mode == "legacy" || mode == "extended", "unknown mode");
    ensure!(
        std::env::var("WOW_UNATTENDED").as_deref() == Ok("1"),
        "set WOW_UNATTENDED=1"
    );
    let user = std::env::var("WOW_USER")?;
    let pass = std::env::var("WOW_PASS")?;
    ensure!(
        user.to_ascii_uppercase().starts_with("PROBE"),
        "dedicated PROBE account required"
    );
    let name = std::env::var("WOW_CHAR")?;
    let logon = logon(
        "127.0.0.1",
        &user,
        &pass,
        benilla_protocol::auth::LOCALE_TAG_EN_US,
    )?;
    let mut session = WorldSession::connect("127.0.0.1:8085", &user, logon.session_key)?;
    let mut roster = session.char_enum()?;
    if !roster.iter().any(|c| c.name == name) {
        session.create_character(&CharCreateReq {
            name: name.clone(),
            race: 1,
            class: 1,
            gender: 0,
            skin: 0,
            face: 0,
            hair_style: 0,
            hair_color: 0,
            facial_hair: 0,
        })?;
        roster = session.char_enum()?;
    }
    let guid = roster
        .iter()
        .find(|c| c.name == name)
        .context("probe character missing")?
        .guid;
    session.player_login(guid)?;
    session.set_active_mover(guid)?;
    session.set_read_timeout(Some(Duration::from_millis(500)))?;
    let mut fields = ObjectFields::default();
    let mut latest = None;
    let mut snapshots = 0;
    for _ in 0..6 {
        drain(
            &mut session,
            guid,
            &mut fields,
            &mut latest,
            &mut snapshots,
            5,
        );
        if fields.created_as().is_some() {
            break;
        }
    }
    ensure!(
        fields.created_as().is_some(),
        "self CREATE missing; login did not complete"
    );
    session.send_chat(".gm off")?;
    let ids: Vec<u32> = std::env::var("WOW_AURA_PROBE_IDS")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()?;
    ensure!(
        ids.len() <= 256 && ids.iter().all(|id| (60000..=60255).contains(id)),
        "only dedicated local dummy aura IDs 60000..60255 are permitted"
    );
    for id in &ids {
        session.send_chat(&format!(".aura {id} 120"))?;
    }
    drain(
        &mut session,
        guid,
        &mut fields,
        &mut latest,
        &mut snapshots,
        8,
    );
    if mode == "extended" {
        let snapshot = latest.context("no negotiated snapshot")?;
        let negative: Vec<_> = snapshot
            .auras
            .iter()
            .filter(|a| !a.aura.is_helpful())
            .collect();
        ensure!(
            negative.len() == ids.len().min(255),
            "got {} debuffs for {} test IDs",
            negative.len(),
            ids.len()
        );
        if ids.len() >= 255 {
            ensure!(
                negative.iter().any(|a| a.aura.slot == 286),
                "last slot missing"
            );
        }
        println!(
            "PASS: {snapshots} negotiated snapshots; {} debuffs; {} legacy aura fields",
            negative.len(),
            fields.unit_auras().count()
        );
    } else {
        ensure!(snapshots == 0, "extension sent to a legacy session/server");
        println!(
            "PASS: legacy login/updates, zero extension snapshots; {} aura fields",
            fields.unit_auras().count()
        );
    }
    if mode == "extended" && std::env::var("WOW_SHADOW_TRANCE_PROBE").as_deref() == Ok("1") {
        verify_shadow_trance(&mut session, guid)?;
    }
    // Remove only the explicitly supplied, temporary test auras. No damage/combat is involved.
    for id in &ids {
        session.send_chat(&format!(".unaura {id}"))?;
    }
    let until = Instant::now() + Duration::from_secs(3);
    let mut cleared = false;
    while Instant::now() < until {
        if let Ok(packet) = session.recv() {
            for event in decode(packet) {
                if let SessionEvent::ExtendedAuras(s) = event {
                    if s.guid == guid && s.auras.iter().all(|a| !ids.contains(&a.aura.spell_id)) {
                        cleared = true;
                    }
                }
            }
        }
    }
    if mode == "extended" && !ids.is_empty() {
        ensure!(cleared, "removed auras remained in snapshots");
    }
    session.logout(Duration::from_secs(25))?;
    println!("PASS: aura cleanup and logout");
    Ok(())
}
