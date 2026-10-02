//! The vision worker: finds the augment cards on screen and reads their titles.
//!
//! Every tick during a Mayhem game it looks for the in-game "hide augments" button
//! (`mayhem_vision::button`), which is on screen exactly while augments can be picked. Only then
//! does it grab the card area and check the card frames (`mayhem_vision::frames`); only when a
//! frame is there does it crop the titles; and only when those changed does it run OCR. So the
//! screen is never OCR-ed during normal play, and nothing depends on guessing *when* an offer is
//! shown: the cards can be hidden and re-shown with the in-game button, or kept open while alive,
//! and the worker simply follows what is on screen.
//!
//! A tick costs well under a millisecond outside an offer — one small capture and one correlation —
//! and about 100 ms when an offer is on screen and its titles have changed, so the tick rate rather
//! than the work decides how quickly a rerolled card is picked up.
//!
//! Generic over [`ScreenCapture`] and [`OcrEngine`], so the whole pipeline is exercised in tests
//! with real screenshots. On Windows it runs with GDI capture and the bundled PaddleOCR model on
//! its own thread, so captures and OCR never block the async runtime.

// Off Windows the worker is only driven by tests.
#![cfg_attr(not(windows), allow(dead_code))]

use std::sync::Arc;
use std::time::{Duration, Instant};

use mayhem_core::anvils::ShardInfo;
use mayhem_core::augments::{OfferReading, OFFER_SLOTS};
use mayhem_core::clock::GameClock;
use mayhem_core::layout::{AugmentLayout, TitleLines};
use mayhem_vision::anvil::{is_anvil_offer, read_anvil, AnvilRead, ShardMatcher};
use mayhem_vision::button::ButtonDetectorConfig;
use mayhem_vision::capture::ScreenCapture;
use mayhem_vision::frames::FrameDetectorConfig;
use mayhem_vision::matcher::AugmentMatcher;
use mayhem_vision::ocr::OcrEngine;
use mayhem_vision::offer::{read_offer, scan_cards, CardDetectors, Fingerprint, OfferRead};
use mayhem_vision::reroll::RerollDetectorConfig;
use mayhem_vision::VisionError;

use super::snapshot::{ButtonView, CardFrameView, OcrDebug, RerollView};
use super::{Engine, EngineState};

/// Re-OCR an unchanged card region at most this often. This is only a safety net: a reroll is
/// normally caught by the fingerprint on the very next tick, so it does not bound how fast a
/// rerolled card refreshes.
const OCR_REFRESH: Duration = Duration::from_secs(2);
/// How often to scan the card area even though neither the rerolls nor the hide button were
/// found.
///
/// The cheap detectors gate everything, so if a patch restyled both the app would quietly stop
/// seeing offers. This sweep makes that failure visible instead: the card frames are found, the
/// buttons are not, and the settings screen says so. It costs one card scan every two seconds, under
/// one percent of the worker's budget.
const BUTTONLESS_SWEEP: Duration = Duration::from_secs(2);
/// Per-pixel difference (0..255) of the most-changed card above which the titles count as changed.
const FINGERPRINT_CHANGE: f32 = 4.0;

/// The detector settings a built `CardDetectors` belongs to, so a settings change rebuilds it.
type DetectorConfigs = (FrameDetectorConfig, ButtonDetectorConfig, RerollDetectorConfig);

/// Everything one tick needs, copied out under the lock.
struct VisionJob {
    session_id: u64,
    clock: GameClock,
    layout: AugmentLayout,
    frame_cfg: FrameDetectorConfig,
    button_cfg: ButtonDetectorConfig,
    reroll_cfg: RerollDetectorConfig,
    matcher: Option<Arc<AugmentMatcher>>,
    /// Shard names and the catalogue, once fetched. Without them an anvil offer reads as an
    /// unreadable augment offer, which is what it did before anvils were read.
    anvils: Option<(Arc<ShardMatcher>, Arc<Vec<ShardInfo>>)>,
}

impl VisionJob {
    /// Only during a Mayhem game: nothing else has augments.
    fn from_state(st: &EngineState) -> Option<Self> {
        let game = st.game.as_ref().filter(|g| g.is_mayhem)?;
        Some(Self {
            session_id: game.id,
            clock: game.clock,
            layout: st.tuning.augment_layout,
            frame_cfg: st.tuning.frame_detector,
            button_cfg: st.tuning.button_detector,
            reroll_cfg: st.tuning.reroll_detector,
            matcher: st.matcher.clone(),
            anvils: st.anvils.matcher.clone().map(|m| (m, st.anvils.catalogue.clone())),
        })
    }
}

