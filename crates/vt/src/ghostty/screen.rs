//! Reading a terminal's state: the cursor, modes, scrollback, and the
//! active screen's lines as text (agent detection reads those).

use libghostty_vt::{
    screen::CellWide,
    style::StyleColor,
    terminal::{Mode, ModeKind, Point, PointCoordinate},
};

use super::GhosttyEngine;

impl GhosttyEngine {
    /// Cursor column and row on the active screen, from 0.
    pub(crate) fn cursor(&self) -> (u16, u16) {
        (self.term.cursor_x().unwrap_or(0), self.term.cursor_y().unwrap_or(0))
    }

    /// Whether an ANSI mode (`CSI n h`, e.g. 4 for insert) is set.
    pub(crate) fn ansi_mode(&self, mode: u16) -> bool {
        self.term.mode(Mode::new(mode, ModeKind::Ansi)).unwrap_or(false)
    }

    /// Lines of scrollback above the screen.
    #[cfg(test)]
    pub(crate) fn history_lines(&self) -> usize {
        self.term.scrollback_rows().unwrap_or(0)
    }

    /// The active screen's lines as text, top to bottom: never the
    /// scrollback, wherever a viewer has scrolled to. Trailing blanks go,
    /// except a blank with a background, which counts as written.
    pub fn screen_lines(&self) -> Vec<String> {
        let term = &self.term;
        let rows = term.rows().unwrap_or(0) as i64;
        let cols = term.cols().unwrap_or(0);
        let history = term.scrollback_rows().unwrap_or(0) as i64;
        let total = term.total_rows().map(|t| t as i64).unwrap_or(history + rows);
        let mut out = Vec::with_capacity(rows as usize);
        for line in 0..rows {
            let y = total - rows + line;
            if y < 0 {
                continue;
            }
            let at = |x: u16| term.grid_ref(Point::Screen(PointCoordinate { x, y: y as u32 }));
            let mut cells = Vec::with_capacity(cols as usize);
            for x in 0..cols {
                let Ok(g) = at(x) else { break };
                let Ok(cell) = g.cell() else { break };
                let wide = cell.wide().unwrap_or(CellWide::Narrow);
                if matches!(wide, CellWide::SpacerTail | CellWide::SpacerHead) {
                    continue;
                }
                let mut buf = ['\0'; 16];
                let n = if cell.has_text().unwrap_or(false) { g.graphemes(&mut buf).unwrap_or(0) } else { 0 };
                let bg = g.style().ok().is_some_and(|s| s.bg_color != StyleColor::None);
                let s: String = if n == 0 { " ".into() } else { buf[..n].iter().collect() };
                cells.push((s, bg, n > 0));
            }
            let keep = cells.iter().rposition(|(s, bg, has)| (*has && s != " ") || *bg).map_or(0, |i| i + 1);
            out.push(cells.into_iter().take(keep).map(|(s, _, _)| s).collect());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VtEngine;

    #[test]
    fn screen_lines_are_the_screen_only() {
        let mut e = GhosttyEngine::new(10, 3);
        for i in 0..5 {
            e.feed(format!("line {i}\r\n").as_bytes());
        }
        e.feed(b"0123456789abc  ");
        assert_eq!(e.screen_lines(), vec!["line 4", "0123456789", "abc"]);
        e.feed(b"\x1b[?1049h\x1b[H\x1b[2Jhello");
        assert_eq!(e.screen_lines(), vec!["hello", "", ""]);
    }
}
