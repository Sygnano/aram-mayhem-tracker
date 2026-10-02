//! The whole stat anvil pipeline on real screenshots: the augment detectors find the shard
//! cards, the bundled OCR reads names and value lines, and the tier and shards come out right.
//!
//! The catalogue is the service's `/v1/anvils?locale=en_us` on 16.19, saved as a fixture.

use mayhem_core::anvils::{AnvilTier, ShardInfo};
use mayhem_core::geometry::PixelRect;
use mayhem_core::layout::AugmentLayout;
use mayhem_vision::anvil::{is_anvil_offer, read_anvil, ShardMatcher};
use mayhem_vision::capture::StillCapture;
use mayhem_vision::matcher::{AugmentMatcher, MatcherConfig};
use mayhem_vision::offer::{read_offer, scan_cards, CardDetectors};
use mayhem_vision::paddle::PaddleRecognizer;

#[derive(serde::Deserialize)]
struct Catalogue {
    shards: Vec<ShardInfo>,
}

fn catalogue() -> Vec<ShardInfo> {
    let raw = include_str!("fixtures/anvils.16.19.en_us.json");
    serde_json::from_str::<Catalogue>(raw).unwrap().shards
}

struct Case {
    name: &'static str,
    png: &'static [u8],
    tier: AnvilTier,
    shards: [&'static str; 3],
    /// The prismatic capture has a VS Code window over the "hide augments" button, so only the
    /// worker's periodic card sweep would find it. Every other screenshot is found by the button.
    needs_sweep: bool,
}

const GOLD_SET: [&str; 3] = ["ARAM_GoldStatAnvil_MPen", "ARAM_GoldStatAnvil_MR", "ARAM_GoldStatAnvil_Hybrid2"];

const CASES: [Case; 7] = [
    Case {
        name: "1280x1024 gold",
        png: include_bytes!("fixtures/screens/anvil_1280x1024_gold.png"),
        tier: AnvilTier::Gold,
        shards: GOLD_SET,
        needs_sweep: false,
    },
    Case {
        name: "1440x900 gold",
        png: include_bytes!("fixtures/screens/anvil_1440x900_gold.png"),
        tier: AnvilTier::Gold,
        shards: GOLD_SET,
        needs_sweep: false,
    },
    Case {
        name: "1680x1050 gold",
        png: include_bytes!("fixtures/screens/anvil_1680x1050_gold.png"),
        tier: AnvilTier::Gold,
        shards: GOLD_SET,
        needs_sweep: false,
    },
    Case {
        name: "1920x1080 gold",
        png: include_bytes!("fixtures/screens/anvil_1920x1080_gold.png"),
        tier: AnvilTier::Gold,
        shards: GOLD_SET,
        needs_sweep: false,
    },
    Case {
        name: "3440x1440 silver",
        png: include_bytes!("fixtures/screens/anvil_3440x1440_silver.png"),
        tier: AnvilTier::Silver,
        shards: ["ARAM_StatAnvil_AR", "ARAM_StatAnvil_HP", "ARAM_StatAnvil_AP"],
        needs_sweep: false,
    },
    Case {
        name: "3440x1440 gold",
        png: include_bytes!("fixtures/screens/anvil_3440x1440_gold.png"),
        tier: AnvilTier::Gold,
        shards: ["ARAM_GoldStatAnvil_Hybrid2", "ARAM_GoldStatAnvil_CritChance", "ARAM_GoldStatAnvil_MPen"],
        needs_sweep: false,
    },
    Case {
        name: "3440x1440 prismatic",
        png: include_bytes!("fixtures/screens/anvil_3440x1440_prismatic.png"),
        tier: AnvilTier::Prismatic,
        shards: ["ARAM_StatAnvil_HS", "ARAM_StatAnvil_MS", "ARAM_StatAnvil_Tenacity"],
        needs_sweep: true,
    },
];

#[test]
fn every_anvil_screenshot_reads_as_its_tier_and_shards() {
    let layout = AugmentLayout::default();
    let detectors = CardDetectors::bundled(Default::default(), Default::default(), Default::default()).unwrap();
    let ocr = PaddleRecognizer::bundled().unwrap();
    let catalogue = catalogue();
    let shards = ShardMatcher::new(MatcherConfig::default(), &catalogue);
    // No augment names: this test is about the shard path. The augment-vs-anvil vote is tested on
    // its own in `anvil.rs`.
    let augments = AugmentMatcher::new(MatcherConfig::default(), &[]);

    for case in &CASES {
        let frame = image::load_from_memory(case.png).unwrap().to_rgba8();
        let client = PixelRect::new(0, 0, frame.width(), frame.height());
        let mut capture = StillCapture { frame };

        let gated = scan_cards(&mut capture, &client, &layout, &detectors, false).unwrap();
        assert!(!gated.rerolls.iter().any(|r| r.present), "{}: anvil cards have no reroll buttons", case.name);
        assert_eq!(gated.button.present, !case.needs_sweep, "{}: button {}", case.name, gated.button.score);

        let scan = if case.needs_sweep {
            scan_cards(&mut capture, &client, &layout, &detectors, true).unwrap()
        } else {
            gated
        };
        assert!(scan.cards_on_screen(), "{}: the augment frame template should match shard cards", case.name);

        let titles = scan.titles.as_ref().expect("titles cropped");
        let values = scan.values.as_ref().expect("value lines cropped with the titles");
        let read = read_offer(&ocr, &augments, titles, layout.title_lines()).unwrap();
        let matches = shards.match_offer(&read.texts);
        assert!(is_anvil_offer(&read.matches, &matches), "{}: {:?}", case.name, read.texts);

        let anvil = read_anvil(&ocr, &catalogue, &read.texts, matches, values).unwrap();
        assert_eq!(anvil.decision.tier, Some(case.tier), "{}: {anvil:#?}", case.name);
        let got: Vec<Option<&str>> = anvil.decision.shards.iter().map(|s| s.as_deref()).collect();
        let want: Vec<Option<&str>> = case.shards.iter().map(|s| Some(*s)).collect();
        assert_eq!(got, want, "{}: {anvil:#?}", case.name);
    }
}
