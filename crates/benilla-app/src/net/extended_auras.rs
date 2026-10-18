//! Deviation: opt-in fork aura snapshots, independent of the stock descriptor.

use std::collections::HashMap;

use benilla_protocol::{messages::extended_auras::AuraSnapshot, SessionEvent, SessionEventKind};
use bevy::prelude::*;

use super::{GuidIndex, NetHandlerApp, ObjectStore, SelfGuid};
use crate::ui_aura::AuraDurations;

/// Keeps the latest snapshot across re-CREATEs; a server can send it before its batched CREATE.
#[derive(Resource, Default)]
pub(super) struct ExtendedAuras(pub(super) HashMap<u64, AuraSnapshot>);

impl ExtendedAuras {
    pub(super) fn apply(&self, guid: u64, fields: &mut benilla_protocol::ObjectFields) {
        if let Some(snapshot) = self.0.get(&guid) {
            fields.set_extended_auras(snapshot.auras.iter().map(|a| a.aura).collect());
        }
    }
}

pub(super) fn register(app: &mut App) {
    app.add_message::<crate::field_watch::FieldNotice>()
        .init_resource::<ExtendedAuras>()
        .net_handler(SessionEventKind::ExtendedAuras, on_snapshot)
        .net_handler(SessionEventKind::Disconnected, clear)
        .net_handler(SessionEventKind::LoggedOut, clear)
        .net_handler(SessionEventKind::Worldport, clear);
}

fn clear(In(event): In<SessionEvent>, mut snapshots: ResMut<ExtendedAuras>) {
    if matches!(
        event,
        SessionEvent::Disconnected { .. }
            | SessionEvent::LoggedOut
            | SessionEvent::Worldport { .. }
    ) {
        snapshots.0.clear();
    }
}

fn on_snapshot(
    In(event): In<SessionEvent>,
    mut snapshots: ResMut<ExtendedAuras>,
    index: Res<GuidIndex>,
    me: Res<SelfGuid>,
    mut stores: Query<&mut ObjectStore>,
    mut durations: Option<ResMut<AuraDurations>>,
    time: Res<Time<Real>>,
    mut notices: MessageWriter<crate::field_watch::FieldNotice>,
) {
    let SessionEvent::ExtendedAuras(snapshot) = event else {
        return;
    };
    // Bound snapshots that arrive before CREATE as well as visible objects. Never clear a held
    // object's authoritative list just because an unrelated pending snapshot reached the cap.
    if snapshots.0.len() >= 4096 && !snapshots.0.contains_key(&snapshot.guid) {
        return;
    }
    let changed = snapshots.0.get(&snapshot.guid).is_none_or(|previous| {
        !previous
            .auras
            .iter()
            .map(|a| a.aura)
            .eq(snapshot.auras.iter().map(|a| a.aura))
    });
    if changed {
        // Overflow changes have no legacy field edge. Deliver the ordinary addon event after
        // the aura feed has published the new list, just like a vanilla aura-field watch.
        notices.write(crate::field_watch::FieldNotice::Signal {
            guid: snapshot.guid,
            id: crate::field_watch::id::UNIT_AURA,
        });
    }
    if let Some(store) = index
        .0
        .get(&snapshot.guid)
        .and_then(|e| stores.get_mut(*e).ok())
    {
        store
            .into_inner()
            .0
            .set_extended_auras(snapshot.auras.iter().map(|a| a.aura).collect());
    }
    if me.0 == Some(snapshot.guid) {
        if let Some(ref mut durations) = durations {
            durations.extended = true;
            durations.clear_stamps();
            for a in &snapshot.auras {
                durations.set(a.aura.slot, a.remaining_ms, time.elapsed_secs_f64());
            }
        }
    }
    snapshots.0.insert(snapshot.guid, snapshot);
}

#[cfg(test)]
mod tests {
    use super::*;
    use benilla_protocol::{
        messages::extended_auras::ExtendedAura, messages::UnitAuraSlot, ObjectFields,
    };

    fn snapshot() -> AuraSnapshot {
        AuraSnapshot {
            guid: 42,
            auras: vec![ExtendedAura {
                aura: UnitAuraSlot {
                    slot: 286,
                    caster_guid: 42,
                    spell_id: 123,
                    flags: 8,
                    level: 60,
                    stacks: 3,
                },
                remaining_ms: 12000,
            }],
        }
    }

    #[test]
    fn snapshots_before_create_survive_recreate_and_clear_on_empty_or_session_end() {
        let mut app = App::new();
        app.init_resource::<GuidIndex>()
            .init_resource::<SelfGuid>()
            .init_resource::<Time<Real>>()
            .init_resource::<AuraDurations>();
        register(&mut app);
        let send =
            |app: &mut App, event| super::super::handlers::dispatch(app.world_mut(), vec![event]);
        send(&mut app, SessionEvent::ExtendedAuras(snapshot()));
        let mut fields = ObjectFields::default();
        app.world()
            .resource::<ExtendedAuras>()
            .apply(42, &mut fields);
        assert_eq!(fields.unit_auras().next().unwrap().slot, 286);
        let entity = app.world_mut().spawn(ObjectStore(fields)).id();
        app.world_mut()
            .resource_mut::<GuidIndex>()
            .0
            .insert(42, entity);
        // A refresh updates the existing object, and a subsequent re-CREATE sees that same list.
        let mut refreshed = snapshot();
        refreshed.auras[0].aura.stacks = 7;
        send(&mut app, SessionEvent::ExtendedAuras(refreshed));
        assert_eq!(
            app.world()
                .get::<ObjectStore>(entity)
                .unwrap()
                .0
                .unit_auras()
                .next()
                .unwrap()
                .stacks,
            7
        );
        let mut recreated = ObjectFields::default();
        app.world()
            .resource::<ExtendedAuras>()
            .apply(42, &mut recreated);
        assert_eq!(recreated.unit_auras().count(), 1);
        send(
            &mut app,
            SessionEvent::ExtendedAuras(AuraSnapshot {
                guid: 42,
                auras: vec![],
            }),
        );
        assert_eq!(
            app.world()
                .get::<ObjectStore>(entity)
                .unwrap()
                .0
                .unit_auras()
                .count(),
            0
        );
        send(&mut app, SessionEvent::LoggedOut);
        assert!(app.world().resource::<ExtendedAuras>().0.is_empty());
    }
}
