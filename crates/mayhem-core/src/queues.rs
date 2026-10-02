//! Queue identification: match against a *set* of ids, and fall back to the queue name.

/// ARAM: Mayhem queue ids from CommunityDragon `queues.json`, verified 2026-09-29.
pub const MAYHEM_QUEUE_IDS: [i64; 9] = [2400, 2401, 2403, 2405, 2410, 2450, 3240, 3270, 3280];

/// Classic ARAM queue ids: Howling Abyss, and the Butcher's Bridge reskin that shares its select.
pub const ARAM_QUEUE_IDS: [i64; 4] = [65, 100, 450, 720];

/// Internal mode names. `KIWI` is ARAM: Mayhem; `KIWI_JADE` its Classic-ish variant.
pub const MAYHEM_MODE_NAMES: [&str; 2] = ["KIWI", "KIWI_JADE"];

/// True when the queue is an ARAM: Mayhem queue. `queue_name` is the human-readable name from the
/// LCU or `queues.json`, if known; it lets new variant ids through without a code change.
pub fn is_mayhem_queue(queue_id: i64, queue_name: Option<&str>) -> bool {
    MAYHEM_QUEUE_IDS.contains(&queue_id) || queue_name.is_some_and(|name| name.to_ascii_lowercase().contains("mayhem"))
}

/// True when the queue uses the *ARAM* champ select: five allies who already have a champion, a
/// bench to swap from, and a reroll button. Every Mayhem variant does, and so does classic ARAM.
///
/// This is deliberately wider than [`is_mayhem_queue`]. The champ-select overlay only needs the
/// screen to have that shape; gating it on Mayhem alone would leave it dark in plain ARAM, where the
/// layout it reads is identical.
pub fn has_aram_champ_select(queue_id: i64, queue_name: Option<&str>) -> bool {
    ARAM_QUEUE_IDS.contains(&queue_id)
        || is_mayhem_queue(queue_id, queue_name)
        || queue_name.is_some_and(|name| name.to_ascii_lowercase().contains("aram"))
}

/// True when a Live Client Data `gameMode` string identifies Mayhem. The exact in-game value is
/// unverified (research check B6), so both known codenames are accepted.
pub fn is_mayhem_game_mode(game_mode: &str) -> bool {
    MAYHEM_MODE_NAMES.iter().any(|m| m.eq_ignore_ascii_case(game_mode))
}

/// Which augment pool applies to a queue.
pub fn augment_pool_for_queue(queue_id: i64, queue_name: Option<&str>) -> Option<&'static str> {
    let classic_ish =
        matches!(queue_id, 2450 | 3280) || queue_name.is_some_and(|n| n.to_ascii_lowercase().contains("classic-ish"));
    if classic_ish {
        Some("KIWI_JADE")
    } else if is_mayhem_queue(queue_id, queue_name) {
        Some("KIWI")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_known_ids_and_names() {
        assert!(is_mayhem_queue(2400, None));
        assert!(is_mayhem_queue(9999, Some("ARAM: Mayhem")));
        assert!(!is_mayhem_queue(450, Some("ARAM")));
        assert!(is_mayhem_game_mode("KIWI"));
        assert!(!is_mayhem_game_mode("ARAM"));
    }

    #[test]
    fn the_aram_champ_select_covers_more_than_mayhem() {
        assert!(has_aram_champ_select(450, None), "classic ARAM has a bench too");
        assert!(has_aram_champ_select(2400, None), "and so does every Mayhem variant");
        assert!(has_aram_champ_select(9999, Some("ARAM: Mayhem")));
        assert!(has_aram_champ_select(9999, Some("ARAM")));
        assert!(!has_aram_champ_select(420, Some("Ranked Solo")), "draft has no bench");
        assert!(!has_aram_champ_select(1700, Some("Arena")));
    }

    #[test]
    fn picks_the_right_pool() {
        assert_eq!(augment_pool_for_queue(2400, None), Some("KIWI"));
        assert_eq!(augment_pool_for_queue(3280, None), Some("KIWI_JADE"));
        assert_eq!(augment_pool_for_queue(1, Some("ARAM: Mayhem Classic-ish")), Some("KIWI_JADE"));
        assert_eq!(augment_pool_for_queue(450, None), None);
    }
}
