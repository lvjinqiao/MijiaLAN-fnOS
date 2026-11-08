//! Physical-pixel placement: keep a compact flyout inside its tray monitor's work area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

pub fn place(work: Rect, icon: Option<Rect>, scale: f32, height: f32) -> Rect {
    let gap = (8. * scale).round().max(1.) as i32;
    let width = (360. * scale).round() as i32;
    let height = (height * scale).round() as i32;
    let width = width.min((work.right - work.left - 2 * gap).max(1));
    let height = height.min((work.bottom - work.top - 2 * gap).max(1));
    let min_x = work.left + gap;
    let min_y = work.top + gap;
    let max_x = (work.right - gap - width).max(min_x);
    let max_y = (work.bottom - gap - height).max(min_y);
    let (x, y) = match icon {
        Some(icon) if icon.right <= work.left => (min_x, icon.bottom - height),
        Some(icon) if icon.left >= work.right => (max_x, icon.bottom - height),
        Some(icon) if icon.bottom <= work.top => (icon.right - width, min_y),
        Some(_) => (max_x, max_y),
        None => (max_x, max_y),
    };
    let left = x.clamp(min_x, max_x);
    let top = y.clamp(min_y, max_y);
    Rect {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bottom_taskbar_and_content_height_share_a_bottom_edge() {
        let work = Rect {
            left: 0,
            top: 0,
            right: 800,
            bottom: 560,
        };
        let icon = Some(Rect {
            left: 760,
            top: 560,
            right: 784,
            bottom: 600,
        });
        let tall = place(work, icon, 1., 660.);
        let short = place(work, icon, 1., 280.);
        assert_eq!(
            tall,
            Rect {
                left: 432,
                top: 8,
                right: 792,
                bottom: 552
            }
        );
        assert_eq!(short.bottom, tall.bottom);
        assert_eq!(short.top, 272);
    }
    #[test]
    fn negative_monitor_coordinates_and_high_dpi_stay_in_work_area() {
        let work = Rect {
            left: -1920,
            top: -120,
            right: 0,
            bottom: 920,
        };
        let panel = place(work, None, 2., 660.);
        assert_eq!(
            panel,
            Rect {
                left: -736,
                top: -104,
                right: -16,
                bottom: 904
            }
        );
    }
    #[test]
    fn left_and_top_taskbars_place_the_panel_next_to_their_icons() {
        let work = Rect {
            left: 48,
            top: 0,
            right: 1280,
            bottom: 900,
        };
        assert_eq!(
            place(
                work,
                Some(Rect {
                    left: 0,
                    top: 700,
                    right: 48,
                    bottom: 730
                }),
                1.,
                400.
            )
            .left,
            56
        );
        let work = Rect {
            left: 0,
            top: 40,
            right: 1280,
            bottom: 900,
        };
        assert_eq!(
            place(
                work,
                Some(Rect {
                    left: 1200,
                    top: 0,
                    right: 1230,
                    bottom: 40
                }),
                1.,
                400.
            )
            .top,
            48
        );
    }
}