/// Builds the OCR engine on first use (loading the model takes a moment), on the worker thread.
type OcrFactory<O> = Box<dyn Fn() -> Result<O, VisionError> + Send>;

pub struct VisionWorker<C: ScreenCapture, O: OcrEngine> {
    capture: C,
    ocr_factory: OcrFactory<O>,
    ocr: Option<Result<O, String>>,
    detectors: Option<(DetectorConfigs, CardDetectors)>,
    /// The game session the cached OCR result belongs to.
    cache_key: Option<u64>,
    last_fingerprint: Option<Fingerprint>,
    last_read: Option<(Instant, OfferRead)>,
    /// The last stat anvil reading. At most one of this and `last_read` is set: the cards on
    /// screen are either augments or shards.
    last_anvil: Option<(Instant, AnvilRead)>,
    /// When the card area was last scanned without a button to justify it.
    last_sweep: Option<Instant>,
    rate: f64,
    last_tick: Option<Instant>,
}

/// What one tick produced, applied to the state under the lock.
#[derive(Default)]
struct TickOutput {
    at: Option<f64>,
    client_size: Option<(u32, u32)>,
    game_window_found: bool,
    cards: Vec<CardFrameView>,
    cards_on_screen: bool,
    rerolls: Vec<RerollView>,
    button: ButtonView,
    offer: Option<OfferReading>,
    /// The anvil offer on screen, if that is what the cards are.
    anvil: Option<AnvilRead>,
    ocr_debug: Option<OcrDebug>,
    ocr_engine: Option<String>,
    error: Option<String>,
}

impl<C: ScreenCapture, O: OcrEngine> VisionWorker<C, O> {
    pub fn new(capture: C, ocr_factory: OcrFactory<O>) -> Self {
        Self {
            capture,
            ocr_factory,
            ocr: None,
            detectors: None,
            cache_key: None,
            last_fingerprint: None,
            last_read: None,
            last_anvil: None,
            last_sweep: None,
            rate: 0.0,
            last_tick: None,
        }
    }

    pub fn tick(&mut self, engine: &Engine) {
        let job = {
            let st = engine.lock();
            VisionJob::from_state(&st)
        };
        let Some(job) = job else {
            // No Mayhem game: forget the cached reading.
            self.cache_key = None;
            self.last_fingerprint = None;
            self.last_read = None;
            self.last_anvil = None;
            return;
        };
        let now_local = engine.now();
        let out = self.run_job(&job, now_local);
        self.update_rate();
        apply(engine, &job, out, self.rate);
    }

    fn update_rate(&mut self) {
        let now = Instant::now();
        if let Some(prev) = self.last_tick.replace(now) {
            let inst = 1.0 / now.duration_since(prev).as_secs_f64().max(1e-3);
            self.rate = if self.rate == 0.0 { inst } else { self.rate * 0.8 + inst * 0.2 };
        }
    }

    /// A new game invalidates the cached OCR result.
    fn check_cache(&mut self, job: &VisionJob) {
        if self.cache_key != Some(job.session_id) {
            self.cache_key = Some(job.session_id);
            self.last_fingerprint = None;
            self.last_read = None;
            self.last_anvil = None;
        }
    }

    fn ocr_engine(&mut self) -> Result<&O, String> {
        if self.ocr.is_none() {
            self.ocr = Some((self.ocr_factory)().map_err(|e| e.to_string()));
        }
        match self.ocr.as_ref().expect("just set") {
            Ok(engine) => Ok(engine),
            Err(e) => Err(e.clone()),
        }
    }

