// ---------------------------------------------------------------------------
// Swipe view – touch-friendly implementation
// ---------------------------------------------------------------------------
//
// Instead of using `Sense::drag()` on the full area (which steals touch events
// from child widgets such as buttons and scroll areas), we allocate the area
// with `Sense::hover()` and passively observe the raw pointer input to detect
// horizontal swipe gestures.
//
// We use **position-based** offset (current pointer position minus press
// origin) rather than accumulated per-frame deltas.  This makes the gesture
// detection immune to coordinate jitter that can build up over many frames on
// noisy touch panels.

/// Minimum horizontal displacement (logical pixels) before a gesture is
/// visually treated as a page-swipe (the pages start following the finger).
const SWIPE_VISUAL_THRESHOLD: f32 = 14.0;

/// Fraction of the page width the finger must travel to actually change the
/// page when the gesture ends.
const SWIPE_PAGE_FRACTION: f32 = 0.12;

pub struct SwipeView {
    target_page: usize,
    num_pages: usize,

    // -- gesture tracking (persisted across frames) --
    /// The position where the current pointer press started, if it was inside
    /// our rect.  `None` when no gesture is being tracked.
    gesture_start: Option<egui::Pos2>,

    /// Displacement from `gesture_start` to the current pointer position.
    /// Computed from positions, **not** accumulated deltas, so it is robust
    /// against per-frame jitter.
    gesture_offset: egui::Vec2,

    /// `true` once the gesture looks like a deliberate horizontal swipe (the
    /// pages start following the finger).  This is a visual flag only – the
    /// actual decision to change pages is made when the pointer is released.
    gesture_committed: bool,
}

impl SwipeView {
    pub fn new(num_pages: usize) -> Self {
        Self {
            target_page: 0,
            num_pages,
            gesture_start: None,
            gesture_offset: egui::Vec2::ZERO,
            gesture_committed: false,
        }
    }

    pub fn set_page(&mut self, page: usize) {
        self.target_page = page.min(self.num_pages - 1);
    }

    /// Update the number of pages.  The current page is clamped so that it
    /// stays within bounds.
    pub fn set_num_pages(&mut self, num_pages: usize) {
        self.num_pages = num_pages.max(1);
        if self.target_page >= self.num_pages {
            self.target_page = self.num_pages - 1;
        }
    }

    pub fn current_page(&self) -> usize {
        self.target_page
    }

    fn reset_gesture(&mut self) {
        self.gesture_start = None;
        self.gesture_offset = egui::Vec2::ZERO;
        self.gesture_committed = false;
    }

    /// Show the swipe view. `paint_background` is called for each visible page
    /// to draw a background image before the scrollable content.
    /// `add_page` is called for each visible page inside the scroll area.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        mut paint_background: impl FnMut(&egui::Painter, egui::Rect, usize),
        mut add_page: impl FnMut(&mut egui::Ui, usize),
    ) {
        let available_width = ui.available_width();
        let available_height = ui.available_height();

        // Allocate the full area.  We use `Sense::hover()` so that this widget
        // does **not** claim any press / drag interactions – those are left for
        // the child widgets (buttons, scroll areas) rendered below.
        let (rect, _response) = ui.allocate_exact_size(
            egui::vec2(available_width, available_height),
            egui::Sense::hover(),
        );

        // -----------------------------------------------------------------
        // Swipe gesture detection (passive pointer observation)
        // -----------------------------------------------------------------
        let pointer_pressed = ui.input(|i| i.pointer.any_pressed());
        let pointer_down = ui.input(|i| i.pointer.any_down());
        let pointer_released = ui.input(|i| i.pointer.any_released());
        let press_origin = ui.input(|i| i.pointer.press_origin());
        let interact_pos = ui.input(|i| i.pointer.interact_pos());

        // Start tracking when the pointer is pressed inside our rect.
        if pointer_pressed {
            if let Some(pos) = press_origin {
                if rect.contains(pos) {
                    self.gesture_start = Some(pos);
                    self.gesture_offset = egui::Vec2::ZERO;
                    self.gesture_committed = false;
                }
            }
        }

        // While the pointer is down, compute position-based offset.
        if let (Some(start), Some(current)) = (self.gesture_start, interact_pos) {
            if pointer_down {
                // Position-based: immune to accumulated jitter.
                self.gesture_offset = current - start;

                // Visually commit once the horizontal distance is large enough
                // and the gesture is predominantly horizontal.
                if !self.gesture_committed {
                    let abs_x = self.gesture_offset.x.abs();
                    let abs_y = self.gesture_offset.y.abs();
                    if abs_x >= SWIPE_VISUAL_THRESHOLD && abs_x > abs_y {
                        self.gesture_committed = true;
                    }
                }
            }
        }

        // When the pointer is released, decide whether to change pages.
        // The check is intentionally strict: the final displacement must be
        // predominantly horizontal *and* exceed the page-fraction threshold.
        if pointer_released && self.gesture_start.is_some() {
            let ox = self.gesture_offset.x;
            let oy = self.gesture_offset.y;
            let is_horizontal = ox.abs() > oy.abs();
            let swipe_threshold = available_width * SWIPE_PAGE_FRACTION;

            if is_horizontal && ox.abs() > swipe_threshold {
                if ox < 0.0 && self.target_page < self.num_pages - 1 {
                    self.target_page += 1;
                } else if ox > 0.0 && self.target_page > 0 {
                    self.target_page -= 1;
                }
            }
            self.reset_gesture();
        }

        // Safety: if the pointer is not down but we still think we are
        // tracking, reset.  This catches edge-cases where the release event
        // was consumed by another widget or lost.
        if !pointer_down && self.gesture_start.is_some() {
            self.reset_gesture();
        }

        // -----------------------------------------------------------------
        // Compute visual page offset
        // -----------------------------------------------------------------
        let animation_id = ui.id().with("swipe_anim");
        let animated_page =
            ui.ctx()
                .animate_value_with_time(animation_id, self.target_page as f32, 0.25);

        // During a committed swipe the pages follow the finger.
        let page_offset = if self.gesture_committed {
            let raw = animated_page - self.gesture_offset.x / available_width;
            raw.clamp(-0.3, (self.num_pages as f32) - 0.7)
        } else {
            animated_page
        };

        // -----------------------------------------------------------------
        // Paint each visible page
        // -----------------------------------------------------------------
        let clip_rect = rect;
        for page_idx in 0..self.num_pages {
            let page_x = rect.left() + (page_idx as f32 - page_offset) * available_width;

            // Only render pages that are at least partially visible.
            if page_x + available_width < rect.left() - 1.0 || page_x > rect.right() + 1.0 {
                continue;
            }

            let page_rect = egui::Rect::from_min_size(
                egui::pos2(page_x, rect.top()),
                egui::vec2(available_width, available_height),
            );

            let mut child_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(page_rect)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            child_ui.set_clip_rect(clip_rect);

            // Paint the background image (fixed, does not scroll with content)
            paint_background(child_ui.painter(), page_rect, page_idx);

            // Wrap page content in a vertical ScrollArea
            egui::ScrollArea::vertical()
                .id_salt(format!("swipe_page_scroll_{}", page_idx))
                .show(&mut child_ui, |ui| {
                    ui.set_min_width(available_width - 16.0);
                    add_page(ui, page_idx);
                    // Add some bottom padding so content doesn't sit right at the edge
                    ui.add_space(20.0);
                });
        }

        // Request repaint during animation / committed drag for smooth visuals
        if self.gesture_committed || (page_offset - self.target_page as f32).abs() > 0.001 {
            ui.ctx().request_repaint();
        }
    }
}
