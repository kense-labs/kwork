//! Keep the visible turn in place when new line breaks change the height of earlier turns.
use gpui::{prelude::*, *};
use std::{cell::Cell, rc::Rc};

pub(super) struct ReadingAnchor {
    index: usize,
    depth: f32,
    layout: Rc<Cell<Option<LayoutId>>>,
}

impl ReadingAnchor {
    pub(super) fn capture(scroll: &ScrollHandle) -> Option<Self> {
        let (index, offset) = scroll.logical_scroll_top();
        let height = scroll.bounds_for_item(index)?.size.height;
        if height <= px(0.) {
            return None;
        }
        Some(Self {
            index,
            depth: (-f32::from(offset) / f32::from(height)).clamp(0., 1.),
            layout: Rc::default(),
        })
    }

    pub(super) fn track(&self, index: usize, row: AnyElement) -> AnyElement {
        if index == self.index {
            MeasuredRow {
                row,
                layout: self.layout.clone(),
            }
            .into_any_element()
        } else {
            row
        }
    }

    pub(super) fn restore(
        &self,
        viewport: Bounds<Pixels>,
        scroll: &ScrollHandle,
        window: &mut Window,
    ) {
        if let Some(id) = self.layout.get() {
            let row = window.layout_bounds(id);
            let y = viewport.top() - row.top() - row.size.height * self.depth;
            scroll.set_offset(point(scroll.offset().x, y));
        }
    }
}

// Expose the row's new bounds after layout but before the scroll view paints this frame.
struct MeasuredRow {
    row: AnyElement,
    layout: Rc<Cell<Option<LayoutId>>>,
}

impl IntoElement for MeasuredRow {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for MeasuredRow {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let id = self.row.request_layout(window, cx);
        self.layout.set(Some(id));
        (id, ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.row.prepaint(window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.row.paint(window, cx);
    }
}
