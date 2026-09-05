use da_vinci_protocol::Level;
use iced::{
    Background, Border, Element, Length,
    alignment::{Horizontal, Vertical},
    widget::{checkbox, column, container, pick_list, responsive, row, scrollable, text},
};

use super::{CONTROL_TEXT_SIZE, danger_native_button, native_button};
use crate::{
    app::{App, BANK_TABS, BankTab, Message},
    session::{ListenerState, Mode, PinInfo, PinKey, PinState},
    theme::{HIGH_BG, LOW_BG, UI_TEXT, UNSET_BG, input_style, panel_style, selected_tab_button},
};

const MODES: [Mode; 3] = [Mode::Input, Mode::InputPullup, Mode::Output];
const ROW_HEIGHT: f32 = 34.0;
const PIN_CONTROL_TEXT_SIZE: f32 = 12.0;
const PIN_NAME_SHARE: u16 = 5;
const PIN_MODE_SHARE: u16 = 7;
const PIN_STATUS_SHARE: u16 = 3;
const PIN_RW_SHARE: u16 = 7;
const PIN_LISTEN_SHARE: u16 = 5;
const PIN_TABLE_TWO_COLUMN_MIN: f32 = 800.0;
const CELL_GAP: f32 = 4.0;

impl App {
    pub(super) fn pin_panel(&self) -> Element<'_, Message> {
        let index = self.bank_tab.index();
        let mut tabs =
            row![native_button("‹").on_press_maybe((index > 0).then_some(Message::PreviousTab))]
                .spacing(4)
                .align_y(iced::Alignment::Center);
        for tab in BANK_TABS {
            let tab_button = if tab == self.bank_tab {
                native_button(tab.label()).style(selected_tab_button)
            } else {
                native_button(tab.label())
            };
            tabs = tabs.push(tab_button.on_press(Message::TabSelected(tab)));
        }
        tabs = tabs.push(
            native_button("›")
                .on_press_maybe((index + 1 < BANK_TABS.len()).then_some(Message::NextTab)),
        );

        let bulk_scope = row![
            text("Scope").size(12),
            pick_list(
                self.bulk_scopes.as_slice(),
                Some(&self.bulk_scope),
                Message::BulkScopeSelected
            )
            .text_size(PIN_CONTROL_TEXT_SIZE)
            .padding([5, 8]),
            text("Mode").size(12),
            pick_list(MODES, Some(self.bulk_mode), Message::BulkModeSelected)
                .text_size(PIN_CONTROL_TEXT_SIZE)
                .padding([5, 8]),
            checkbox(self.overwrite)
                .label("Overwrite")
                .size(16)
                .spacing(5)
                .text_size(CONTROL_TEXT_SIZE)
                .on_toggle(Message::OverwriteChanged),
            native_button("Apply mode").on_press(Message::ApplyBulkMode),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .wrap()
        .vertical_spacing(4);

        let bulk_actions: Element<'_, Message> = if let Some((target, level)) = &self.confirm_set {
            let level_name = match level {
                Level::High => "HIGH",
                Level::Low => "LOW",
            };
            row![
                text(format!("Set every output in {target} {level_name}?")),
                danger_native_button(format!("Set {level_name}")).on_press(Message::BulkSetConfirm),
                native_button("Cancel").on_press(Message::BulkSetCancel),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .wrap()
            .vertical_spacing(4)
            .into()
        } else {
            row![
                native_button("Read").on_press(Message::BulkRead),
                native_button("Listen").on_press(Message::BulkListen(true)),
                native_button("Stop listening").on_press(Message::BulkListen(false)),
                native_button("Set HIGH").on_press(Message::BulkSet(Level::High)),
                native_button("Set LOW").on_press(Message::BulkSet(Level::Low)),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center)
            .wrap()
            .vertical_spacing(4)
            .into()
        };
        let bulk = column![bulk_scope, bulk_actions].spacing(4);

        let table: Element<'_, Message> = match self.bank_tab {
            BankTab::A => self.full_bank_table("PIOA"),
            BankTab::C => self.full_bank_table("PIOC"),
            BankTab::D => self.full_bank_table("PIOD"),
            BankTab::BAndE => responsive(|size| {
                responsive_pin_columns(
                    size.width,
                    self.bank_column("PIOB", 0, u8::MAX, true),
                    self.bank_column("PIOE", 0, u8::MAX, true),
                )
            })
            .height(Length::Fill)
            .into(),
        };

        let content = column![tabs, bulk, table].spacing(6);

        container(content)
            .padding(8)
            .style(panel_style)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn full_bank_table(&self, bank_token: &'static str) -> Element<'_, Message> {
        responsive(move |size| {
            responsive_pin_columns(
                size.width,
                self.bank_column(bank_token, 0, 16, false),
                self.bank_column(bank_token, 16, 16, false),
            )
        })
        .height(Length::Fill)
        .into()
    }

    fn bank_column(
        &self,
        bank_token: &str,
        start_bit: u8,
        count: u8,
        show_bank_name: bool,
    ) -> iced::widget::Column<'_, Message> {
        let mut column = column![].spacing(2);
        if show_bank_name {
            column = column.push(text(bank_token.to_owned()).size(14));
        }
        column = column.push(pin_header());
        let Some(bank) = self.session.bank_key(self.sam_route, bank_token) else {
            return column;
        };
        let end = start_bit.saturating_add(count);
        for (pin, info) in self.session.pins(self.sam_route) {
            if info.bank != bank || info.bit < start_bit || info.bit >= end {
                continue;
            }
            column = column.push(self.pin_row(pin));
        }
        column
    }

