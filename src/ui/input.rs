//! Native single-line input, adapted from GPUI's input example.
//!
//! The platform talks in UTF-16 offsets; the selection and renderer use UTF-8.
use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, ContentMask, Context, CursorStyle, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId, KeyBinding,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point,
    ShapedLine, SharedString, Style, TextRun, UTF16Selection, UnderlineStyle, Window, actions, div,
    fill, point, prelude::*, px, relative, rgb, rgba, size,
};
use unicode_segmentation::UnicodeSegmentation;

actions!(
    neko_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        SelectHome,
        SelectEnd,
        WordLeft,
        WordRight,
        SelectWordLeft,
        SelectWordRight,
        DeleteWordLeft,
        DeleteWordRight,
        Paste,
        Cut,
        Copy,
        Submit,
        Escape,
    ]
);

#[derive(Clone, Copy, Debug)]
pub enum InputEvent {
    Changed,
    Submit,
    Escape,
}

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some("NekoTextInput")),
        KeyBinding::new("delete", Delete, Some("NekoTextInput")),
        KeyBinding::new("left", Left, Some("NekoTextInput")),
        KeyBinding::new("right", Right, Some("NekoTextInput")),
        KeyBinding::new("shift-left", SelectLeft, Some("NekoTextInput")),
        KeyBinding::new("shift-right", SelectRight, Some("NekoTextInput")),
        KeyBinding::new("ctrl-a", SelectAll, Some("NekoTextInput")),
        KeyBinding::new("ctrl-v", Paste, Some("NekoTextInput")),
        KeyBinding::new("ctrl-c", Copy, Some("NekoTextInput")),
        KeyBinding::new("ctrl-x", Cut, Some("NekoTextInput")),
        KeyBinding::new("shift-insert", Paste, Some("NekoTextInput")),
        KeyBinding::new("ctrl-insert", Copy, Some("NekoTextInput")),
        KeyBinding::new("shift-delete", Cut, Some("NekoTextInput")),
        KeyBinding::new("home", Home, Some("NekoTextInput")),
        KeyBinding::new("end", End, Some("NekoTextInput")),
        KeyBinding::new("shift-home", SelectHome, Some("NekoTextInput")),
        KeyBinding::new("shift-end", SelectEnd, Some("NekoTextInput")),
        KeyBinding::new("ctrl-left", WordLeft, Some("NekoTextInput")),
        KeyBinding::new("ctrl-right", WordRight, Some("NekoTextInput")),
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, Some("NekoTextInput")),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, Some("NekoTextInput")),
        KeyBinding::new("ctrl-backspace", DeleteWordLeft, Some("NekoTextInput")),
        KeyBinding::new("ctrl-delete", DeleteWordRight, Some("NekoTextInput")),
        KeyBinding::new("enter", Submit, Some("NekoTextInput")),
        KeyBinding::new("escape", Escape, Some("NekoTextInput")),
    ]);
}

pub struct TextInput {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    scroll_offset: Pixels,
    is_selecting: bool,
}

impl EventEmitter<InputEvent> for TextInput {}

