import { useSnapshot } from "../useSnapshot";
import { AnvilLabels, CardPanels, RarityList } from "./overlay/AugmentPanels";
import { ChampSelectBlocks } from "./overlay/ChampSelectBlocks";

/**
 * The overlay window covers its host's client area exactly, so positions can be CSS percentages —
 * no DPI arithmetic — and `vh` units scale text with the host window, which is the unit the card
 * layout itself is measured in.
 *
 * The same window serves two hosts: the League client during champ select, the game during a match.
 * **The backend decides which it is glued to, and this draws for that host and no other.** The view
 * used to choose for itself from `champSelect.active`, which could disagree with the backend during
 * the overlap at game start: the window was already on the game while this was still drawing
 * champ-select blocks, laid out for the client, over it.
 */
export function Overlay() {
  const s = useSnapshot();
  if (!s) return null;
  switch (s.overlay.target) {
    case "client":
      return (
        <div className="overlay-root">
          <ChampSelectBlocks s={s} />
        </div>
      );
    case "game":
      // Not gated on `s.game`: the Live Client API only answers near the end of the loading
      // screen, and the overlay has to be up before that.
      return (
        <div className="overlay-root">
          {s.anvil.onScreen ? <AnvilLabels s={s} /> : <CardPanels s={s} />}
          {s.options.showAugmentList && <RarityList s={s} />}
        </div>
      );
    default:
      return null;
  }
}