    fn pin_row(&self, pin: PinKey) -> Element<'_, Message> {
        let Some(info) = self.session.pin_info(pin) else {
            return text("Unknown pin").into();
        };
        let name = pin_cell(text(pin_display(info)).size(12), PIN_NAME_SHARE);
        if !info.capabilities.available() {
            return row![
                name,
                pin_cell(text("RESERVED").size(11), PIN_MODE_SHARE),
                level_box(None, false),
                pin_cell(text("System").size(11), PIN_RW_SHARE),
                pin_cell(text(""), PIN_LISTEN_SHARE),
            ]
            .spacing(CELL_GAP)
            .width(Length::Fill)
            .height(Length::Fixed(ROW_HEIGHT))
            .align_y(iced::Alignment::Center)
            .into();
        }

        let state = self.session.pin_state(pin).unwrap_or(PinState::UNSET);
        let mode: Element<'_, Message> = if state.target_mode.is_some() {
            container(
                text(
                    state
                        .mode
                        .map(|mode| mode.to_string())
                        .unwrap_or_else(|| "UNSET".into()),
                )
                .size(PIN_CONTROL_TEXT_SIZE)
                .wrapping(text::Wrapping::None),
            )
            .padding([5, 10])
            .width(Length::Fill)
            .style(input_style)
            .into()
        } else {
            pick_list(MODES, state.mode, move |mode| {
                Message::ModeSelected(pin, mode)
            })
            .placeholder("UNSET")
            .text_size(PIN_CONTROL_TEXT_SIZE)
            .padding([5, 8])
            .width(Length::Fill)
            .into()
        };

        let rw: Element<'_, Message> = if state.mode.is_some_and(Mode::is_input) {
            native_button("Read")
                .on_press_maybe((!state.value_pending).then_some(Message::Read(pin)))
                .into()
        } else if state.mode == Some(Mode::Output) {
            let label = if state.level == Some(Level::High) {
                "Write LOW"
            } else {
                "Write HIGH"
            };
            native_button(label)
                .on_press_maybe((!state.value_pending).then_some(Message::Write(pin)))
                .into()
        } else {
            text("").into()
        };

        let listen: Element<'_, Message> = if state.mode.is_some_and(Mode::is_input) {
            let label = if matches!(state.listener, ListenerState::On | ListenerState::Disabling) {
                "Stop"
            } else {
                "Listen"
            };
            native_button(label)
                .on_press_maybe((!state.listener.is_pending()).then_some(Message::Listen(pin)))
                .into()
        } else {
            text("").into()
        };

        row![
            name,
            pin_cell(mode, PIN_MODE_SHARE),
            level_box(state.level, state.value_pending),
            pin_cell(rw, PIN_RW_SHARE),
            pin_cell(listen, PIN_LISTEN_SHARE),
        ]
        .spacing(CELL_GAP)
        .width(Length::Fill)
        .height(Length::Fixed(ROW_HEIGHT))
        .align_y(iced::Alignment::Center)
        .into()
    }
}

fn pin_header<'a>() -> Element<'a, Message> {
    row![
        pin_cell(text("PIN").size(11), PIN_NAME_SHARE),
        pin_cell(text("MODE").size(11), PIN_MODE_SHARE),
        pin_cell(text("LEVEL").size(11), PIN_STATUS_SHARE),
        pin_cell(text("READ/WRITE").size(10), PIN_RW_SHARE),
        pin_cell(text("LISTEN/STOP").size(10), PIN_LISTEN_SHARE),
    ]
    .spacing(CELL_GAP)
    .width(Length::Fill)
    .into()
}

fn responsive_pin_columns<'a>(
    available_width: f32,
    left: iced::widget::Column<'a, Message>,
    right: iced::widget::Column<'a, Message>,
) -> Element<'a, Message> {
    let content: Element<'a, Message> = if available_width >= PIN_TABLE_TWO_COLUMN_MIN {
        row![
            left.width(Length::FillPortion(1)),
            right.width(Length::FillPortion(1)),
        ]
        .spacing(8)
        .width(Length::Fill)
        .into()
    } else {
        column![left.width(Length::Fill), right.width(Length::Fill)]
            .spacing(8)
            .width(Length::Fill)
            .into()
    };

    scrollable(content)
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
}

fn pin_cell<'a>(content: impl Into<Element<'a, Message>>, share: u16) -> Element<'a, Message> {
    container(content)
        .width(Length::FillPortion(share))
        .height(Length::Fixed(ROW_HEIGHT))
        .align_x(Horizontal::Left)
        .align_y(Vertical::Center)
        .into()
}

fn level_box(level: Option<Level>, pending: bool) -> Element<'static, Message> {
    let label = if pending {
        "…"
    } else {
        match level {
            Some(Level::High) => "HIGH",
            Some(Level::Low) => "LOW",
            None => "—",
        }
    };
    let background = if pending {
        UNSET_BG
    } else {
        match level {
            Some(Level::High) => HIGH_BG,
            Some(Level::Low) => LOW_BG,
            None => UNSET_BG,
        }
    };
    container(text(label).size(11))
        .width(Length::FillPortion(PIN_STATUS_SHARE))
        .height(Length::Fixed(28.0))
        .align_x(Horizontal::Center)
        .align_y(Vertical::Center)
        .style(move |_| container::Style {
            background: Some(Background::Color(background)),
            text_color: Some(UI_TEXT),
            border: Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

pub(super) fn pin_display(pin: &PinInfo) -> String {
    match pin.package_pin {
        Some(package_pin) => format!("{} ({package_pin})", pin.token),
        None => pin.token.clone(),
    }
}