    fn run_job(&mut self, job: &VisionJob, now_local: f64) -> TickOutput {
        self.check_cache(job);
        let mut out = TickOutput::default();
        let client = match self.capture.game_client_rect() {
            Ok(r) => r,
            Err(e) => {
                out.error = Some(e.to_string());
                return out;
            }
        };
        out.game_window_found = true;
        out.client_size = Some((client.width, client.height));
        // Stamp the sample at capture time, extrapolated from the last Live Client poll.
        out.at = job.clock.game_time_at(now_local);
        if !job.layout.fits(&client) {
            out.error = Some(format!(
                "the game window ({}×{}) is too narrow for the augment cards",
                client.width, client.height
            ));
            return out;
        }

        let key = (job.frame_cfg, job.button_cfg, job.reroll_cfg);
        if self.detectors.as_ref().is_none_or(|(k, _)| *k != key) {
            match CardDetectors::bundled(job.frame_cfg, job.button_cfg, job.reroll_cfg) {
                Ok(d) => self.detectors = Some((key, d)),
                Err(e) => {
                    out.error = Some(format!("detector templates: {e}"));
                    return out;
                }
            }
        }
        let (_, detectors) = self.detectors.as_ref().expect("set above");
        // Scan the card area with nothing to justify it only on the periodic sweep.
        let sweep_due = self.last_sweep.is_none_or(|t| t.elapsed() >= BUTTONLESS_SWEEP);
        let scan = match scan_cards(&mut self.capture, &client, &job.layout, detectors, sweep_due) {
            Ok(s) => s,
            Err(e) => {
                out.error = Some(format!("card capture: {e}"));
                return out;
            }
        };
        let no_buttons = !scan.button.present && !scan.rerolls.iter().any(|r| r.present);
        if sweep_due && no_buttons {
            self.last_sweep = Some(Instant::now());
            if scan.frames.iter().any(|f| f.present) {
                out.error = Some(
                    "the augment cards are on screen but neither the reroll buttons nor the \
                     'hide augments' button were recognised - the bundled templates may be out of date"
                        .into(),
                );
            }
        }
        out.cards = scan.frames.iter().map(CardFrameView::from).collect();
        out.cards_on_screen = scan.cards_on_screen();
        out.rerolls = scan.rerolls.iter().map(RerollView::from).collect();
        out.button = ButtonView { score: scan.button.score, present: scan.button.present };

        match (&scan.titles, &job.matcher) {
            (Some(titles), Some(matcher)) => {
                let lines = job.layout.title_lines();
                self.read_titles(titles, lines, scan.values.as_ref(), matcher.clone(), job.anvils.clone(), &mut out)
            }
            // Cards on screen but no names to match against: say so, keep the offer as is.
            (Some(_), None) => out.error = Some("augment names are not loaded yet (static data)".into()),
            // Cards up but no crops to read: hold the last reading rather than closing the offer.
            (None, _) if scan.cards_on_screen() => {
                out.offer = self.last_read.as_ref().map(|(_, r)| r.reading);
                out.anvil = self.last_anvil.as_ref().map(|(_, r)| r.clone());
            }
            // The cards are genuinely gone. An empty reading lets the tracker close the offer, and
            // the remembered reading has to go with it - the next offer is a different one, and
            // stale slots must not be inherited across it.
            (None, _) => {
                out.offer = Some(OfferReading::default());
                self.last_fingerprint = None;
                self.last_read = None;
                self.last_anvil = None;
            }
        }
        out
    }

    /// Fills slots this pass could not name from the pass before it.
    ///
    /// A title can become unreadable while its card is plainly still there - a spell effect across
    /// it, the reroll animation mid-flip, the game's own tooltip. The augment underneath has not
    /// changed, so the old reading stands until a *new* reading says otherwise; dropping the card
    /// instead would make panels flicker out and back for no reason the player can see.
    ///
    /// This only ever carries within one continuous sighting of the cards: when they leave the
    /// screen the memory is dropped, so the next offer starts from nothing.
    fn sticky(&self, fresh: OfferReading) -> OfferReading {
        let Some((_, prev)) = self.last_read.as_ref() else { return fresh };
        let mut slots = fresh.slots;
        for i in 0..OFFER_SLOTS {
            if slots[i].is_some() {
                continue;
            }
            let Some(id) = prev.reading.slots[i] else { continue };
            // Never inherit an augment another card has just been read as: the same augment is
            // never offered twice, so that would be a reroll we had simply not read yet.
            if slots.iter().flatten().any(|&s| s == id) {
                continue;
            }
            slots[i] = Some(id);
        }
        OfferReading { slots }
    }

