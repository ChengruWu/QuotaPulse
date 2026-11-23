#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

pub fn popup_position(
    tray: Bounds,
    monitor: Bounds,
    work_area: Bounds,
    popup_width: u32,
    popup_height: u32,
    gap: i32,
) -> Point {
    let monitor_right = monitor.x + monitor.width as i32;
    let monitor_bottom = monitor.y + monitor.height as i32;
    let work_right = work_area.x + work_area.width as i32;
    let work_bottom = work_area.y + work_area.height as i32;
    let insets = [
        (work_area.y - monitor.y, 0),
        (monitor_bottom - work_bottom, 1),
        (work_area.x - monitor.x, 2),
        (monitor_right - work_right, 3),
    ];
    let edge = insets
        .into_iter()
        .max_by_key(|(inset, _)| *inset)
        .filter(|(inset, _)| *inset > 0)
        .map(|(_, edge)| edge)
        .unwrap_or(1);

    let popup_width = popup_width as i32;
    let popup_height = popup_height as i32;
    let centered_x = tray.x + tray.width as i32 / 2 - popup_width / 2;
    let centered_y = tray.y + tray.height as i32 / 2 - popup_height / 2;
    let min_x = work_area.x + gap;
    let max_x = (work_right - popup_width - gap).max(min_x);
    let min_y = work_area.y + gap;
    let max_y = (work_bottom - popup_height - gap).max(min_y);

    match edge {
        0 => Point {
            x: centered_x.clamp(min_x, max_x),
            y: min_y,
        },
        2 => Point {
            x: min_x,
            y: centered_y.clamp(min_y, max_y),
        },
        3 => Point {
            x: max_x,
            y: centered_y.clamp(min_y, max_y),
        },
        _ => Point {
            x: centered_x.clamp(min_x, max_x),
            y: max_y,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_above_a_bottom_taskbar_and_clamps_right() {
        assert_eq!(
            popup_position(
                Bounds {
                    x: 1880,
                    y: 1040,
                    width: 24,
                    height: 24
                },
                Bounds {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080
                },
                Bounds {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1040
                },
                320,
                320,
                8,
            ),
            Point { x: 1592, y: 712 }
        );
    }

    #[test]
    fn supports_top_and_side_taskbars() {
        assert_eq!(
            popup_position(
                Bounds {
                    x: 900,
                    y: 8,
                    width: 24,
                    height: 24
                },
                Bounds {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080
                },
                Bounds {
                    x: 0,
                    y: 40,
                    width: 1920,
                    height: 1040
                },
                320,
                320,
                8,
            )
            .y,
            48
        );
        assert_eq!(
            popup_position(
                Bounds {
                    x: 8,
                    y: 500,
                    width: 24,
                    height: 24
                },
                Bounds {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080
                },
                Bounds {
                    x: 40,
                    y: 0,
                    width: 1880,
                    height: 1080
                },
                320,
                320,
                8,
            )
            .x,
            48
        );
    }

    #[test]
    fn supports_negative_monitor_coordinates() {
        let point = popup_position(
            Bounds {
                x: -60,
                y: 1000,
                width: 24,
                height: 24,
            },
            Bounds {
                x: -1920,
                y: 0,
                width: 1920,
                height: 1080,
            },
            Bounds {
                x: -1920,
                y: 0,
                width: 1920,
                height: 1040,
            },
            480,
            480,
            12,
        );
        assert!(point.x >= -1908 && point.x <= -492);
        assert_eq!(point.y, 548);
    }

    #[test]
    fn positions_non_square_360x460_overview_card_above_taskbar() {
        assert_eq!(
            popup_position(
                Bounds {
                    x: 1860,
                    y: 1040,
                    width: 24,
                    height: 24
                },
                Bounds {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080
                },
                Bounds {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1040
                },
                360,
                460,
                8,
            ),
            Point { x: 1552, y: 572 }
        );
    }
}
