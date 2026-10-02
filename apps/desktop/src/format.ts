const RARITY: Record<string, { label: string; color: string }> = {
  silver: { label: "Silver", color: "#b8c4d0" },
  gold: { label: "Gold", color: "#e2b94a" },
  prismatic: { label: "Prismatic", color: "#c58cf5" },
  eventchoice: { label: "Event", color: "#6fd0c9" },
};

/**
 * How a rarity is shown. Takes either spelling: CommunityDragon's (`kGold`, on a card read off the
 * screen) or the service's (`gold`, on a pool), so no caller has to rebuild one from the other.
 */
export function rarity(r: string) {
  return RARITY[r.replace(/^k(?=[A-Z])/, "").toLowerCase()] ?? { label: r || "?", color: "#9aa4b2" };
}