impl TextInput {
    pub fn new(placeholder: &str, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: "".into(),
            placeholder: placeholder.to_owned().into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            scroll_offset: px(0.),
            is_selecting: false,
        }
    }

    pub fn value(&self) -> String {
        self.content.to_string()
    }

    pub fn set_value(&mut self, value: &str, cx: &mut Context<Self>) {
        let value = single_line(value);
        let changed = self.content.as_ref() != value.as_str();
        self.content = value.into();
        self.selected_range = self.content.len()..self.content.len();
        self.selection_reversed = false;
        self.marked_range = None;
        self.is_selecting = false;
        if changed {
            cx.emit(InputEvent::Changed);
        }
        cx.notify();
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        let offset = if self.selected_range.is_empty() {
            self.previous_boundary(self.cursor_offset())
        } else {
            self.selected_range.start
        };
        self.move_to(offset, cx);
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        let offset = if self.selected_range.is_empty() {
            self.next_boundary(self.cursor_offset())
        } else {
            self.selected_range.end
        };
        self.move_to(offset, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn select_home(&mut self, _: &SelectHome, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, cx);
    }

    fn select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.content.len(), cx);
    }

    fn word_left(&mut self, _: &WordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.previous_word_boundary(), cx);
    }

    fn word_right(&mut self, _: &WordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.next_word_boundary(), cx);
    }

    fn select_word_left(&mut self, _: &SelectWordLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_word_boundary(), cx);
    }

    fn select_word_right(&mut self, _: &SelectWordRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_word_boundary(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_word_left(
        &mut self,
        _: &DeleteWordLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_word_boundary(), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_word_right(
        &mut self,
        _: &DeleteWordRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_word_boundary(), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        if self.marked_range.is_none() {
            cx.emit(InputEvent::Submit);
        }
    }

    fn escape(&mut self, _: &Escape, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(InputEvent::Escape);
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        self.is_selecting = true;
        let index = self.index_for_mouse_position(event.position);
        if event.click_count >= 3 {
            self.move_to(0, cx);
            self.select_to(self.content.len(), cx);
        } else if event.click_count == 2 {
            let range = self
                .content
                .split_word_bound_indices()
                .find(|(start, word)| *start <= index && index < *start + word.len())
                .map(|(start, word)| start..start + word.len())
                .unwrap_or(index..index);
            self.move_to(range.start, cx);
            self.select_to(range.end, cx);
        } else if event.modifiers.shift {
            self.select_to(index, cx);
        } else {
            self.move_to(index, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_owned(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_owned(),
            ));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds, &self.last_layout) else {
            return 0;
        };
        if line.text != self.content {
            return self.cursor_offset();
        }
        let index = line.closest_index_for_x(position.x - bounds.left() + self.scroll_offset);
        // Shapers may expose a code-point position inside an extended grapheme.
        self.content
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .chain(std::iter::once(self.content.len()))
            .min_by_key(|boundary| boundary.abs_diff(index))
            .unwrap_or(0)
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        let anchor = if self.selection_reversed {
            self.selected_range.end
        } else {
            self.selected_range.start
        };
        self.selection_reversed = offset < anchor;
        self.selected_range = offset.min(anchor)..offset.max(anchor);
        cx.notify();
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        utf8_to_utf16(&self.content, range.start)..utf8_to_utf16(&self.content, range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        let start = utf16_to_utf8(&self.content, range.start);
        let end = utf16_to_utf8(&self.content, range.end);
        start.min(end)..start.max(end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(index, _)| (index < offset).then_some(index))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(index, _)| (index > offset).then_some(index))
            .unwrap_or(self.content.len())
    }

    fn previous_word_boundary(&self) -> usize {
        self.content
            .unicode_word_indices()
            .rev()
            .find_map(|(index, _)| (index < self.cursor_offset()).then_some(index))
            .unwrap_or(0)
    }

    fn next_word_boundary(&self) -> usize {
        self.content
            .unicode_word_indices()
            .find_map(|(index, _)| (index > self.cursor_offset()).then_some(index))
            .unwrap_or(self.content.len())
    }
}

fn single_line(text: &str) -> String {
    text.replace("\r\n", " ")
        .replace(['\n', '\r', '\t', '\u{2028}', '\u{2029}'], " ")
}

fn utf16_to_utf8(text: &str, offset: usize) -> usize {
    let mut utf16_count = 0;
    for (index, ch) in text.char_indices() {
        if utf16_count >= offset {
            return index;
        }
        utf16_count += ch.len_utf16();
    }
    text.len()
}

fn utf8_to_utf16(text: &str, offset: usize) -> usize {
    text.char_indices()
        .take_while(|(index, _)| *index < offset)
        .map(|(_, ch)| ch.len_utf16())
        .sum()
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let new_text = single_line(new_text);
        let content =
            self.content[..range.start].to_owned() + &new_text + &self.content[range.end..];
        let changed = content.as_str() != self.content.as_ref();
        self.content = content.into();
        let cursor = range.start + new_text.len();
        self.selected_range = cursor..cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        if changed {
            cx.emit(InputEvent::Changed);
        }
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let new_text = single_line(new_text);
        let content =
            self.content[..range.start].to_owned() + &new_text + &self.content[range.end..];
        let changed = content.as_str() != self.content.as_ref();
        self.content = content.into();
        self.marked_range =
            (!new_text.is_empty()).then_some(range.start..range.start + new_text.len());
        self.selected_range = new_selected_range_utf16
            .map(|selection| {
                // Composition selection is relative to the newly inserted text, not the document.
                let start = utf16_to_utf8(&new_text, selection.start);
                let end = utf16_to_utf8(&new_text, selection.end);
                range.start + start.min(end)..range.start + start.max(end)
            })
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.selection_reversed = false;
        if changed {
            cx.emit(InputEvent::Changed);
        }
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let line = self.last_layout.as_ref()?;
        if line.text != self.content {
            return None;
        }
        let range = self.range_from_utf16(&range_utf16);
        let bounds = self.last_bounds.unwrap_or(bounds);
        Some(Bounds::from_corners(
            point(
                (bounds.left() + line.x_for_index(range.start) - self.scroll_offset)
                    .max(bounds.left())
                    .min(bounds.right()),
                bounds.top(),
            ),
            point(
                (bounds.left() + line.x_for_index(range.end) - self.scroll_offset)
                    .max(bounds.left())
                    .min(bounds.right()),
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        self.last_bounds?.localize(&position)?;
        Some(utf8_to_utf16(
            &self.content,
            self.index_for_mouse_position(position),
        ))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    line: ShapedLine,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
    scroll_offset: Pixels,
}

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = px(20.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> PrepaintState {
        let input = self.input.read(cx);
        let style = window.text_style();
        let display_text = if input.content.is_empty() {
            input.placeholder.clone()
        } else {
            input.content.clone()
        };
        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: if input.content.is_empty() {
                rgb(0x85858d).into()
            } else {
                rgb(0xeeeeee).into()
            },
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked) = &input.marked_range {
            vec![
                TextRun {
                    len: marked.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked.end - marked.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };
        let line = window.text_system().shape_line(
            display_text,
            style.font_size.to_pixels(window.rem_size()),
            &runs,
            None,
        );
        let cursor_x = line.x_for_index(input.cursor_offset());
        let mut scroll_offset = input.scroll_offset;
        let available = (bounds.size.width - px(2.)).max(px(0.));
        if input.content.is_empty() {
            scroll_offset = px(0.);
        } else {
            if cursor_x < scroll_offset {
                scroll_offset = cursor_x;
            }
            if cursor_x > scroll_offset + available {
                scroll_offset = cursor_x - available;
            }
            scroll_offset = scroll_offset
                .min((line.width - available).max(px(0.)))
                .max(px(0.));
        }
        let origin_x = bounds.left() - scroll_offset;
        let focused = input.focus_handle.is_focused(window);
        let cursor = (focused && input.selected_range.is_empty()).then(|| {
            fill(
                Bounds::new(
                    point(origin_x + cursor_x, bounds.top() + px(1.)),
                    size(px(1.5), bounds.size.height - px(2.)),
                ),
                rgb(0xc7b6f4),
            )
        });
        let selection = (!input.selected_range.is_empty()).then(|| {
            fill(
                Bounds::from_corners(
                    point(
                        origin_x + line.x_for_index(input.selected_range.start),
                        bounds.top(),
                    ),
                    point(
                        origin_x + line.x_for_index(input.selected_range.end),
                        bounds.bottom(),
                    ),
                ),
                if focused {
                    rgba(0xb7a4e84d)
                } else {
                    rgba(0xb7a4e826)
                },
            )
        });
        PrepaintState {
            line,
            cursor,
            selection,
            scroll_offset,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        // Div::on_mouse_move only fires while hovered. Native selection must
        // continue when the pointer moves beyond the field during a drag.
        if self.input.read(cx).is_selecting {
            let input = self.input.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase.bubble() {
                    input.update(cx, |input, cx| input.on_mouse_move(event, window, cx));
                }
            });
            let input = self.input.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                if phase.bubble() && event.button == MouseButton::Left {
                    input.update(cx, |input, cx| input.on_mouse_up(event, window, cx));
                }
            });
        }
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if let Some(selection) = prepaint.selection.take() {
                window.paint_quad(selection);
            }
            let origin = point(bounds.left() - prepaint.scroll_offset, bounds.top());
            let _ = prepaint.line.paint(origin, px(20.), window, cx);
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        });
        self.input.update(cx, |input, _| {
            input.last_layout = Some(prepaint.line.clone());
            input.last_bounds = Some(bounds);
            input.scroll_offset = prepaint.scroll_offset;
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("neko-text-input")
            .flex()
            .items_center()
            .w_full()
            .min_w(px(0.))
            .h(px(38.))
            .px(px(12.))
            .rounded(px(9.))
            .border_1()
            .border_color(rgb(0x303034))
            .bg(rgb(0x222226))
            .text_size(px(13.))
            .line_height(px(20.))
            .text_color(rgb(0xeeeeee))
            .key_context("NekoTextInput")
            .track_focus(&self.focus_handle)
            .focus(|style| style.border_color(rgb(0xb7a4e8)))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::select_home))
            .on_action(cx.listener(Self::select_end))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::delete_word_left))
            .on_action(cx.listener(Self::delete_word_right))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::submit))
            .on_action(cx.listener(Self::escape))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .child(TextElement { input: cx.entity() })
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_offsets_round_trip_for_unicode() {
        let text = "a🌙café e\u{301}日本";
        for (byte, _) in text
            .char_indices()
            .chain(std::iter::once((text.len(), '\0')))
        {
            assert_eq!(utf16_to_utf8(text, utf8_to_utf16(text, byte)), byte);
        }
        assert_eq!(utf16_to_utf8(text, usize::MAX), text.len());
        assert_eq!(utf16_to_utf8("🌙a", 1), 4);
    }

    #[test]
    fn pasted_text_stays_on_one_line() {
        assert_eq!(
            single_line("one\r\ntwo\rthree\nfour\tfive\u{2028}six"),
            "one two three four five six"
        );
        assert_eq!(single_line("星の夜 🌙"), "星の夜 🌙");
    }
}