    fn read_titles(
        &mut self,
        titles: &[mayhem_vision::RgbaImage; OFFER_SLOTS],
        lines: TitleLines,
        values: Option<&[[mayhem_vision::RgbaImage; 2]; OFFER_SLOTS]>,
        matcher: Arc<AugmentMatcher>,
        anvils: Option<(Arc<ShardMatcher>, Arc<Vec<ShardInfo>>)>,
        out: &mut TickOutput,
    ) {
        let fingerprint = Fingerprint::of(titles);
        let read_at = self.last_read.as_ref().map(|(t, _)| *t).or(self.last_anvil.as_ref().map(|(t, _)| *t));
        let unchanged = self.last_fingerprint.as_ref().is_some_and(|f| f.distance(&fingerprint) < FINGERPRINT_CHANGE)
            && read_at.is_some_and(|t| t.elapsed() < OCR_REFRESH);
        if unchanged {
            if let Some((_, anvil)) = &self.last_anvil {
                out.offer = Some(OfferReading::default());
                out.anvil = Some(anvil.clone());
            } else {
                out.offer = self.last_read.as_ref().map(|(_, r)| r.reading);
            }
            return;
        }
        let engine = match self.ocr_engine() {
            Ok(e) => e,
            Err(e) => {
                out.error = Some(format!("OCR unavailable: {e}"));
                return;
            }
        };
        out.ocr_engine = Some(engine.describe());
        let started = Instant::now();
        let mut read = match read_offer(engine, &matcher, titles, lines) {
            Ok(read) => read,
            Err(e) => {
                out.error = Some(format!("OCR: {e}"));
                return;
            }
        };

        // Stat anvil cards wear the augment card's frame, so they arrive here too. The titles say
        // which it is, and only then are the value lines worth reading.
        if let (Some((shards, catalogue)), Some(values)) = (&anvils, values) {
            let shard_matches = shards.match_offer(&read.texts);
            if is_anvil_offer(&read.matches, &shard_matches) {
                match read_anvil(engine, catalogue, &read.texts, shard_matches, values) {
                    Ok(mut anvil) => {
                        // Anvil cards cannot be rerolled, so a reading that cannot settle the tier
                        // (a cursor over the numbers) keeps the one that could.
                        if anvil.decision.tier.is_none() {
                            if let Some((_, prev)) = self.last_anvil.as_ref().filter(|(_, p)| p.decision.tier.is_some())
                            {
                                anvil.decision = prev.decision.clone();
                            }
                        }
                        out.ocr_debug = Some(OcrDebug {
                            texts: read.texts.clone(),
                            scores: anvil.matches.clone().map(|m| m.map(|m| m.score)),
                            at_game_time: out.at.unwrap_or_default(),
                            millis: started.elapsed().as_millis() as u64,
                        });
                        // No augment offer is up: an empty reading lets the tracker close one.
                        out.offer = Some(OfferReading::default());
                        out.anvil = Some(anvil.clone());
                        self.last_fingerprint = Some(fingerprint);
                        self.last_read = None;
                        self.last_anvil = Some((Instant::now(), anvil));
                    }
                    Err(e) => out.error = Some(format!("OCR (anvil values): {e}")),
                }
                return;
            }
        }

        read.reading = self.sticky(read.reading);
        out.offer = Some(read.reading);
        out.ocr_debug = Some(OcrDebug {
            texts: read.texts.clone(),
            scores: read.matches.map(|m| m.map(|m| m.score)),
            at_game_time: out.at.unwrap_or_default(),
            millis: started.elapsed().as_millis() as u64,
        });
        self.last_fingerprint = Some(fingerprint);
        self.last_read = Some((Instant::now(), read));
        self.last_anvil = None;
    }
}

fn apply(engine: &Engine, job: &VisionJob, out: TickOutput, rate: f64) {
    let mut guard = engine.lock();
    // Reborrow once so disjoint fields can be borrowed independently.
    let st: &mut EngineState = &mut guard;
    st.vision.game_window_found = out.game_window_found;
    st.vision.client_size = out.client_size.or(st.vision.client_size);
    st.vision.samples_per_second = rate;
    st.vision.last_error = out.error;
    st.vision.cards = out.cards;
    st.vision.cards_on_screen = out.cards_on_screen;
    st.vision.rerolls = out.rerolls;
    st.vision.button = out.button;
    st.vision.anvil = out.anvil;
    if out.ocr_engine.is_some() {
        st.vision.ocr_engine = out.ocr_engine;
    }
    if out.ocr_debug.is_some() {
        st.vision.last_ocr = out.ocr_debug;
    }

    let Some(at) = out.at else { return };
    let schedule = st.tuning.augment_schedule.clone();
    let mut events = Vec::new();
    if let Some(game) = st.game.as_mut().filter(|g| g.id == job.session_id) {
        if let Some(reading) = out.offer {
            let unlocked = game.stages_unlocked(&schedule);
            if let Some(e) = game.offers.feed(at, reading, unlocked) {
                events.push(format!("augments {e:?}"));
            }
        }
    }
    for e in events {
        st.log_event(e);
    }
}

