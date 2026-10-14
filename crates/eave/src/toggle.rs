//! The host side of [`eave_api::ui::toggle`]: a pill with a knob. A click sends the widget's
//! message; when the plugin's state flips, the knob slides over and the pill fills with the ink.

use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, renderer};
use iced::{Border, Color, Element, Event, Length, Rectangle, Size, window};

use eave_api::ui::Tone;

use crate::ease::Ease;
use crate::theme;

pub struct Toggle<M> {
    on: bool,
    on_toggle: M,
}

impl<M> Toggle<M> {
    pub fn new(on: bool, on_toggle: M) -> Self {
        Self { on, on_toggle }
    }
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    let channel = |a: f32, b: f32| a + (b - a) * t;
    Color {
        r: channel(a.r, b.r),
        g: channel(a.g, b.g),
        b: channel(a.b, b.b),
        a: channel(a.a, b.a),
    }
}

impl<M: Clone> Widget<M, iced::Theme, iced::Renderer> for Toggle<M> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Ease>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Ease::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(
            Length::Fixed(theme::TOGGLE_WIDTH),
            Length::Fixed(theme::TOGGLE_HEIGHT),
        )
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        _renderer: &iced::Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        tree.state
            .downcast_mut::<Ease>()
            .set(if self.on { 1.0 } else { 0.0 });
        layout::Node::new(Size::new(theme::TOGGLE_WIDTH, theme::TOGGLE_HEIGHT))
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let p = crate::theme::Palette::of(theme);
        use iced::advanced::Renderer as _;

        let t = tree.state.downcast_ref::<Ease>().value();
        let bounds = layout.bounds();
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: Border::default().rounded(bounds.height / 2.0),
                ..renderer::Quad::default()
            },
            mix(p.track, p.accent_fill, t),
        );
        let inset = (bounds.height - theme::TOGGLE_KNOB) / 2.0;
        let travel = bounds.width - 2.0 * inset - theme::TOGGLE_KNOB;
        let knob = Rectangle::new(
            iced::Point::new(bounds.x + inset + travel * t, bounds.y + inset),
            Size::new(theme::TOGGLE_KNOB, theme::TOGGLE_KNOB),
        );
        renderer.fill_quad(
            renderer::Quad {
                bounds: knob,
                border: Border::default().rounded(theme::TOGGLE_KNOB / 2.0),
                ..renderer::Quad::default()
            },
            mix(p.tone(Tone::Dim), p.on_accent, t),
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        _viewport: &Rectangle,
    ) {
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if cursor.is_over(layout.bounds()) =>
            {
                shell.publish(self.on_toggle.clone());
                shell.capture_event();
            }
            Event::Window(window::Event::RedrawRequested(now))
                if tree
                    .state
                    .downcast_mut::<Ease>()
                    .step(*now, theme::TOGGLE_SLIDE) =>
            {
                shell.request_redraw();
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::None
        }
    }
}

impl<'a, M: Clone + 'a> From<Toggle<M>> for Element<'a, M> {
    fn from(toggle: Toggle<M>) -> Self {
        Element::new(toggle)
    }
}
