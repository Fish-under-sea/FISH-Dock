// The displays' work areas (and the whole of each), in physical pixels, read now and then (a window
// getter waits on the main thread, so not on every move).
use tauri::{AppHandle, Monitor};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub sf: f64,
    // The whole display: x, y, width, height (the top bar takes its strip from it).
    pub mon: (i32, i32, i32, i32),
}

#[derive(Clone, Default)]
pub struct Screens {
    pub areas: Vec<Area>,
    pub primary: Option<Area>,
}

fn area(m: &Monitor) -> Area {
    let wa = m.work_area();
    let (p, s) = (m.position(), m.size());
    Area { x: wa.position.x, y: wa.position.y, w: wa.size.width as i32, h: wa.size.height as i32, sf: m.scale_factor(), mon: (p.x, p.y, s.width as i32, s.height as i32) }
}

pub fn read(app: &AppHandle) -> Screens {
    Screens {
        areas: app.available_monitors().unwrap_or_default().iter().map(area).collect(),
        primary: app.primary_monitor().ok().flatten().map(|m| area(&m)),
    }
}

impl Screens {
    // For the log: each display, the main one marked, with its scale and
    // work area (multi-display reports could not tell them: #4).
    pub fn describe(&self) -> String {
        self.areas
            .iter()
            .map(|a| {
                let main = self.primary.is_some_and(|p| p.mon == a.mon);
                format!("{:?} at {}%{}, work area ({}, {}, {}, {})", a.mon, (a.sf * 100.0).round(), if main { ", main" } else { "" }, a.x, a.y, a.w, a.h)
            })
            .collect::<Vec<_>>()
            .join("; ")
    }

    // The work area holding this point, or the nearest one.
    pub fn near(&self, x: i32, y: i32) -> Option<Area> {
        let gap = |a: &Area| {
            let dx = (a.x - x).max(0).max(x - (a.x + a.w)) as i64;
            let dy = (a.y - y).max(0).max(y - (a.y + a.h)) as i64;
            dx * dx + dy * dy
        };
        self.areas.iter().copied().min_by_key(gap).or(self.primary)
    }

    pub fn preferred(&self, last: Option<(i32, i32, i32, i32)>) -> Option<Area> {
        last.and_then(|mon| self.areas.iter().copied().find(|a| a.mon == mon)).or(self.primary)
    }

    // A window of this size at (x, y), kept on some display's work area.
    pub fn clamp(&self, (x, y): (i32, i32), (w, h): (i32, i32)) -> (i32, i32) {
        let Some(a) = self.near(x + w / 2, y + h / 2) else { return (x, y) };
        (x.max(a.x).min(a.x + a.w - w), y.max(a.y).min(a.y + a.h - h))
    }
}

// The pointer, in the physical pixels of a window whose display has the scale
// sf. On a Mac Tauri gives it in the primary display's scale, wherever it is,
// while each window's place is in its own display's: on a second display of
// another scale she never saw the pointer over her and could not be clicked
// (2026-10-14). Elsewhere the two agree.
pub fn cursor(sh: &crate::Shared, sf: f64) -> Option<(i32, i32)> {
    let c = sh.app.cursor_position().ok()?;
    #[cfg(target_os = "macos")]
    let (x, y) = {
        let primary = sh.screens().primary.map_or(1.0, |a| a.sf);
        (c.x / primary * sf, c.y / primary * sf)
    };
    #[cfg(not(target_os = "macos"))]
    let (x, y) = {
        let _ = sf;
        (c.x, c.y)
    };
    Some((x.round() as i32, y.round() as i32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_uses_the_saved_display_or_the_primary_when_it_is_gone() {
        let primary = Area { x: 0, y: 0, w: 1920, h: 1040, sf: 1.0, mon: (0, 0, 1920, 1080) };
        let second = Area { x: -2560, y: 0, w: 2560, h: 1400, sf: 1.25, mon: (-2560, 0, 2560, 1440) };
        let screens = Screens { areas: vec![primary, second], primary: Some(primary) };
        assert_eq!(screens.preferred(None), Some(primary));
        assert_eq!(screens.preferred(Some(second.mon)), Some(second));
        let disconnected = Screens { areas: vec![primary], primary: Some(primary) };
        assert_eq!(disconnected.preferred(Some(second.mon)), Some(primary));
        assert_eq!(Screens::default().preferred(Some(second.mon)), None);
    }
}
