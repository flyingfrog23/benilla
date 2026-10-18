use benilla_ui::script::{AuraState, UiScript, UnitGuids};

#[test]
fn automatic_overflow_ui_reaches_debuff_255_and_hides_after_removal() {
    let mut script = UiScript::new().unwrap();
    script.set_unit_guids(&UnitGuids {
        player: 1,
        target: 2,
        ..Default::default()
    });
    let auras: Vec<_> = (1..=255)
        .map(|id| AuraState {
            spell_id: id,
            icon: Some(format!("Interface\\Icons\\Spell_{id}")),
            count: 3,
            helpful: false,
            ..Default::default()
        })
        .collect();
    script.set_player_auras(auras.clone());
    script.set_unit_auras(2, Some(auras));
    script
        .eval::<()>(
            "UIParent = CreateFrame('Frame', 'UIParent'); CreateFont('GameFontNormalSmall'):SetFont('Fonts/FRIZQT__.TTF', 10)",
        )
        .unwrap();
    let xml = include_str!("../../benilla-app/assets/ui/FrameXMLFixes.xml");
    let lua = xml
        .split("<![CDATA[")
        .nth(1)
        .unwrap()
        .split("]]>")
        .next()
        .unwrap();
    script.eval::<()>(lua).unwrap();
    // Legacy servers never call the negotiated hook; loading the layer adds no panels.
    assert!(script
        .eval::<bool>("return BenillaExtendedAuras_player == nil")
        .unwrap());
    script.eval::<()>("BenillaExtendedAurasUpdate()").unwrap();
    for unit in ["player", "target"] {
        for _ in 0..6 {
            script
                .eval::<()>(&format!("BenillaExtendedAuras_{unit}_Next:Click()"))
                .unwrap();
        }
        let last_button = if unit == "player" { 31 } else { 23 };
        let next_button = last_button + 1;
        let (page, index, icon, shown, hidden): (u32, u32, String, bool, bool) = script.eval(&format!(
            "return BenillaExtendedAuras_{unit}.page, BenillaExtendedAuras_{unit}_{last_button}.index, \
             BenillaExtendedAuras_{unit}_{last_button}.icon:GetTexture(), BenillaExtendedAuras_{unit}_{last_button}:IsShown(), \
             BenillaExtendedAuras_{unit}_{next_button}:IsShown()"
        )).unwrap();
        assert_eq!((page, index, shown, hidden), (7, 255, true, false));
        assert_eq!(icon, "Interface\\Icons\\Spell_255");
    }
    script.set_unit_auras(2, Some(vec![]));
    // A server removal repaints the native extension, without an addon event or user action.
    script.eval::<()>("BenillaExtendedAurasUpdate()").unwrap();
    assert!(script
        .eval::<bool>("return not BenillaExtendedAuras_target:IsShown()")
        .unwrap());
}
