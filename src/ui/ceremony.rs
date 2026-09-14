//! Small pure scenes: terminal cells move, journal text never does.

/// Two half-shells travel toward a shared centre. `closed` is in 0..=1.
pub fn capsule_outline(closed: f64, width: usize, ascii: bool) -> String {
    let width = width.max(1);
    if width < 13 {
        return cap_effects::layout_text("[ * ]", width, false)
            .plain_lines()
            .join("\n");
    }
    let max_gap = ((width - 13) / 2).min(8);
    let gap = ((1.0 - closed.clamp(0.0, 1.0)) * max_gap as f64).round() as usize;
    let pad = " ".repeat((width.min(29) - (13 + 2 * gap)) / 2);
    let space = " ".repeat(gap);
    let seam = if gap == 0 {
        if ascii { "---" } else { "───" }.to_owned()
    } else {
        " ".repeat(2 * gap + 3)
    };
    if ascii {
        format!("{pad}+----{seam}----+\n{pad}|    {space} * {space}    |\n{pad}+----{seam}----+")
    } else {
        format!("{pad}╭────{seam}────╮\n{pad}│    {space} ◇ {space}    │\n{pad}╰────{seam}────╯")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shells_move_and_close_without_crossing_the_terminal_edge() {
        for width in [1, 8, 13, 24, 40, 80] {
            let open = capsule_outline(0.0, width, false);
            let closed = capsule_outline(1.0, width, false);
            for scene in [&open, &closed] {
                assert!(scene
                    .lines()
                    .all(|line| cap_effects::grapheme_width(line) <= width));
            }
            if width >= 24 {
                assert_ne!(open, closed);
                assert!(
                    open.lines().next().unwrap().find('╭')
                        < closed.lines().next().unwrap().find('╭')
                );
                assert!(closed.contains("╭───────────╮"));
            }
        }
    }
}
