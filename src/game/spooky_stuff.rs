//! SpookyStuffBehaviour: plays StartOfDaySound once in the StartOfDayTime hour and EndOfDaySound once in the
//! EndOfDayTime hour of each local day. No scene or prefab of the game carries one, so nothing links it.
#![allow(non_snake_case, dead_code)]

use super::coroutine::Yield;
use super::Game;

/// One SpookyStuffBehaviour: the component's instance id (its Source, EndOfDaySound and StartOfDaySound are the
/// script's fields in `Game::audio`), and its state.
#[derive(Clone, Debug, Default)]
pub struct SpookyStuff {
    pub id: i32,
    pub StartOfDayTime: i32,
    pub EndOfDayTime: i32,
    hadStart: bool,
    hadEnd: bool,
    lastDay: u16,
}

impl Game {
    /// SpookyStuffBehaviour.Start: lastDay = DateTime.Now.Day, then StartCoroutine(Tick()) (its first step runs now).
    pub fn spooky_stuff_start(&mut self, s: &mut SpookyStuff) -> Yield {
        s.lastDay = super::television::local_now().1;
        self.spooky_stuff_tick(s)
    }

    /// SpookyStuffBehaviour.Tick: one pass; its `yield return new WaitForSecondsRealtime(60f)`, after which the
    /// coroutine starts a new Tick.
    pub fn spooky_stuff_tick(&mut self, s: &mut SpookyStuff) -> Yield {
        let (_, day, seconds) = super::television::local_now();
        let hour = (seconds / 3600.0).floor() as i32;
        if hour == s.StartOfDayTime {
            self.spooky_stuff_start_of_day(s);
        } else if hour == s.EndOfDayTime {
            self.spooky_stuff_end_of_day(s);
        }
        if day != s.lastDay {
            s.lastDay = day;
            s.hadEnd = false;
            s.hadStart = false;
        }
        Yield::WaitForSecondsRealtime(60.0)
    }

    /// SpookyStuffBehaviour.StartOfDay
    fn spooky_stuff_start_of_day(&mut self, s: &mut SpookyStuff) {
        if !s.hadStart {
            s.hadStart = true;
            self.spooky_stuff_play(s.id, "StartOfDaySound");
        }
    }

    /// SpookyStuffBehaviour.EndOfDay
    fn spooky_stuff_end_of_day(&mut self, s: &mut SpookyStuff) {
        if !s.hadEnd {
            s.hadEnd = true;
            self.spooky_stuff_play(s.id, "EndOfDaySound");
        }
    }

    /// Source.PlayOneShot(clip): a null Source throws.
    fn spooky_stuff_play(&mut self, id: i32, clip: &str) {
        let src = self.audio.source_field(id, "Source").unwrap_or_else(|| super::menu::throw("NullReferenceException: Object reference not set to an instance of an object"));
        let clip = self.audio.clip(id, clip);
        self.audio.play_one_shot(src, clip.as_ref(), 1.0);
    }
}
