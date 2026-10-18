# Extended debuffs in this fork

This optional fork extension supports 255 visible debuffs plus 32 buffs per visible unit.
The reference has 48 fixed aura fields: 32 buffs and 16 debuffs (`UnitDebuff`, 0x5198f0;
benilla-re `systems/ui/auradisplay.md`). Raising that field count changes every following
unit and player field and breaks vanilla clients. This extension leaves the descriptor alone.

Build both this Benilla fork and the matching vmangos-core fork. In `mangosd.conf`, set:

```ini
DebuffLimit = 255
Benilla.ExtendedAuras = 1
```

Restart mangosd, then launch this Benilla fork. It advertises support automatically, and activates
extended lists only after a valid server snapshot. `BENILLA_EXTENDED_AURAS=0` forces legacy
negotiation for troubleshooting; there is no stock CVar change. The server switch defaults off.
`DebuffLimit` remains the server's gameplay setting and applies to all clients, regardless of
negotiation. `Benilla.ExtendedAuras` requires a restart; config reload preserves its old value.

The ordinary `UnitDebuff(unit, index)` API reaches index 255 on visible units. Your own casts
lead both buff and debuff lists, on yourself and other units. Other players' auras remain
present with stable ordering within each ownership group. Legacy servers supply no caster
identity in their aura fields, so they retain legacy ordering.

The built-in UI activates automatically after negotiation. When the player exceeds the stock
8 displayed debuffs, or the target exceeds 16, an overflow bar appears beside the corresponding
stock frame. It shows the remaining debuffs, with 36 icons per page and previous/next buttons.
The bar disappears when the list fits in the stock frame again. Hover for the normal tooltip.
No addon, installation step, or game file edit is needed. Third-party addons can also enumerate
the same `UnitDebuff` API and receive `UNIT_AURA` updates for overflow changes.

| Client | Server | Behavior |
| --- | --- | --- |
| Stock 1.12.1 or existing patched stock client | Extended server | Existing legacy fields and Wallcraft behavior; no extension messages |
| Benilla with extension off | Extended server | Existing legacy behavior |
| Benilla with extension on | Unmodified pre-Wallcraft server | Legacy fallback; no unknown client opcode |
| Benilla with extension on | Unmodified Wallcraft server | Legacy fallback, including its borrowed buff slots |
| Benilla with extension on | Extended server, either legacy allocation | Separate 32-buff/255-debuff list with correct polarity |

When backporting the server extension to a pre-Wallcraft tree, also replace its 40-debuff
`setConfigMinMax` clamp with `setConfig`, as in this server fork; retain that tree's legacy
slot allocation. The extension hooks do not depend on Wallcraft's slot borrowing.

An unmodified stock client cannot display 255 debuffs. An unmodified Wallcraft server supplies
no polarity marker for the NPC debuffs it places in buff slots; fallback preserves that wire
behavior, rather than guessing which uncancelable buff is a debuff. The extension's separate
slot ranges remove that ambiguity when both peers support it. Out-of-range party aura records
keep the stock format; the extension covers streamed, visible units.

## Version 1 wire contract

The client appends one ordinary secure-addon record to `CMSG_AUTH_SESSION`:
`Benilla_ExtendedAuras_v1`, flags 1, modulus CRC `0x42454131`, URL CRC 0. Older servers accept
it as a custom addon. No new opcode, update field, stock addon record, or client build is used.
The extended server sends snapshots only to sessions advertising this exact record, when its
setting is enabled and the client build is 5875.

A snapshot is an `SMSG_MESSAGECHAT` envelope with type `CHAT_MSG_SYSTEM` (10), language
`LANG_ADDON` (`0xffffffff`), sender GUID 0, and text:

```text
BENAURA1<TAB>decimal-unit-guid<TAB>record-count<TAB>slot,spell,flags,level,stacks,remaining-ms,caster-guid;...
```

Slots 0–31 are buffs; 32–286 are debuffs. Slots stay stable until that aura is removed, independently
of the legacy slot. Flags use the vanilla effect/cancelable nibble. Stacks are actual counts,
1–255; remaining milliseconds are signed, with -1 for a permanent aura. Caster GUID is decimal, 0 if unknown. Each snapshot replaces
the entire list, including an empty list. The client caps each message at 24 KiB. The parser rejects duplicate or out-of-range slots,
bad flags, inconsistent counts, excessive size, and non-server envelopes. Reserved malformed
messages never enter the chat UI. Other player addon messages are not snapshots.

Initial snapshots can precede a batched object CREATE and are retained until creation. Later
snapshots follow aura apply, removal, effect, stack/charge, and duration changes, coalesced per
unit update. Only viewers that already know the unit receive broadcasts; the ordinary buff
visibility gate still controls the Lua API. Destruction, out-of-range removal, worldport, and
session end discard the relevant cached snapshots. Vanilla duration packets remain unchanged;
after the player's first extended snapshot, its independent slot timers use snapshots instead. Overflow changes also issue the
ordinary `UNIT_AURA` event after publishing the new lists, so addons update without a legacy
field change.

255 is the version 1 bound, not unlimited. Higher `DebuffLimit` values can leave additional
server auras without extension slots. Keep it at 255 or lower for this feature. Buff limits,
debuff priority eviction, dispel logic, and cancellation rules remain the server's existing rules.

## Spell checks and procs

Display slots are separate from the server's aura holders and modifier lists. Conflagrate's
server script searches every periodic-damage aura and checks its caster before accepting the
cast and consuming Immolate; it does not search the 48 legacy fields. Nightfall's proc handler
also uses server aura effects and triggers Shadow Trance (17941). The client does not authorize
that proc by acknowledging a damage display. Normal damage packets, aura-state fields, and
spell-modifier packets remain unchanged.

Benilla's aura-ID lookups include the extension, even beyond legacy slot 47. Presentation
ordering and hidden-buff rules do not remove entries from that lookup. Shadow Trance's casting
time modifier still arrives through `SMSG_SET_PCT_SPELL_MODIFIER`, independently of its icon.
The local, non-combat probe can verify that modifier's application and removal while 255 dummy
debuffs are active. A full damage/proc roll and Conflagrate consumption still need a combat
playtest; the repository forbids unattended combat probes.

## Checks

Run `cargo test -p benilla-protocol`, `cargo test -p benilla-ui --test extended_auras`, and the
Benilla app tests for `net::extended_auras` and `ui_aura`. The built-in UI test pages through
255 debuffs on player and target, checks the final icon and empty slots, then removes the list.
The server's independent allocation/encoding test is built with:

```sh
c++ -std=c++14 -Wall -Wextra -Werror -I src/game/Spells tests/extended_auras.cpp -o /tmp/extended-auras-test
/tmp/extended-auras-test
```

Graphical presentation and a stock Windows client's login still require their respective
runtime checks; parser and UI-engine tests do not establish those results.