/// Starts the worker thread on Windows; elsewhere, records why screen reading is unavailable.
pub fn spawn(engine: Arc<Engine>) {
    #[cfg(windows)]
    {
        use mayhem_vision::paddle::PaddleRecognizer;
        use mayhem_vision::windows::GdiCapture;
        std::thread::Builder::new()
            .name("vision".into())
            .spawn(move || {
                {
                    let mut st = engine.lock();
                    st.vision.available = true;
                    st.vision.unavailable_reason = None;
                }
                let mut worker: VisionWorker<GdiCapture, PaddleRecognizer> =
                    VisionWorker::new(GdiCapture, Box::new(PaddleRecognizer::bundled));
                // The worker is plain data plus GDI calls that hold nothing across a tick, so
                // carrying on to report the panic is sound; it is not ticked again afterwards.
                let run = std::panic::AssertUnwindSafe(|| loop {
                    let started = Instant::now();
                    worker.tick(&engine);
                    let hz = engine.lock().tuning.vision_rate_hz.clamp(0.5, 10.0);
                    let period = Duration::from_secs_f64(1.0 / hz);
                    std::thread::sleep(period.saturating_sub(started.elapsed()));
                });
                // The loop never returns, so getting here means a tick panicked. The panic hook has
                // already written where; this is what stops the UI from claiming the screen is
                // still being read.
                let _ = std::panic::catch_unwind(run);
                let mut st = engine.lock();
                st.vision.available = false;
                st.vision.cards_on_screen = false;
                st.vision.unavailable_reason =
                    Some("the screen reader stopped after an internal error; restart the app".into());
                st.fault("the screen reader stopped (it panicked)".into());
            })
            .expect("spawn vision thread");
    }
    #[cfg(not(windows))]
    {
        let mut st = engine.lock();
        st.vision.available = false;
        st.vision.unavailable_reason = Some("screen capture is implemented for Windows only".into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::GameSession;
    use league_api::lcd::{ActivePlayer, AllGameData, GameData, Player};
    use mayhem_core::augments::{OfferTracker, OfferTrackerConfig};
    use mayhem_vision::capture::StillCapture;
    use mayhem_vision::ocr::ScriptedOcr;
    use mayhem_vision::RgbaImage;
    use static_data::{PoolAugment, StaticData};

    /// A real 1920×1080 screenshot of a Gold offer (the vision crate's fixture).
    const GOLD_OFFER_1080P: &[u8] =
        include_bytes!("../../../../../crates/mayhem-vision/tests/fixtures/screens/augments_1920x1080_gold.jpg");

    fn engine_in_mayhem_game(game_time: f64) -> Engine {
        let dir = std::env::temp_dir().join(format!("mayhem-engine-test-{}", std::process::id()));
        let engine = Engine::new(dir.join("config.json"), dir.join("cache"));
        {
            let mut st = engine.lock();
            let augment = |id: i64, name: &str, rarity: &str| PoolAugment {
                id,
                name_id: format!("ARAM_{id}"),
                name: name.into(),
                rarity: rarity.into(),
                icon_url: String::new(),
            };
            // Also builds the matcher.
            st.set_static_data(StaticData {
                patch: "16.19".into(),
                locale: "default".into(),
                kiwi: vec![
                    augment(11, "Scopier Weapons", "kGold"),
                    augment(12, "Vulnerability", "kGold"),
                    augment(13, "Combusting Interest", "kGold"),
                    augment(14, "Back To Basics", "kPrismatic"),
                ],
                kiwi_jade: Vec::new(),
                unmatched: Vec::new(),
                champion_ids: std::collections::HashMap::new(),
                champion_names: std::collections::HashMap::new(),
                offline: false,
            });
            let me = Player { riot_id: "Me#1".into(), level: 1, ..Default::default() };
            let data = AllGameData {
                active_player: ActivePlayer { level: 1, riot_id: "Me#1".into(), ..Default::default() },
                all_players: vec![me],
                game_data: GameData { game_mode: "KIWI".into(), game_time, map_number: 12, ..Default::default() },
                ..Default::default()
            };
            let mut clock = GameClock::default();
            clock.observe(engine.now(), game_time);
            st.game = Some(GameSession {
                id: 1,
                clock,
                last_game_time: game_time,
                data,
                is_mayhem: true,
                offers: OfferTracker::default(),
            });
        }
        engine
    }

    fn worker(frame: RgbaImage, ocr: &'static [&'static str]) -> VisionWorker<StillCapture, ScriptedOcr> {
        VisionWorker::new(StillCapture { frame }, Box::new(move || Ok(ScriptedOcr::new(ocr.iter().copied()))))
    }

    fn gold_offer() -> RgbaImage {
        image::load_from_memory(GOLD_OFFER_1080P).unwrap().to_rgba8()
    }

    #[test]
    fn real_screenshot_becomes_an_offer() {
        let engine = engine_in_mayhem_game(115.0);
        // Scripted text keeps this test about the engine; the bundled OCR itself is tested on the
        // same screenshots in mayhem-vision (paddle.rs).
        let mut w = worker(gold_offer(), &["Scopier Weapons", "Vulnerability", "Combusting Interest"]);
        w.tick(&engine);
        // Unchanged titles: the second tick reuses the first read instead of OCR-ing again.
        w.tick(&engine);
        let snap = engine.lock().snapshot();
        // The rerolls are the gate everything else came through, and the hide button was never
        // consulted because they answered first.
        assert!(snap.vision.rerolls.iter().any(|r| r.present), "no reroll found: {:?}", snap.vision.rerolls);
        assert!(snap.vision.cards_on_screen);
        assert!(snap.vision.cards.iter().all(|c| c.present));
        let offer = snap.augments.offer.expect("offer shown after two agreeing readings");
        assert_eq!(offer.stage, 1);
        let names: Vec<_> = offer.cards.iter().map(|c| c.as_ref().map(|c| c.name.clone())).collect();
        assert_eq!(
            names,
            vec![Some("Scopier Weapons".into()), Some("Vulnerability".into()), Some("Combusting Interest".into())]
        );
    }

    #[test]
    fn gameplay_without_cards_is_never_ocred() {
        let engine = engine_in_mayhem_game(400.0);
        let frame = RgbaImage::from_pixel(1920, 1080, image::Rgba([30, 50, 60, 255]));
        let mut w = worker(frame, &["Scopier Weapons", "Vulnerability", "Combusting Interest"]);
        for _ in 0..3 {
            w.tick(&engine);
        }
        let st = engine.lock();
        assert!(!st.vision.button.present, "no augment screen on a blank frame");
        assert!(!st.vision.rerolls.iter().any(|r| r.present), "no rerolls on a blank frame");
        assert!(!st.vision.cards_on_screen);
        assert!(st.vision.last_ocr.is_none(), "no OCR without card frames");
        assert!(st.snapshot().augments.offer.is_none());
    }

    /// The blind spot the periodic sweep exists for: the cards are plainly on screen but *every*
    /// cheap detector has been painted over, which is what a patch restyling both button sets would
    /// look like. The sweep must still find the card frames, keep reading the offer, and say what
    /// is wrong instead of leaving the app silently dark.
    #[test]
    fn broken_button_templates_are_reported_but_do_not_blind_the_worker() {
        use mayhem_core::layout::AugmentLayout;
        let engine = engine_in_mayhem_game(115.0);
        let mut frame = gold_offer();
        let client = mayhem_core::geometry::PixelRect::new(0, 0, frame.width(), frame.height());
        let layout = AugmentLayout::default();
        let mut blank_out = |r: mayhem_core::geometry::PixelRect| {
            for y in r.y.max(0) as u32..((r.y + r.height as i32) as u32).min(frame.height()) {
                for x in r.x.max(0) as u32..((r.x + r.width as i32) as u32).min(frame.width()) {
                    frame.put_pixel(x, y, image::Rgba([12, 14, 18, 255]));
                }
            }
        };
        blank_out(layout.button(&client));
        for r in layout.rerolls(&client) {
            blank_out(r);
        }

        let mut w = worker(frame, &["Scopier Weapons", "Vulnerability", "Combusting Interest"]);
        w.tick(&engine);

        let st = engine.lock();
        assert!(!st.vision.button.present, "button score {}", st.vision.button.score);
        assert!(!st.vision.rerolls.iter().any(|r| r.present), "rerolls: {:?}", st.vision.rerolls);
        assert!(st.vision.cards_on_screen, "the sweep should still have found the cards");
        assert!(
            st.vision.last_error.as_deref().is_some_and(|e| e.contains("templates may be out of date")),
            "expected a complaint about the templates, got {:?}",
            st.vision.last_error
        );
    }

    /// A title that becomes unreadable does not drop its card. The augment underneath has not
    /// changed, so the reading stands until a new one replaces it - otherwise a spell effect across
    /// the cards would flicker the panels out and back.
    #[test]
    fn an_unreadable_title_keeps_the_augment_it_last_read() {
        let engine = engine_in_mayhem_game(115.0);
        // The scripted engine is one queue across both passes: the first reads all three titles,
        // the second reads only the middle one, as if something were drawn over the outer two.
        let mut w =
            worker(gold_offer(), &["Scopier Weapons", "Vulnerability", "Combusting Interest", "", "Vulnerability", ""]);
        w.tick(&engine);
        // Force a re-read: the fingerprint has not changed, so without this the cached reading
        // would be reused and the test would prove nothing.
        w.last_fingerprint = None;
        w.tick(&engine);

        let snap = engine.lock().snapshot();
        let offer = snap.augments.offer.expect("the offer stays up");
        let names: Vec<_> = offer.cards.iter().map(|c| c.as_ref().map(|c| c.name.clone())).collect();
        assert_eq!(
            names,
            vec![Some("Scopier Weapons".into()), Some("Vulnerability".into()), Some("Combusting Interest".into())],
            "the unreadable cards should have kept their augments"
        );
    }

    /// ...but only within one sighting. When the cards leave the screen the offer closes and the
    /// remembered reading goes with it, so the next offer cannot inherit stale slots.
    #[test]
    fn the_remembered_reading_is_dropped_when_the_cards_go() {
        let engine = engine_in_mayhem_game(115.0);
        let mut w = worker(gold_offer(), &["Scopier Weapons", "Vulnerability", "Combusting Interest"]);
        // Two ticks: the tracker wants two agreeing readings before it shows an offer. The second
        // reuses the cached read, so the scripted queue is not consumed twice.
        w.tick(&engine);
        w.tick(&engine);
        assert!(engine.lock().snapshot().augments.offer.is_some());

        // The cards go. The tracker wants `close_after` blank readings before it closes, and the
        // scripted OCR queue is empty by now, so any reading that survived would be a stale one.
        w.capture.frame = RgbaImage::from_pixel(1920, 1080, image::Rgba([30, 50, 60, 255]));
        for _ in 0..OfferTrackerConfig::default().close_after {
            w.tick(&engine);
        }
        let st = engine.lock();
        assert!(!st.vision.cards_on_screen);
        assert!(st.snapshot().augments.offer.is_none(), "the offer should close with the cards");
    }

    /// A real 1920×1080 gold stat anvil offer: Magic Penetration, Magic Resist, Might.
    const GOLD_ANVIL_1080P: &[u8] =
        include_bytes!("../../../../../crates/mayhem-vision/tests/fixtures/screens/anvil_1920x1080_gold.png");

    /// Loads the 16.19 shard catalogue fixture into the engine and locks `champion_id`, with a
    /// rankings document putting that champion in one group.
    fn with_anvils(engine: &Engine, champion_id: i64, grouped: bool) {
        use aramkit_client::{AnvilGroup, AnvilRankings, AnvilTiers};
        use mayhem_core::anvils::ShardInfo;

        #[derive(serde::Deserialize)]
        struct Catalogue {
            shards: Vec<ShardInfo>,
        }
        let raw = include_str!("../../../../../crates/mayhem-vision/tests/fixtures/anvils.16.19.en_us.json");
        let shards = serde_json::from_str::<Catalogue>(raw).unwrap().shards;

        let ids = |v: &[&[&str]]| v.iter().map(|b| b.iter().map(|s| s.to_string()).collect()).collect();
        let group = AnvilGroup {
            name: "Mages".into(),
            champions: if grouped { vec![champion_id] } else { vec![] },
            tiers: AnvilTiers {
                // Magic Pen first, then Magic Resist tied with Armor; Might is left unranked.
                gold: ids(&[&["ARAM_GoldStatAnvil_MPen"], &["ARAM_GoldStatAnvil_MR", "ARAM_GoldStatAnvil_AR"]]),
                ..Default::default()
            },
        };

        let mut st = engine.lock();
        st.anvils.matcher = Some(Arc::new(ShardMatcher::new(st.tuning.matcher, &shards)));
        st.anvils.catalogue = Arc::new(shards);
        st.anvils.rankings = Some(AnvilRankings { saved_at: Some(1), version: 1, groups: vec![group] });
        st.client.locked_champion = Some(champion_id);
    }

    /// Titles, then six value lines (card by card, line one then two): what the bundled OCR reads
    /// off the 1920×1080 anvil screenshot, as measured by `anvil_measure`.
    const ANVIL_OCR: &[&str] = &[
        "Magic Penetration Shard",
        "Magic Resist Shard",
        "Might Shard",
        "+ 18 Magic Penetration.",
        "",
        "+O 45 Magic Resist.",
        "",
        "+/ 25 Attack Damage.",
        "+25 Ability Power.",
    ];

    #[test]
    fn a_real_anvil_screenshot_is_ranked_for_the_champion() {
        let engine = engine_in_mayhem_game(700.0);
        with_anvils(&engine, 99, true);
        let frame = image::load_from_memory(GOLD_ANVIL_1080P).unwrap().to_rgba8();
        let mut w = worker(frame, ANVIL_OCR);
        w.tick(&engine);
        // The second tick reuses the reading: the scripted queue is empty, so a re-read would
        // find nothing and the assertions below would fail.
        w.tick(&engine);

        let snap = engine.lock().snapshot();
        assert!(snap.vision.button.present, "the hide button gates anvil offers");
        assert!(snap.augments.offer.is_none(), "an anvil is not an augment offer");
        let anvil = snap.anvil;
        assert!(anvil.on_screen);
        assert_eq!(anvil.tier, Some(mayhem_core::anvils::AnvilTier::Gold));
        assert_eq!(anvil.group.as_deref(), Some("Mages"));
        assert_eq!(anvil.status, None);
        let cards: Vec<_> = anvil.cards.iter().map(|c| c.as_ref().map(|c| (c.id.as_str(), c.rank, c.best))).collect();
        assert_eq!(
            cards,
            vec![
                Some(("ARAM_GoldStatAnvil_MPen", Some(1), true)),
                Some(("ARAM_GoldStatAnvil_MR", Some(2), false)),
                Some(("ARAM_GoldStatAnvil_Hybrid2", None, false)),
            ]
        );
    }

    #[test]
    fn a_champion_in_no_group_gets_no_labels_and_says_why() {
        let engine = engine_in_mayhem_game(700.0);
        with_anvils(&engine, 99, false);
        let frame = image::load_from_memory(GOLD_ANVIL_1080P).unwrap().to_rgba8();
        let mut w = worker(frame, ANVIL_OCR);
        w.tick(&engine);

        let anvil = engine.lock().snapshot().anvil;
        assert!(anvil.on_screen);
        assert_eq!(anvil.group, None);
        assert!(anvil.cards.iter().flatten().all(|c| c.rank.is_none()));
        assert!(anvil.status.as_deref().is_some_and(|s| s.starts_with("no anvil ranking for")), "{:?}", anvil.status);
    }

    #[test]
    fn augment_offers_are_untouched_when_shard_names_are_loaded() {
        let engine = engine_in_mayhem_game(115.0);
        with_anvils(&engine, 99, true);
        let mut w = worker(gold_offer(), &["Scopier Weapons", "Vulnerability", "Combusting Interest"]);
        w.tick(&engine);
        w.tick(&engine);
        let snap = engine.lock().snapshot();
        assert!(snap.augments.offer.is_some());
        assert!(!snap.anvil.on_screen);
    }

    #[test]
    fn no_mayhem_game_is_a_no_op() {
        let engine = engine_in_mayhem_game(400.0);
        engine.lock().game.as_mut().unwrap().is_mayhem = false;
        let mut w = worker(gold_offer(), &[]);
        w.tick(&engine);
        assert!(!engine.lock().vision.game_window_found);
    }
}
