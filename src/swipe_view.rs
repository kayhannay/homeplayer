// ---------------------------------------------------------------------------
// Swipe view
// ---------------------------------------------------------------------------

pub struct SwipeView {
    target_page: usize,
    num_pages: usize,
    drag_offset: f32,
    is_dragging: bool,
}

impl SwipeView {
    pub fn new(num_pages: usize) -> Self {
        Self {
            target_page: 0,
            num_pages,
            drag_offset: 0.0,
            is_dragging: false,
        }
    }

    pub fn set_page(&mut self, page: usize) {
        self.target_page = page.min(self.num_pages - 1);
    }

    pub fn current_page(&self) -> usize {
        self.target_page
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

        // Allocate the full area and make it sense drags
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(available_width, available_height),
            egui::Sense::drag(),
        );

        // Handle drag gestures
        if response.dragged() {
            self.is_dragging = true;
            self.drag_offset += response.drag_delta().x;
        }

        if response.drag_stopped() {
            self.is_dragging = false;
            let swipe_threshold = available_width * 0.15;

            if self.drag_offset < -swipe_threshold && self.target_page < self.num_pages - 1 {
                self.target_page += 1;
            } else if self.drag_offset > swipe_threshold && self.target_page > 0 {
                self.target_page -= 1;
            }
            self.drag_offset = 0.0;
        }

        // Animate towards target page
        let animation_id = response.id.with("swipe_anim");
        let animated_page =
            ui.ctx()
                .animate_value_with_time(animation_id, self.target_page as f32, 0.25);

        // During drag, offset the view by the drag amount
        let page_offset = if self.is_dragging {
            // Clamp so you can't drag beyond the edges too far
            let raw = animated_page - self.drag_offset / available_width;
            raw.clamp(-0.3, (self.num_pages as f32) - 0.7)
        } else {
            animated_page
        };

        // Paint each visible page
        let clip_rect = rect;
        for page_idx in 0..self.num_pages {
            let page_x = rect.left() + (page_idx as f32 - page_offset) * available_width;

            // Only render pages that are at least partially visible
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

        // Request repaint during animation/drag for smooth visuals
        if self.is_dragging || (page_offset - self.target_page as f32).abs() > 0.001 {
            ui.ctx().request_repaint();
        }
    }
}
